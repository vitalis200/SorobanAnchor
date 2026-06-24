use alloc::{string::String as RustString, vec::Vec as RustVec};
use soroban_sdk::{
    contract, contractimpl, contracttype, panic_with_error, symbol_short, Address,
    xdr::ToXdr, Bytes, BytesN, Env, IntoVal, String, Symbol, Vec,
};
extern crate alloc;

use crate::deterministic_hash::{compute_payload_hash, make_storage_key, verify_payload_hash};
use crate::errors::ErrorCode;
use crate::rate_limiter::RateLimiter;
use crate::sep10_jwt;
use crate::transaction_state_tracker::{OptRecovery, TransactionState, TransactionStateRecord};
use crate::replay_detection::{self, ReplayMetrics};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone)]
pub struct Session {
    pub session_id: u64,
    pub initiator: Address,
    pub created_at: u64,
    pub nonce: u64,
    pub operation_count: u64,
    pub session_ttl_seconds: u64,
    pub closed: bool,
}

#[contracttype]
#[derive(Clone)]
pub struct Quote {
    pub quote_id: u64,
    pub anchor: Address,
    pub base_asset: String,
    pub quote_asset: String,
    pub rate: u64,
    pub fee_percentage: u32,
    pub minimum_amount: u64,
    pub maximum_amount: u64,
    pub valid_until: u64,
    /// Schema version for this record. See [`SCHEMA_V1`].
    pub schema_version: u32,
    /// Optional routing reason or referral code explaining why this route/anchor
    /// was chosen (e.g. `"lowest_fee"`, `"preferred_anchor"`, `"referral"`).
    /// `None` when no reason was recorded.
    pub routing_reason: Option<String>,
}

#[contracttype]
#[derive(Clone)]
pub struct OperationContext {
    pub session_id: u64,
    pub operation_index: u64,
    pub operation_type: String,
    pub timestamp: u64,
    pub status: String,
    pub result_data: u64,
}

#[contracttype]
#[derive(Clone)]
pub struct AuditLog {
    pub log_id: u64,
    pub session_id: u64,
    pub actor: Address,
    pub operation: OperationContext,
}

#[contracttype]
#[derive(Clone)]
pub struct RequestId {
    pub id: Bytes,
    pub created_at: u64,
}

/// Carries the root request ID and the ordered chain of operation names
/// performed under that root request. Every sub-operation appends its name
/// to `operation_chain` rather than creating a new root ID.
#[contracttype]
#[derive(Clone)]
pub struct RequestContext {
    /// The root request ID that initiated this chain of operations.
    pub root_request_id: RequestId,
    /// Ordered list of operation names performed under this root request.
    pub operation_chain: Vec<String>,
    /// Ledger timestamp when this context was first created.
    pub created_at: u64,
}

#[contracttype]
#[derive(Clone)]
pub struct Attestation {
    pub id: u64,
    pub issuer: Address,
    pub subject: Address,
    pub timestamp: u64,
    pub payload_hash: Bytes,
    pub signature: Bytes,
    /// Schema version for this record. See [`SCHEMA_V1`].
    pub schema_version: u32,
}

#[contracttype]
#[derive(Clone)]
pub struct TracingSpan {
    pub request_id: RequestId,
    pub operation: String,
    pub actor: Address,
    pub started_at: u64,
    pub completed_at: u64,
    pub status: String,
    /// Raw bytes of the parent span's request_id.id, or empty Bytes if this is a root span.
    pub parent_request_id_bytes: Bytes,
    /// Zero-based index of this span within the trace, used for ordering.
    pub span_index: u32,
}

/// Holds the root request ID bytes and the current span index counter for a trace.
#[contracttype]
#[derive(Clone)]
pub struct TracingContext {
    pub root_request_id_bytes: Bytes,
    pub next_span_index: u32,
}

/// Unified attestor profile — single source of truth for all attestor metadata.
///
/// Replaces the separate `ENDPOINT`, `WEBHOOK`, and `SERVICES` storage keys.
/// All profile fields are updated atomically through `set_endpoint`,
/// `register_webhook`, and `configure_services`.
#[contracttype]
#[derive(Clone)]
pub struct AttestorProfile {
    pub attestor: Address,
    /// HTTPS endpoint URL (empty string = not set).
    pub endpoint: String,
    /// Webhook URL (empty string = not set).
    pub webhook_url: String,
    /// Supported service type codes (see `SERVICE_*` constants).
    pub services: Vec<u32>,
    /// Whether this attestor is currently enabled.
    pub enabled: bool,
    /// Ledger timestamp of the last profile update.
    pub updated_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ServiceRetirementInfo {
    pub service_code: u32,
    pub retired: bool,
    pub retirement_timestamp: Option<u64>,
    pub deprecation_notice: Option<String>,
}

#[contracttype]
#[derive(Clone)]
pub struct AnchorServices {
    pub anchor: Address,
    pub services: Vec<u32>,
    /// Schema version of the service-capability set (#239). Records are always
    /// stamped with the version under which they were configured so capability
    /// discovery is explicit and forward-compatible.
    pub service_capability_version: u32,
    /// Retirement metadata for services, indicating if they are deprecated or retired.
    pub service_retirements: Vec<ServiceRetirementInfo>,
}

pub const SERVICE_DEPOSITS: u32 = 1;
pub const SERVICE_WITHDRAWALS: u32 = 2;
pub const SERVICE_QUOTES: u32 = 3;
pub const SERVICE_KYC: u32 = 4;

// ---------------------------------------------------------------------------
// #344 — Admin permission model
//
// Every admin-gated method maps to one of the categories below. The primary
// admin (set during `initialize`) has implicit access to ALL categories.
// Additional addresses may be granted category-scoped roles via `grant_role`.
//
// | Category / Role   | Protected operations                                    |
// |-------------------|---------------------------------------------------------|
// | (primary admin)   | initialize, upgrade, migrate, set_cache_config,         |
// |                   | set_sep10_jwt_verifying_key, rotate_sep10_key,          |
// |                   | set_jwt_max_len, set_jwt_skew, set_rate_limit_config,   |
// |                   | set_anchor_metadata, reactivate_anchor                  |
// | KycAdmin          | approve_kyc, reject_kyc                                 |
// | AttestorAdmin     | register_attestor, revoke_attestor,                     |
// |                   | register_attestor_with_session,                         |
// |                   | revoke_attestor_with_session                            |
// | CacheAdmin        | cache_metadata, cache_metadata_swr, force_refresh_metadata,|
// |                   | refresh_metadata_cache, refresh_metadata_cache_swr,     |
// |                   | cache_capabilities, refresh_capabilities_cache          |
// ---------------------------------------------------------------------------

/// Role-based access control for delegatable admin operations (#345).
///
/// Addresses may be granted a role by the primary admin via [`AnchorKitContract::grant_role`].
/// Role holders can call the operations associated with their role without being
/// the primary admin. The primary admin always passes any role check regardless
/// of explicit grants.
#[contracttype]
#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(u32)]
pub enum AdminRole {
    /// May call `approve_kyc` and `reject_kyc`.
    KycAdmin      = 0,
    /// May call `register_attestor`, `revoke_attestor`, and their session variants.
    AttestorAdmin = 1,
    /// May call all `cache_*` and `refresh_*_cache*` methods.
    CacheAdmin    = 2,
}

/// Current on-chain service-capability schema version (#239).
///
/// This constant gates which service codes the contract recognises and is the
/// anchor point for backwards-compatible evolution of the capability set:
///
/// - **Adding a service identifier** — extend the recognised code range
///   ([`MAX_KNOWN_SERVICE_CODE`]) and bump this constant. New codes then become
///   acceptable to [`configure_services_versioned`].
/// - **Forward safety** — `configure_services_versioned` rejects any version
///   *newer* than this constant, so a contract never stores a capability set it
///   cannot interpret.
/// - **Preserving existing anchors** — records written under an older version
///   stay readable and usable: their codes are always a subset of the current
///   recognised range, so [`supports_service`] and routing keep working without
///   a forced re-configuration.
pub const SERVICE_CAPABILITY_VERSION: u32 = 1;

/// Highest service code recognised by [`SERVICE_CAPABILITY_VERSION`]. Codes
/// outside `SERVICE_DEPOSITS..=MAX_KNOWN_SERVICE_CODE` are rejected by
/// [`configure_services_versioned`]. Extend this (and bump the version) to
/// introduce new service identifiers.
const MAX_KNOWN_SERVICE_CODE: u32 = SERVICE_KYC;

/// Typed representation of a service capability an anchor can support.
///
/// Each variant maps to a stable `u32` discriminant stored on-chain.
/// Use [`ServiceType::as_u32`] to convert before passing to contract functions.
#[derive(Clone, PartialEq)]
pub enum ServiceType {
    Deposits,
    Withdrawals,
    Quotes,
    KYC,
}

impl ServiceType {
    pub fn as_u32(&self) -> u32 {
        match self {
            ServiceType::Deposits => SERVICE_DEPOSITS,
            ServiceType::Withdrawals => SERVICE_WITHDRAWALS,
            ServiceType::Quotes => SERVICE_QUOTES,
            ServiceType::KYC => SERVICE_KYC,
        }
    }
}

// ---------------------------------------------------------------------------
// Routing types
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone)]
pub struct RoutingAnchorMeta {
    pub anchor: Address,
    pub reputation_score: u32,
    pub average_settlement_time: u64,
    pub liquidity_score: u32,
    pub uptime_percentage: u32,
    pub total_volume: u64,
    pub is_active: bool,
}

#[contracttype]
#[derive(Clone)]
pub struct RoutingRequest {
    pub base_asset: String,
    pub quote_asset: String,
    pub amount: u64,
    pub operation_type: u32,
}

#[contracttype]
#[derive(Clone)]
pub struct RoutingOptions {
    pub request: RoutingRequest,
    pub strategy: Vec<Symbol>,
    pub min_reputation: u32,
    pub max_anchors: u32,
    pub require_kyc: bool,
    pub require_compliance: bool,
    pub subject: Address,
}

/// Composite weighted routing strategy.
/// `fee_weight + speed_weight + reputation_weight` must equal 1.0.
pub struct WeightedRoutingStrategy {
    pub fee_weight: f32,
    pub speed_weight: f32,
    pub reputation_weight: f32,
}

impl WeightedRoutingStrategy {
    /// Validate that weights sum to 1.0 (within floating-point tolerance).
    pub fn validate(&self) -> bool {
        let sum = self.fee_weight + self.speed_weight + self.reputation_weight;
        (sum - 1.0_f32).abs() < 1e-4
    }

    /// Compute a normalized composite score in [0.0, 1.0].
    /// Lower fee and faster settlement are better; higher reputation is better.
    /// Each dimension is normalised against the provided max values.
    pub fn score_anchor(
        &self,
        fee_pct: u32,
        settlement_time: u64,
        reputation: u32,
        max_fee: u32,
        max_settlement: u64,
        max_reputation: u32,
    ) -> f32 {
        let fee_score = if max_fee == 0 {
            1.0_f32
        } else {
            1.0_f32 - (fee_pct as f32 / max_fee as f32)
        };
        let speed_score = if max_settlement == 0 {
            1.0_f32
        } else {
            1.0_f32 - (settlement_time as f32 / max_settlement as f32)
        };
        let rep_score = if max_reputation == 0 {
            0.0_f32
        } else {
            reputation as f32 / max_reputation as f32
        };
        self.fee_weight * fee_score
            + self.speed_weight * speed_score
            + self.reputation_weight * rep_score
    }
}

// ---------------------------------------------------------------------------
// KYC and Compliance types
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct CompliancePolicy {
    pub minimum_score: Option<u32>,
}

impl CompliancePolicy {
    pub fn default_policy() -> Self {
        CompliancePolicy { minimum_score: None }
    }
}

#[contracttype]
#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(u32)]
pub enum KycStatus {
    NotSubmitted = 0,
    Pending = 1,
    Approved = 2,
    Rejected = 3,
    Expired = 4,
}

#[contracttype]
#[derive(Clone)]
pub struct ComplianceCheck {
    pub subject: Address,
    pub check_type: String,
    pub result: u32,
    pub score: Option<u32>,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone)]
pub struct KycRecord {
    pub subject: Address,
    pub status: u32,
    pub submitted_at: u64,
    pub reviewed_at: Option<u64>,
    pub expiry: Option<u64>,
    pub rejection_reason_hash: Option<Bytes>,
    /// Schema version for this record. See [`SCHEMA_V1`].
    pub schema_version: u32,
}

// ---------------------------------------------------------------------------
// Anchor Blacklist and Clustering (#296)
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone)]
pub struct AnchorBlacklistEntry {
    pub anchor: Address,
    pub reason: String,
    pub blacklisted_at: u64,
}

#[contracttype]
#[derive(Clone)]
pub struct AnchorCluster {
    pub cluster_id: String,
    pub name: String,
    pub anchors: Vec<Address>,
    pub created_at: u64,
}

// ---------------------------------------------------------------------------
// Health check types (#268)
// ---------------------------------------------------------------------------

/// Overall contract health state returned by [`AnchorKitContract::get_health_status`].
#[contracttype]
#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(u32)]
pub enum HealthStatus {
    /// Contract is initialized and all subsystems are operational.
    Healthy = 0,
    /// Contract is initialized but one or more subsystems are using fallback defaults.
    Degraded = 1,
    /// Contract has not been initialized.
    Unavailable = 2,
}

/// Metadata freshness report returned by [`AnchorKitContract::get_metadata_freshness`].
#[contracttype]
#[derive(Clone)]
pub struct MetadataFreshnessReport {
    pub anchor: Address,
    pub state: MetadataCacheState,
    /// Age of the cached entry in seconds (0 when missing).
    pub age_seconds: u64,
    /// Whether a background refresh is recommended.
    pub needs_refresh: bool,
}

/// Rate limiter health report returned by [`AnchorKitContract::get_rate_limiter_health`].
#[contracttype]
#[derive(Clone)]
pub struct RateLimiterHealth {
    pub attestor: Address,
    /// Effective submission count in the current window (0 if window expired).
    pub submission_count: u32,
    pub max_submissions: u32,
    pub window_length: u32,
    pub window_start_ledger: u32,
    /// `true` when the attestor has reached the submission limit.
    pub is_throttled: bool,
}

// ---------------------------------------------------------------------------
// Metadata cache types
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, PartialEq)]
pub struct AnchorMetadata {
    pub anchor: Address,
    pub reputation_score: u32,
    pub liquidity_score: u32,
    pub uptime_percentage: u32,
    pub total_volume: u64,
    pub average_settlement_time: u64,
    pub is_active: bool,
}

#[contracttype]
#[derive(Clone)]
pub struct MetadataCache {
    pub metadata: AnchorMetadata,
    pub cached_at: u64,
    pub ttl_seconds: u64,
    /// Grace period after `ttl_seconds` during which stale data may be served.
    pub stale_ttl_seconds: u64,
    /// Set to `true` when the entry is within the stale window; caller should refresh.
    pub needs_refresh: bool,
}

/// Explicit lifecycle state of a metadata cache entry under the
/// stale-while-revalidate (SWR) policy. Returned by
/// [`AnchorKitContract::get_metadata_cache_state`] so callers can branch on
/// freshness without triggering a panic on an expired/absent entry.
#[contracttype]
#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(u32)]
pub enum MetadataCacheState {
    /// No entry exists for the anchor.
    Missing = 0,
    /// Within the primary TTL — safe to use as-is.
    Fresh = 1,
    /// Past the primary TTL but within the stale grace window — usable, but the
    /// caller should kick off a background refresh.
    Stale = 2,
    /// Past both the primary TTL and the stale window — must not be served.
    Expired = 3,
}

#[contracttype]
#[derive(Clone)]
pub struct CapabilitiesCache {
    pub toml_url: String,
    pub capabilities: String,
    pub cached_at: u64,
    pub ttl_seconds: u64,
}

#[contracttype]
#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(u32)]
pub enum RefreshStatus {
    Success = 1,
    Failed = 2,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct RefreshDiagnostic {
    pub operation: String,
    pub status: RefreshStatus,
    pub attempted_at: u64,
    pub had_cached_entry: bool,
    pub detail: String,
}

// ---------------------------------------------------------------------------
// Anchor Info Discovery types
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone)]
pub struct AssetInfo {
    pub code: String,
    pub issuer: String,
    pub deposit_enabled: bool,
    pub withdrawal_enabled: bool,
    pub deposit_fee_fixed: u64,
    pub deposit_fee_percent: u32,
    pub withdrawal_fee_fixed: u64,
    pub withdrawal_fee_percent: u32,
    pub deposit_min_amount: u64,
    pub deposit_max_amount: u64,
    pub withdrawal_min_amount: u64,
    pub withdrawal_max_amount: u64,
}

#[contracttype]
#[derive(Clone)]
pub struct StellarToml {
    pub version: String,
    pub network_passphrase: String,
    pub accounts: Vec<String>,
    pub signing_key: String,
    pub currencies: Vec<AssetInfo>,
    pub transfer_server: String,
    pub transfer_server_sep0024: String,
    pub kyc_server: String,
    pub web_auth_endpoint: String,
}

#[contracttype]
#[derive(Clone)]
pub struct CachedToml {
    pub toml: StellarToml,
    pub cached_at: u64,
    pub ttl_seconds: u64,
}

const MIN_TEMP_TTL: u32 = 15; // min_temp_entry_ttl - 1

// ---------------------------------------------------------------------------
// #244 — Contract-level cache configuration
// ---------------------------------------------------------------------------

/// Central cache TTL configuration stored in contract instance storage.
///
/// All cache operations read these values as defaults. Callers may still pass
/// an explicit TTL override (non-zero) to `cache_metadata` / `cache_capabilities`
/// / `fetch_anchor_info`; a zero override means "use the configured default".
///
/// Fields:
/// - `metadata_ttl_seconds`     — primary TTL for anchor metadata entries.
/// - `capabilities_ttl_seconds` — primary TTL for capabilities / stellar.toml entries.
/// - `swr_ttl_seconds`          — stale-while-revalidate grace period appended after
///                                the primary TTL before an entry is fully expired.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct CacheConfig {
    pub metadata_ttl_seconds: u64,
    pub capabilities_ttl_seconds: u64,
    pub swr_ttl_seconds: u64,
}

impl CacheConfig {
    /// Sensible production defaults: 1 h metadata, 6 h capabilities, 5 min SWR.
    pub fn default_config() -> Self {
        CacheConfig {
            metadata_ttl_seconds: 3_600,
            capabilities_ttl_seconds: 21_600,
            swr_ttl_seconds: 300,
        }
    }
}

/// Capacity limits for registrations and caches.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct CapacityConfig {
    pub max_attestors: u64,
    pub max_cache_entries: u64,
}

impl CapacityConfig {
    /// Sensible production defaults: 1000 attestors, 10000 cache entries.
    pub fn default_config() -> Self {
        CapacityConfig {
            max_attestors: 1000,
            max_cache_entries: 10000,
        }
    }
}

// ---------------------------------------------------------------------------
// #348 — Anchor health and service readiness types
// ---------------------------------------------------------------------------

/// Readiness snapshot for an anchor, indicating which services are available.
#[contracttype]
#[derive(Clone)]
pub struct AnchorReadinessReport {
    pub anchor: Address,
    /// True when the anchor is a registered attestor.
    pub is_registered: bool,
    /// True when the anchor has the deposit service configured.
    pub deposit_ready: bool,
    /// True when the anchor has the withdrawal service configured.
    pub withdrawal_ready: bool,
    /// True when the anchor has the quote service configured and holds a
    /// currently valid (non-expired) quote.
    pub quote_ready: bool,
    /// True when the anchor has the KYC service configured.
    pub kyc_ready: bool,
    /// Ledger timestamp when this report was generated.
    pub checked_at: u64,
}

// ---------------------------------------------------------------------------
// #350 — Read-only diagnostic types
// ---------------------------------------------------------------------------

/// Read-only snapshot of the rate limiter state for a specific attestor.
#[contracttype]
#[derive(Clone)]
pub struct RateLimiterDiagnostics {
    pub attestor: Address,
    /// Submissions recorded in the current window.
    pub submission_count: u32,
    /// Ledger sequence number when the current window started.
    pub window_start_ledger: u32,
    /// Maximum submissions allowed per window.
    pub max_submissions: u32,
    /// Length of the sliding window in ledgers.
    pub window_length: u32,
    /// True when the attestor has reached the per-window limit.
    pub is_at_limit: bool,
    /// Ledger timestamp when this snapshot was taken.
    pub checked_at: u64,
}

/// Read-only snapshot of the metadata and capabilities cache for an anchor.
#[contracttype]
#[derive(Clone)]
pub struct CacheDiagnostics {
    pub anchor: Address,
    /// True when a metadata entry is present in the cache.
    pub metadata_cached: bool,
    /// Seconds elapsed since the metadata entry was cached (0 if absent).
    pub metadata_age_seconds: u64,
    /// Configured TTL for the metadata entry (0 if absent).
    pub metadata_ttl_seconds: u64,
    /// True when a capabilities entry is present in the cache.
    pub capabilities_cached: bool,
    /// Seconds elapsed since the capabilities entry was cached (0 if absent).
    pub capabilities_age_seconds: u64,
    /// Configured TTL for the capabilities entry (0 if absent).
    pub capabilities_ttl_seconds: u64,
    /// Ledger timestamp when this snapshot was taken.
    pub checked_at: u64,
}

/// Read-only snapshot of session counters.
#[contracttype]
#[derive(Clone)]
pub struct SessionDiagnostics {
    /// Total number of sessions created since contract initialization.
    pub total_sessions_created: u64,
    /// Ledger timestamp when this snapshot was taken.
    pub checked_at: u64,
}

/// Aggregated read-only health snapshot for the contract's key subsystems.
#[contracttype]
#[derive(Clone)]
pub struct ContractDiagnostics {
    /// True when the contract has been initialized.
    pub is_initialized: bool,
    /// Total attestations submitted since initialization.
    pub total_attestations: u64,
    /// Total quotes submitted since initialization.
    pub total_quotes: u64,
    /// Total sessions created since initialization.
    pub total_sessions: u64,
    /// Active rate limit: max submissions per window.
    pub rate_limit_max_submissions: u32,
    /// Active rate limit: window length in ledgers.
    pub rate_limit_window_length: u32,
    /// Ledger timestamp when this snapshot was taken.
    pub checked_at: u64,
}

// ---------------------------------------------------------------------------
// Anchor health metrics types
// ---------------------------------------------------------------------------

/// Accumulated endpoint health counters for a single anchor.
///
/// Written by [`AnchorKitContract::record_health_event`] and read by
/// [`AnchorKitContract::get_anchor_health`].
///
/// `uptime_bps` is derived on read: `success_count * 10_000 / total_calls`
/// (basis points, 0–10 000). Returns 0 when `total_calls == 0`.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AnchorHealthMetrics {
    pub anchor: Address,
    /// Total successful endpoint calls recorded.
    pub success_count: u64,
    /// Total failed endpoint calls recorded.
    pub failure_count: u64,
    /// Total calls (`success_count + failure_count`).
    pub total_calls: u64,
    /// Uptime in basis points (0–10 000). 10 000 = 100 %.
    pub uptime_bps: u32,
    /// Ledger timestamp of the most recent recorded event (0 if none).
    pub last_event_at: u64,
}

// ---------------------------------------------------------------------------
// Proof-of-possession types
// ---------------------------------------------------------------------------

/// On-chain record of an anchor's proof-of-possession for an endpoint.
///
/// The anchor submits a SHA-256 hash of `challenge || endpoint` (where
/// `challenge` is a nonce the anchor fetches from its own stellar.toml or
/// metadata endpoint). The contract stores the hash; callers verify by
/// recomputing it off-chain and calling
/// [`AnchorKitContract::verify_endpoint_proof`].
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AnchorProofRecord {
    pub anchor: Address,
    /// The endpoint URL this proof covers.
    pub endpoint: String,
    /// SHA-256(challenge_bytes || endpoint_bytes) submitted by the anchor.
    pub proof_hash: BytesN<32>,
    /// Ledger timestamp when the proof was registered.
    pub registered_at: u64,
    /// True once the proof has been successfully verified by a caller.
    pub verified: bool,
}

// ---------------------------------------------------------------------------
// #247 — on-chain schema versioning
//
// Each persistent contract type carries a `schema_version: u32` field.
// The version is bumped only when the serialized shape changes in a way that
// is incompatible with old stored values (e.g. a field is added or removed).
//
// Version history:
//   SCHEMA_V1 = 1  — initial versioned layout (introduced in this release)
//
// Migration strategy:
//   After a WASM upgrade that increments a schema version, call `migrate()`
//   as admin. The migrate function reads entries with the old schema version
//   and rewrites them with the new one.  Because Soroban XDR decoding is
//   strict, old unversioned values (implicitly "V0") will fail to decode into
//   the new type; the migration must handle that by catching panics or by
//   storing a versioned wrapper enum around the concrete type.
// ---------------------------------------------------------------------------

/// Schema version written into every new [`Attestation`], [`Quote`], and
/// [`KycRecord`].  Consumers should compare against this constant when reading
/// stored data to detect version skew.
pub const SCHEMA_V1: u32 = 1;

/// Aggregated transaction counts returned by
/// [`AnchorKitContract::summarize_transactions_by_status`].
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct TransactionStatusSummary {
    pub pending_count: u64,
    pub in_progress_count: u64,
    pub completed_count: u64,
    pub failed_count: u64,
    pub total_count: u64,
}

/// A single versioned snapshot of anchor metadata, stored in the history log.
///
/// Written by [`AnchorKitContract::set_anchor_metadata`] each time the metadata
/// changes. The `version` field is a monotonically increasing counter scoped to
/// the anchor; `updated_at` is the ledger timestamp of the write.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AnchorMetadataVersion {
    /// Monotonically increasing version number (1-based).
    pub version: u32,
    /// Ledger timestamp when this version was written.
    pub updated_at: u64,
    pub reputation_score: u32,
    pub average_settlement_time: u64,
    pub liquidity_score: u32,
    pub uptime_percentage: u32,
    pub total_volume: u64,
    pub is_active: bool,
}

// ---------------------------------------------------------------------------
// Event structs
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone)]
struct SessionCreatedEvent {
    session_id: u64,
    initiator: Address,
    timestamp: u64,
}

#[contracttype]
#[derive(Clone)]
struct SessionClosedEvent {
    session_id: u64,
    initiator: Address,
    timestamp: u64,
}

#[contracttype]
#[derive(Clone)]
struct QuoteSubmitEvent {
    quote_id: u64,
    anchor: Address,
    base_asset: String,
    quote_asset: String,
    rate: u64,
    valid_until: u64,
    /// Optional routing reason recorded at quote submission time.
    routing_reason: Option<String>,
}

#[contracttype]
#[derive(Clone)]
struct QuoteReceivedEvent {
    quote_id: u64,
    receiver: Address,
    timestamp: u64,
}

#[contracttype]
#[derive(Clone)]
struct AuditLogEvent {
    log_id: u64,
    session_id: u64,
    operation_index: u64,
    operation_type: String,
    status: String,
}

#[contracttype]
#[derive(Clone)]
struct AttestEvent {
    payload_hash: Bytes,
    timestamp: u64,
}

#[contracttype]
#[derive(Clone)]
pub struct EndpointUpdated {
    pub attestor: Address,
    pub endpoint: String,
}

#[contracttype]
#[derive(Clone)]
struct TxStateChangedEvent {
    transaction_id: u64,
    old_state: u32,
    new_state: u32,
    timestamp: u64,
}

#[contracttype]
#[derive(Clone)]
struct WebhookEvent {
    event_type: String,
    transaction_id: u64,
    timestamp: u64,
    payload_hash: Bytes,
}

// ---------------------------------------------------------------------------
// Contract upgrade types (#200)
// Provides admin-controlled WASM upgrade with version tracking and audit events.
// ---------------------------------------------------------------------------

/// Semantic version stored in persistent contract storage after each upgrade.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ContractVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    /// Ledger timestamp of the most recent upgrade (0 = never upgraded).
    pub upgraded_at: u64,
}

impl ContractVersion {
    /// Increment the patch component and record the upgrade timestamp.
    pub fn bump_patch(self, upgraded_at: u64) -> Self {
        ContractVersion {
            major: self.major,
            minor: self.minor,
            patch: self.patch + 1,
            upgraded_at,
        }
    }
}

/// Event emitted after a successful contract upgrade.
#[contracttype]
#[derive(Clone)]
struct UpgradeEvent {
    old_wasm_hash: BytesN<32>,
    new_wasm_hash: BytesN<32>,
    new_major: u32,
    new_minor: u32,
    new_patch: u32,
    upgraded_at: u64,
}

// ---------------------------------------------------------------------------
// TTLs (in ledgers)
// ---------------------------------------------------------------------------
const PERSISTENT_TTL: u32 = 1_555_200;
const SPAN_TTL: u32 = 17_280;
const INSTANCE_TTL: u32 = 518_400;

/// Default session lifetime in seconds (1 hour). Used when session_ttl_seconds is zero.
pub const DEFAULT_SESSION_TTL: u64 = 3600;

/// Maximum operations allowed per session before it is considered exhausted.
pub const MAX_OPS_PER_SESSION: u64 = 100;

/// Minimum TTL for replay-protection entries (7 days in ledgers at ~5 s/ledger).
pub const REPLAY_TTL: u32 = 120_960;

/// Default lifetime for an approved KYC record before the approval expires.
const KYC_EXPIRY_SECONDS: u64 = 30 * 24 * 60 * 60; // 30 days

fn current_kyc_status(env: &Env, record: &KycRecord) -> KycStatus {
    if let Some(expiry) = record.expiry {
        if env.ledger().timestamp() > expiry {
            return KycStatus::Expired;
        }
    }
    match record.status {
        0 => KycStatus::NotSubmitted,
        1 => KycStatus::Pending,
        2 => KycStatus::Approved,
        3 => KycStatus::Rejected,
        4 => KycStatus::Expired,
        _ => KycStatus::NotSubmitted,
    }
}

fn validate_kyc_transition(current: KycStatus, next: KycStatus, record: &KycRecord, now: u64) -> bool {
    if next == KycStatus::Pending && now.saturating_sub(record.submitted_at) < 86400 {
        return false;
    }

    match (current, next) {
        (KycStatus::NotSubmitted, KycStatus::Pending) => true,
        (KycStatus::Expired, KycStatus::Pending) => true,
        (KycStatus::Rejected, KycStatus::Pending) => true,
        (KycStatus::Pending, KycStatus::Approved) => true,
        (KycStatus::Pending, KycStatus::Rejected) => true,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Storage key helpers — all keys go through make_storage_key for collision
// resistance (#229). Each namespace uses a unique prefix byte slice.
// ---------------------------------------------------------------------------

fn admin_key(env: &Env) -> BytesN<32> {
    make_storage_key(env, &[b"ADMIN"])
}

fn initialized_key(env: &Env) -> BytesN<32> {
    make_storage_key(env, &[b"INITIALIZED"])
}

fn kyc_record_key(env: &Env, subject: &Address) -> BytesN<32> {
    let xdr = subject.clone().to_xdr(env);
    let raw = xdr_to_vec(&xdr);
    make_storage_key(env, &[b"KYC", &raw])
}

fn compliance_check_key(env: &Env, subject: &Address, check_type: &String) -> BytesN<32> {
    let xdr = subject.clone().to_xdr(env);
    let raw = xdr_to_vec(&xdr);
    let ct_xdr = check_type.clone().to_xdr(env);
    let ct_bytes = xdr_to_vec(&ct_xdr);
    make_storage_key(env, &[b"COMP", &raw, &ct_bytes])
}

fn compliance_history_count_key(env: &Env, subject: &Address, check_type: &String) -> BytesN<32> {
    let xdr = subject.clone().to_xdr(env);
    let raw = xdr_to_vec(&xdr);
    let ct_xdr = check_type.clone().to_xdr(env);
    let ct_bytes = xdr_to_vec(&ct_xdr);
    make_storage_key(env, &[b"COMPHCNT", &raw, &ct_bytes])
}

fn compliance_history_entry_key(env: &Env, subject: &Address, check_type: &String, idx: u64) -> BytesN<32> {
    let xdr = subject.clone().to_xdr(env);
    let raw = xdr_to_vec(&xdr);
    let ct_xdr = check_type.clone().to_xdr(env);
    let ct_bytes = xdr_to_vec(&ct_xdr);
    make_storage_key(env, &[b"COMPHIST", &raw, &ct_bytes, &idx.to_be_bytes()])
}

fn compliance_subject_index_key(env: &Env, subject: &Address) -> BytesN<32> {
    let xdr = subject.clone().to_xdr(env);
    let raw = xdr_to_vec(&xdr);
    make_storage_key(env, &[b"COMPIDX", &raw])
}

fn audit_retention_key(env: &Env) -> BytesN<32> {
    make_storage_key(env, &[b"AUDITRET"])
}

fn anchor_meta_key(env: &Env, anchor: &Address) -> BytesN<32> {
    let xdr = anchor.clone().to_xdr(env);
    let raw = xdr_to_vec(&xdr);
    make_storage_key(env, &[b"ANCHMETA", &raw])
}

fn anchor_blacklist_key(env: &Env, anchor: &Address) -> BytesN<32> {
    let xdr = anchor.clone().to_xdr(env);
    let raw = xdr_to_vec(&xdr);
    make_storage_key(env, &[b"BLACKLIST", &raw])
}

fn anchor_cluster_key(env: &Env, cluster_id: &String) -> BytesN<32> {
    let xdr = cluster_id.clone().to_xdr(env);
    let raw = xdr_to_vec(&xdr);
    make_storage_key(env, &[b"CLUSTER", &raw])
}

fn anchor_cluster_list_key(env: &Env) -> BytesN<32> {
    make_storage_key(env, &[b"CLUSTERLIST"])
}

/// Convert a Soroban `Bytes` value to a native `Vec<u8>` for use in key helpers.
fn xdr_to_vec(b: &Bytes) -> alloc::vec::Vec<u8> {
    let mut v = alloc::vec::Vec::with_capacity(b.len() as usize);
    for i in 0..b.len() {
        v.push(b.get(i).unwrap_or(0));
    }
    v
}

/// Storage key for a specific `(role, grantee)` pair.
fn role_key(env: &Env, role: AdminRole, grantee: &Address) -> BytesN<32> {
    let xdr = grantee.clone().to_xdr(env);
    let raw = xdr_to_vec(&xdr);
    let role_byte = [role as u32 as u8];
    make_storage_key(env, &[b"ROLESET", &role_byte, &raw])
}

fn anchor_meta_opt(env: &Env, anchor: &Address) -> Option<RoutingAnchorMeta> {
    env.storage().persistent().get(&anchor_meta_key(env, anchor))
}

// ---------------------------------------------------------------------------
// #245 — fee and limit validation helpers
//
// These are free functions (not contract methods) so they can be called from
// multiple contract entry-points without requiring `Self`.
// ---------------------------------------------------------------------------

/// Panic with [`ErrorCode::InvalidQuote`] when `fee` exceeds 100 % (10 000 bps).
fn validate_fee_percent(env: &Env, fee: u32) {
    if fee > 10_000 {
        panic_with_error!(env, ErrorCode::InvalidQuote);
    }
}

/// Panic with [`ErrorCode::InvalidQuote`] when `max_amount` is non-zero and
/// less than `min_amount` (inverted limit range).
fn validate_amount_limits(env: &Env, min_amount: u64, max_amount: u64) {
    if max_amount != 0 && min_amount > max_amount {
        panic_with_error!(env, ErrorCode::InvalidQuote);
    }
}

/// Panic with [`ErrorCode::InvalidAssetCode`] when `code` is empty, longer
/// than 12 characters, or contains non-alphanumeric characters.
fn validate_currency_code(env: &Env, code: &String) {
    let len = code.len();
    if len == 0 || len > 12 {
        panic_with_error!(env, ErrorCode::InvalidAssetCode);
    }
}

/// Validate all fee and limit fields of a single [`AssetInfo`] record.
fn validate_asset_info(env: &Env, asset: &AssetInfo) {
    validate_currency_code(env, &asset.code);
    validate_fee_percent(env, asset.deposit_fee_percent);
    validate_fee_percent(env, asset.withdrawal_fee_percent);
    validate_amount_limits(env, asset.deposit_min_amount, asset.deposit_max_amount);
    validate_amount_limits(env, asset.withdrawal_min_amount, asset.withdrawal_max_amount);
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct AnchorKitContract;

#[contractimpl]
impl AnchorKitContract {
    // -----------------------------------------------------------------------
    // Initialization
    // -----------------------------------------------------------------------

    /// Initialize the contract with an admin address.
    ///
    /// Sets up the contract instance and persistent storage. Must be called exactly once
    /// before any other contract operations. Subsequent calls will panic.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    /// * `admin` - The address that will have admin privileges. Must authorize this call.
    ///
    /// # Authorization
    ///
    /// Requires the `admin` address to sign the transaction.
    ///
    /// # Errors
    ///
    /// Panics with [`ErrorCode::AlreadyInitialized`] if the contract has already been initialized.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use soroban_sdk::{Address, Env};
    /// use anchorkit::AnchorKitContract;
    ///
    /// let env = Env::default();
    /// let admin = Address::random(&env);
    /// AnchorKitContract::initialize(env, admin);
    /// ```
    pub fn initialize(env: Env, admin: Address) {
        admin.require_auth();
        // #228: dedicated initialized flag in persistent storage prevents
        // re-initialization after upgrade.
        let init_key = initialized_key(&env);
        if env.storage().persistent().has(&init_key) {
            panic_with_error!(&env, ErrorCode::AlreadyInitialized);
        }
        env.storage().persistent().set(&init_key, &true);
        env.storage().persistent().extend_ttl(&init_key, PERSISTENT_TTL, PERSISTENT_TTL);
        env.storage().instance().set(&admin_key(&env), &admin);
        env.storage().instance().extend_ttl(INSTANCE_TTL, INSTANCE_TTL);
    }

    /// Check if the contract has been initialized.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    ///
    /// # Returns
    ///
    /// `true` if [`initialize`](Self::initialize) has been called successfully, `false` otherwise.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use soroban_sdk::Env;
    /// use anchorkit::AnchorKitContract;
    ///
    /// let env = Env::default();
    /// let initialized = AnchorKitContract::is_initialized(env);
    /// assert!(initialized);
    /// ```
    pub fn is_initialized(env: Env) -> bool {
        env.storage()
            .instance()
            .get::<_, bool>(&symbol_short!("INITED"))
            .unwrap_or(false)
    }

    /// Retrieve the current admin address.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    ///
    /// # Returns
    ///
    /// The [`Address`] of the current admin.
    ///
    /// # Errors
    ///
    /// Panics with [`ErrorCode::NotInitialized`] if the contract has not been initialized.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use soroban_sdk::Env;
    /// use anchorkit::AnchorKitContract;
    ///
    /// let env = Env::default();
    /// let admin = AnchorKitContract::get_admin(env);
    /// ```
    pub fn get_admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get::<_, Address>(&admin_key(&env))
            .unwrap_or_else(|| panic_with_error!(&env, ErrorCode::NotInitialized))
    }

    // -----------------------------------------------------------------------
    // Role-based access control (#345)
    // -----------------------------------------------------------------------

    /// Grant `role` to `grantee`. Only the primary admin may call this.
    ///
    /// After this call `grantee` may invoke the operations protected by `role`
    /// without being the primary admin.  Granting a role that is already held
    /// is a no-op.
    pub fn grant_role(env: Env, grantee: Address, role: AdminRole) {
        Self::require_admin(&env);
        let key = role_key(&env, role, &grantee);
        env.storage().persistent().set(&key, &true);
        env.storage().persistent().extend_ttl(&key, PERSISTENT_TTL, PERSISTENT_TTL);
        env.events().publish(
            (symbol_short!("role"), symbol_short!("granted"), grantee),
            role as u32,
        );
    }

    /// Revoke `role` from `grantee`. Only the primary admin may call this.
    ///
    /// Revoking a role that was never granted is a no-op.
    pub fn revoke_role(env: Env, grantee: Address, role: AdminRole) {
        Self::require_admin(&env);
        let key = role_key(&env, role, &grantee);
        if env.storage().persistent().has(&key) {
            env.storage().persistent().remove(&key);
        }
        env.events().publish(
            (symbol_short!("role"), symbol_short!("revoked"), grantee),
            role as u32,
        );
    }

    /// Returns `true` if `address` holds `role` or is the primary admin.
    pub fn has_role(env: Env, address: Address, role: AdminRole) -> bool {
        Self::has_role_internal(&env, &address, role)
    }

    // -----------------------------------------------------------------------
    // Contract upgrade (#200)
    // -----------------------------------------------------------------------

    /// Storage key for the contract version record.
    fn version_key(env: &Env) -> BytesN<32> {
        make_storage_key(env, &[b"VERSION"])
    }

    /// Retrieve the current contract version.
    ///
    /// Returns semantic version information and the timestamp of the last upgrade.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    ///
    /// # Returns
    ///
    /// A [`ContractVersion`] struct containing:
    /// - `major`, `minor`, `patch` - semantic version components
    /// - `upgraded_at` - ledger timestamp of the last upgrade (0 if never upgraded)
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use soroban_sdk::Env;
    /// use anchorkit::AnchorKitContract;
    ///
    /// let env = Env::default();
    /// let version = AnchorKitContract::get_version(env);
    /// println!("Version: {}.{}.{}", version.major, version.minor, version.patch);
    /// ```
    pub fn get_version(env: Env) -> ContractVersion {
        env.storage()
            .instance()
            .get::<_, ContractVersion>(&Self::version_key(&env))
            .unwrap_or(ContractVersion {
                major: 0,
                minor: 1,
                patch: 0,
                upgraded_at: 0,
            })
    }

    /// Upgrade the contract WASM code to a new version.
    ///
    /// Atomically updates the contract bytecode, increments the patch version, and emits
    /// an upgrade event. The contract must be initialized and the caller must be the admin.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    /// * `new_wasm_hash` - The SHA-256 hash of the new WASM bytecode.
    ///
    /// # Authorization
    ///
    /// Requires admin authorization.
    ///
    /// # Errors
    ///
    /// Panics with:
    /// - [`ErrorCode::NotInitialized`] if the contract has not been initialized.
    /// - [`ErrorCode::UnauthorizedAttestor`] if the caller is not the admin.
    ///
    /// # Side effects
    ///
    /// - Increments the patch version component.
    /// - Records the upgrade timestamp.
    /// - Emits an `UpgradeEvent` with old/new WASM hashes and version info.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use soroban_sdk::{Env, BytesN};
    /// use anchorkit::AnchorKitContract;
    ///
    /// let env = Env::default();
    /// let new_hash = BytesN::from_array(&env, &[0u8; 32]);
    /// AnchorKitContract::upgrade(env, new_hash);
    /// ```
    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) {
        // #228: must be initialized before upgrade is permitted
        if !env.storage().persistent().has(&initialized_key(&env)) {
            panic_with_error!(&env, ErrorCode::NotInitialized);
        }
        Self::require_admin(&env);

        let now = env.ledger().timestamp();
        let old_version = Self::get_version(env.clone());

        let old_hash_key = make_storage_key(&env, &[b"OLDHASH"]);
        let old_wasm_hash: BytesN<32> = env
            .storage()
            .instance()
            .get::<_, BytesN<32>>(&old_hash_key)
            .unwrap_or_else(|| BytesN::from_array(&env, &[0u8; 32]));

        env.deployer().update_current_contract_wasm(new_wasm_hash.clone());

        let new_version = old_version.bump_patch(now);
        env.storage()
            .instance()
            .set(&Self::version_key(&env), &new_version);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL, INSTANCE_TTL);

        env.storage()
            .instance()
            .set(&old_hash_key, &new_wasm_hash.clone());

        env.events().publish(
            (symbol_short!("contract"), symbol_short!("upgraded")),
            UpgradeEvent {
                old_wasm_hash,
                new_wasm_hash,
                new_major: new_version.major,
                new_minor: new_version.minor,
                new_patch: new_version.patch,
                upgraded_at: now,
            },
        );
    }

    /// Run post-upgrade migration logic (idempotent).
    ///
    /// Called after a contract upgrade to perform any necessary data migrations or
    /// initialization of new storage fields. This function is idempotent — calling it
    /// multiple times has the same effect as calling it once.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    ///
    /// # Authorization
    ///
    /// Requires admin authorization.
    ///
    /// # Errors
    ///
    /// Panics with [`ErrorCode::NotInitialized`] if the contract has not been initialized.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use soroban_sdk::Env;
    /// use anchorkit::AnchorKitContract;
    ///
    /// let env = Env::default();
    /// AnchorKitContract::migrate(env);
    /// ```
    pub fn migrate(env: Env, new_schema_version: u32) {
        // migrate must not run before initialization
        if !env.storage().persistent().has(&initialized_key(&env)) {
            panic_with_error!(&env, ErrorCode::NotInitialized);
        }
        Self::require_admin(&env);

        // Version must be positive
        if new_schema_version == 0 {
            panic_with_error!(&env, ErrorCode::ValidationError);
        }

        // Version must advance
        let schema_key = make_storage_key(&env, &[b"SCHEMAVER"]);
        let current: u32 = env.storage().instance().get(&schema_key).unwrap_or(0);
        if new_schema_version <= current {
            panic_with_error!(&env, ErrorCode::ValidationError);
        }

        env.storage().instance().set(&schema_key, &new_schema_version);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL, INSTANCE_TTL);
    }

    /// Get the current on-chain data schema version.
    ///
    /// Returns the stored schema version (set via `migrate`), or 0 before any
    /// migration has been run.
    pub fn get_schema_version(env: Env) -> u32 {
        let schema_key = make_storage_key(&env, &[b"SCHEMAVER"]);
        env.storage().instance().get(&schema_key).unwrap_or(0)
    }

    // -----------------------------------------------------------------------
    // Cache configuration (#244)
    // -----------------------------------------------------------------------

    fn cache_config_key(env: &Env) -> soroban_sdk::Vec<soroban_sdk::Symbol> {
        soroban_sdk::vec![env, symbol_short!("CACHCFG")]
    }

    fn compliance_policy_key(env: &Env) -> soroban_sdk::Vec<soroban_sdk::Symbol> {
        soroban_sdk::vec![env, symbol_short!("COMPPOL")]
    }

    /// Set the global compliance policy.
    ///
    /// Configures minimum score requirements for compliance checks.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    /// * `policy` - A [`CompliancePolicy`] struct with:
    ///   - `minimum_score` - Optional minimum score requirement for compliance checks
    ///
    /// # Authorization
    ///
    /// Requires admin authorization.
    pub fn set_compliance_policy(env: Env, policy: CompliancePolicy) {
        Self::require_admin(&env);
        env.storage().instance().set(&Self::compliance_policy_key(&env), &policy);
        env.storage().instance().extend_ttl(INSTANCE_TTL, INSTANCE_TTL);
    }

    /// Get the current global compliance policy.
    ///
    /// Returns the active compliance policy, or the default policy if no
    /// configuration has been set.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    ///
    /// # Returns
    ///
    /// A [`CompliancePolicy`] struct with the current policy settings.
    pub fn get_compliance_policy(env: Env) -> CompliancePolicy {
        env.storage()
            .instance()
            .get::<_, CompliancePolicy>(&Self::compliance_policy_key(&env))
            .unwrap_or_else(CompliancePolicy::default_policy)
    }

    /// Set the global cache configuration.
    ///
    /// Configures default TTL values for metadata and capabilities caching. These values
    /// are used as fallbacks when cache operations are called without an explicit TTL override.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    /// * `config` - A [`CacheConfig`] struct with:
    ///   - `metadata_ttl_seconds` - primary TTL for anchor metadata entries
    ///   - `capabilities_ttl_seconds` - primary TTL for capabilities/stellar.toml entries
    ///   - `swr_ttl_seconds` - stale-while-revalidate grace period
    ///
    /// # Authorization
    ///
    /// Requires admin authorization.
    ///
    /// # Errors
    ///
    /// Panics with [`ErrorCode::UnauthorizedAttestor`] if the caller is not the admin.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use soroban_sdk::Env;
    /// use anchorkit::{AnchorKitContract, CacheConfig};
    ///
    /// let env = Env::default();
    /// let config = CacheConfig {
    ///     metadata_ttl_seconds: 3600,
    ///     capabilities_ttl_seconds: 21600,
    ///     swr_ttl_seconds: 300,
    /// };
    /// AnchorKitContract::set_cache_config(env, config);
    /// ```
    pub fn set_cache_config(env: Env, config: CacheConfig) {
        Self::require_admin(&env);
        env.storage().instance().set(&Self::cache_config_key(&env), &config);
        env.storage().instance().extend_ttl(INSTANCE_TTL, INSTANCE_TTL);
    }

    /// Get the current global cache configuration.
    ///
    /// Returns the active cache TTL settings, or sensible production defaults if no
    /// configuration has been set.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    ///
    /// # Returns
    ///
    /// A [`CacheConfig`] struct with the current TTL settings.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use soroban_sdk::Env;
    /// use anchorkit::AnchorKitContract;
    ///
    /// let env = Env::default();
    /// let config = AnchorKitContract::get_cache_config(env);
    /// println!("Metadata TTL: {} seconds", config.metadata_ttl_seconds);
    /// ```
    pub fn get_cache_config(env: Env) -> CacheConfig {
        env.storage()
            .instance()
            .get::<_, CacheConfig>(&Self::cache_config_key(&env))
            .unwrap_or_else(CacheConfig::default_config)
    }

    /// Resolve the effective TTL: use `override_ttl` when non-zero, otherwise
    /// fall back to `configured`.
    fn effective_ttl(override_ttl: u64, configured: u64) -> u64 {
        if override_ttl != 0 { override_ttl } else { configured }
    }

    // -----------------------------------------------------------------------
    // Capacity configuration and counters
    // -----------------------------------------------------------------------

    fn capacity_config_key(env: &Env) -> soroban_sdk::Vec<soroban_sdk::Symbol> {
        soroban_sdk::vec![env, symbol_short!("CAPCFG")]
    }

    fn attestor_count_key(env: &Env) -> soroban_sdk::Vec<soroban_sdk::Symbol> {
        soroban_sdk::vec![env, symbol_short!("ATCNT")]
    }

    fn cache_count_key(env: &Env) -> soroban_sdk::Vec<soroban_sdk::Symbol> {
        soroban_sdk::vec![env, symbol_short!("CACNT")]
    }

    /// Set the global capacity configuration.
    ///
    /// Configures maximum limits for registered attestors and cache entries.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    /// * `config` - A [`CapacityConfig`] struct with:
    ///   - `max_attestors` - maximum number of registered attestors
    ///   - `max_cache_entries` - maximum number of cache entries
    ///
    /// # Authorization
    ///
    /// Requires admin authorization.
    pub fn set_capacity_config(env: Env, config: CapacityConfig) {
        Self::require_admin(&env);
        env.storage().instance().set(&Self::capacity_config_key(&env), &config);
        env.storage().instance().extend_ttl(INSTANCE_TTL, INSTANCE_TTL);
    }

    /// Get the current global capacity configuration.
    ///
    /// Returns the active capacity limits, or sensible production defaults if no
    /// configuration has been set.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    ///
    /// # Returns
    ///
    /// A [`CapacityConfig`] struct with the current capacity limits.
    pub fn get_capacity_config(env: Env) -> CapacityConfig {
        env.storage()
            .instance()
            .get::<_, CapacityConfig>(&Self::capacity_config_key(&env))
            .unwrap_or_else(CapacityConfig::default_config)
    }

    /// Get the current number of registered attestors.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    ///
    /// # Returns
    ///
    /// The current number of registered attestors.
    pub fn get_attestor_count(env: Env) -> u64 {
        env.storage()
            .instance()
            .get::<_, u64>(&Self::attestor_count_key(&env))
            .unwrap_or(0)
    }

    /// Get the current number of cache entries.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    ///
    /// # Returns
    ///
    /// The current number of cache entries.
    pub fn get_cache_count(env: Env) -> u64 {
        env.storage()
            .instance()
            .get::<_, u64>(&Self::cache_count_key(&env))
            .unwrap_or(0)
    }

    fn refresh_diagnostic_key(
        env: &Env,
        anchor: &Address,
        operation: &String,
    ) -> soroban_sdk::Vec<soroban_sdk::Val> {
        soroban_sdk::vec![
            env,
            symbol_short!("REFDIAG").into_val(env),
            anchor.clone().into_val(env),
            operation.clone().into_val(env),
        ]
    }

    fn record_refresh_diagnostic(
        env: &Env,
        anchor: &Address,
        operation: String,
        status: RefreshStatus,
        had_cached_entry: bool,
        detail: String,
    ) {
        let diagnostic = RefreshDiagnostic {
            operation: operation.clone(),
            status,
            attempted_at: env.ledger().timestamp(),
            had_cached_entry,
            detail,
        };
        let key = Self::refresh_diagnostic_key(env, anchor, &operation);
        env.storage().temporary().set(&key, &diagnostic);
        env.storage().temporary().extend_ttl(&key, MIN_TEMP_TTL, MIN_TEMP_TTL);
    }

    pub fn get_refresh_diagnostic(
        env: Env,
        anchor: Address,
        operation: String,
    ) -> RefreshDiagnostic {
        let key = Self::refresh_diagnostic_key(&env, &anchor, &operation);
        env.storage()
            .temporary()
            .get(&key)
            .unwrap_or_else(|| panic_with_error!(&env, ErrorCode::CacheNotFound))
    }

    // -----------------------------------------------------------------------
    // Request ID generation
    // -----------------------------------------------------------------------

    /// Generate a deterministic request ID: sha256(timestamp_u64_be || sequence_number_u32_be)[:16]
    pub fn generate_request_id(env: Env) -> RequestId {
        let ts = env.ledger().timestamp();
        let seq = env.ledger().sequence() as u32;

        // Build input: 8-byte timestamp || 4-byte sequence number (big-endian)
        let mut input = Bytes::new(&env);
        for b in ts.to_be_bytes().iter() {
            input.push_back(*b);
        }
        for b in seq.to_be_bytes().iter() {
            input.push_back(*b);
        }

        let hash = env.crypto().sha256(&input);
        let mut id = Bytes::new(&env);
        let hash_bytes = hash.to_array();
        for b in hash_bytes.iter().take(16) {
            id.push_back(*b);
        }

        RequestId { id, created_at: ts }
    }

    /// Generate a deterministic child request ID from a root request's bytes and a nonce.
    ///
    /// ID = sha256(root_bytes || nonce_u64_be || ledger_timestamp_u64_be)[:16]
    ///
    /// This ensures child IDs are:
    /// - deterministic given the same inputs
    /// - unique across different nonces / timestamps
    /// - cryptographically bound to the root request
    pub fn generate_child_request_id(env: Env, root_bytes: Bytes, nonce: u64) -> RequestId {
        let ts = env.ledger().timestamp();
        let mut input = root_bytes;
        for b in nonce.to_be_bytes().iter() {
            input.push_back(*b);
        }
        for b in ts.to_be_bytes().iter() {
            input.push_back(*b);
        }
        let hash = env.crypto().sha256(&input);
        let mut id = Bytes::new(&env);
        let hash_bytes = hash.to_array();
        for b in hash_bytes.iter().take(16) {
            id.push_back(*b);
        }
        RequestId { id, created_at: ts }
    }

    // -----------------------------------------------------------------------
    // Attestor management
    // -----------------------------------------------------------------------

    /// Stores the 32-byte Ed25519 public key used to verify SEP-10 JWTs for `issuer`
    /// (the anchor identity whose signing key appears in stellar.toml / SEP-10 flow).
    pub fn set_sep10_jwt_verifying_key(env: Env, issuer: Address, public_key: Bytes) {
        Self::require_admin(&env);
        if public_key.len() != 32 {
            panic_with_error!(&env, ErrorCode::ValidationError);
        }
        let xdr = issuer.clone().to_xdr(&env);
        let raw = xdr_to_vec(&xdr);
        let storage_key = make_storage_key(&env, &[b"SEP10KEY", &raw]);
        env.storage().persistent().set(&storage_key, &public_key);
        env.storage()
            .persistent()
            .extend_ttl(&storage_key, PERSISTENT_TTL, PERSISTENT_TTL);
    }

    /// Rotate the SEP-10 issuer key for `issuer` to `new_public_key`.
    ///
    /// Requires admin authorization. The old key is replaced atomically; any
    /// subsequent `verify_sep10_token` call will use the new key. The previous
    /// key is stored under `"SEP10OLD"` for one TTL period to allow in-flight
    /// tokens signed with the old key to drain gracefully.
    pub fn rotate_sep10_key(env: Env, issuer: Address, new_public_key: Bytes) {
        Self::require_admin(&env);
        if new_public_key.len() != 32 {
            panic_with_error!(&env, ErrorCode::ValidationError);
        }
        let storage_key = (symbol_short!("SEP10KEY"), issuer.clone());
        // Preserve old key for graceful drain
        if let Some(old_key) = env.storage().persistent().get::<_, Bytes>(&storage_key) {
            let old_key_storage = (symbol_short!("SEP10OLD"), issuer.clone());
            env.storage().persistent().set(&old_key_storage, &old_key);
            env.storage()
                .persistent()
                .extend_ttl(&old_key_storage, PERSISTENT_TTL, PERSISTENT_TTL);
        }
        env.storage().persistent().set(&storage_key, &new_public_key);
        env.storage()
            .persistent()
            .extend_ttl(&storage_key, PERSISTENT_TTL, PERSISTENT_TTL);
        env.events().publish(
            (symbol_short!("sep10key"), symbol_short!("rotated"), issuer),
            (),
        );
    }

    /// Return the current SEP-10 verifying key for `issuer`, or `None` if not set.
    pub fn get_sep10_key(env: Env, issuer: Address) -> Option<Bytes> {
        env.storage()
            .persistent()
            .get(&(symbol_short!("SEP10KEY"), issuer))
    }

    /// Configure the maximum JWT length accepted by `verify_sep10_jwt` (issue #64).
    /// Must be between 2048 and 16384. Admin-only.
    pub fn set_jwt_max_len(env: Env, max_len: u32) {
        Self::require_admin(&env);
        if max_len < sep10_jwt::MAX_JWT_LEN || max_len > 16384 {
            panic_with_error!(&env, ErrorCode::ValidationError);
        }
        env.storage()
            .instance()
            .set(&symbol_short!("JWTMAXLEN"), &max_len);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL, INSTANCE_TTL);
    }

    /// Return the currently configured JWT max length (defaults to 2048).
    pub fn get_jwt_max_len(env: Env) -> u32 {
        env.storage()
            .instance()
            .get::<_, u32>(&symbol_short!("JWTMAXLEN"))
            .unwrap_or(sep10_jwt::MAX_JWT_LEN)
    }

    /// Configure the clock skew tolerance (seconds) used by `verify_sep10_jwt`. Admin-only.
    /// Falls back to 60 s when not set. Maximum allowed value is 300 s.
    pub fn set_jwt_skew(env: Env, skew_seconds: u64) {
        Self::require_admin(&env);
        if skew_seconds > 300 {
            panic_with_error!(&env, ErrorCode::ValidationError);
        }
        env.storage()
            .instance()
            .set(&symbol_short!("JWTSKEW"), &skew_seconds);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL, INSTANCE_TTL);
    }

    /// Return the currently configured JWT clock skew tolerance in seconds (defaults to 60).
    pub fn get_jwt_skew(env: Env) -> u64 {
        env.storage()
            .instance()
            .get::<_, u64>(&symbol_short!("JWTSKEW"))
            .unwrap_or(sep10_jwt::DEFAULT_CLOCK_SKEW)
    }

    /// Verifies a SEP-10 JWT (JWS compact, EdDSA) using the stored key for `issuer`: signature, `exp`, and `sub`.
    pub fn verify_sep10_token(env: Env, token: String, issuer: Address) {
        let xdr = issuer.clone().to_xdr(&env);
        let raw = xdr_to_vec(&xdr);
        let pk: Bytes = env
            .storage()
            .persistent()
            .get(&make_storage_key(&env, &[b"SEP10KEY", &raw]))
            .unwrap_or_else(|| panic_with_error!(&env, ErrorCode::InvalidSep10Token));
        if sep10_jwt::verify_sep10_jwt(&env, &token, &pk, None).is_err() {
            panic_with_error!(&env, ErrorCode::InvalidSep10Token);
        }
    }

    fn verify_sep10_token_matches_attestor(
        env: &Env,
        token: &String,
        issuer: &Address,
        attestor: &Address,
    ) {
        let xdr = issuer.clone().to_xdr(env);
        let raw = xdr_to_vec(&xdr);
        let pk: Bytes = env
            .storage()
            .persistent()
            .get(&make_storage_key(env, &[b"SEP10KEY", &raw]))
            .unwrap_or_else(|| panic_with_error!(env, ErrorCode::InvalidSep10Token));
        let expected = attestor.to_string();
        if sep10_jwt::verify_sep10_jwt(env, token, &pk, Some(&expected)).is_err() {
            panic_with_error!(env, ErrorCode::InvalidSep10Token);
        }
    }

    pub fn verify_sep10_token_for_subject(
        env: Env,
        token: String,
        issuer: Address,
        subject: Address,
    ) {
        let xdr = issuer.clone().to_xdr(&env);
        let raw = xdr_to_vec(&xdr);
        let pk: Bytes = env
            .storage()
            .persistent()
            .get(&make_storage_key(&env, &[b"SEP10KEY", &raw]))
            .unwrap_or_else(|| panic_with_error!(&env, ErrorCode::InvalidSep10Token));
        let expected = subject.to_string();
        if sep10_jwt::verify_sep10_jwt(&env, &token, &pk, Some(&expected)).is_err() {
            panic_with_error!(&env, ErrorCode::InvalidSep10Token);
        }
    }

    /// Register a new attestor with SEP-10 verification.
    ///
    /// Adds an attestor to the registry after verifying a SEP-10 JWT token.
    /// The attestor's Ed25519 public key is stored for signature verification.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    /// * `attestor` - Address of the attestor to register.
    /// * `sep10_token` - SEP-10 JWT token for verification.
    /// * `sep10_issuer` - Issuer address for SEP-10 token validation.
    /// * `public_key` - Ed25519 public key for attestation signature verification.
    ///
    /// # Authorization
    ///
    /// Requires admin authorization.
    ///
    /// # Errors
    ///
    /// Panics with:
    /// - [`ErrorCode::AttestorAlreadyRegistered`] if attestor already registered
    /// - [`ErrorCode::InvalidSep10Token`] if token is invalid or expired
    /// - [`ErrorCode::UnauthorizedAttestor`] if caller not authorized
    ///
    /// # Side effects
    ///
    /// - Stores attestor in registry
    /// - Stores public key for signature verification
    /// - Emits attestor.added event
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use soroban_sdk::{Address, BytesN, Env, String};
    /// use anchorkit::AnchorKitContract;
    ///
    /// let env = Env::default();
    /// let attestor = Address::random(&env);
    /// let issuer = Address::random(&env);
    /// let token = String::from_str(&env, "eyJ...");
    /// let pubkey = BytesN::from_array(&env, &[0u8; 32]);
    /// AnchorKitContract::register_attestor(env, attestor, token, issuer, pubkey);
    /// ```
    pub fn register_attestor(env: Env, attestor: Address, sep10_token: String, sep10_issuer: Address, public_key: BytesN<32>) {
        Self::require_admin(&env);
        Self::verify_sep10_token_matches_attestor(&env, &sep10_token, &sep10_issuer, &attestor);
        
        // Check capacity
        let config = Self::get_capacity_config(env.clone());
        let current_count = Self::get_attestor_count(env.clone());
        if current_count >= config.max_attestors {
            panic_with_error!(&env, ErrorCode::AttestorCapacityExceeded);
        }
        
        let xdr = attestor.clone().to_xdr(&env);
        let raw = xdr_to_vec(&xdr);
        let key = make_storage_key(&env, &[b"ATTESTOR", &raw]);
        if env.storage().persistent().has(&key) {
            panic_with_error!(&env, ErrorCode::AttestorAlreadyRegistered);
        }
        env.storage().persistent().set(&key, &true);
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_TTL, PERSISTENT_TTL);
        let pk_key = make_storage_key(&env, &[b"ATPUBKEY", &raw]);
        env.storage().persistent().set(&pk_key, &public_key);
        env.storage()
            .persistent()
            .extend_ttl(&pk_key, PERSISTENT_TTL, PERSISTENT_TTL);
        
        // Increment count
        env.storage().instance().set(&Self::attestor_count_key(&env), &(current_count + 1));
        env.storage().instance().extend_ttl(INSTANCE_TTL, INSTANCE_TTL);
        
        env.events().publish(
            (symbol_short!("attestor"), symbol_short!("added"), attestor),
            (),
        );
    }

    /// Revoke an attestor's registration.
    ///
    /// Removes an attestor from the registry, preventing them from issuing new attestations.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    /// * `attestor` - Address of attestor to revoke.
    ///
    /// # Authorization
    ///
    /// Requires admin authorization.
    ///
    /// # Errors
    ///
    /// Panics with:
    /// - [`ErrorCode::AttestorNotRegistered`] if attestor not registered
    /// - [`ErrorCode::UnauthorizedAttestor`] if caller not authorized
    ///
    /// # Side effects
    ///
    /// - Removes attestor from registry
    /// - Removes stored public key
    /// - Emits attestor.removed event
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use soroban_sdk::{Address, Env};
    /// use anchorkit::AnchorKitContract;
    ///
    /// let env = Env::default();
    /// let attestor = Address::random(&env);
    /// AnchorKitContract::revoke_attestor(env, attestor);
    /// ```
    pub fn revoke_attestor(env: Env, attestor: Address) {
        Self::require_admin(&env);
        let xdr = attestor.clone().to_xdr(&env);
        let raw = xdr_to_vec(&xdr);
        let key = make_storage_key(&env, &[b"ATTESTOR", &raw]);
        if !env.storage().persistent().has(&key) {
            panic_with_error!(&env, ErrorCode::AttestorNotRegistered);
        }
        env.storage().persistent().remove(&key);
        let pk_key = make_storage_key(&env, &[b"ATPUBKEY", &raw]);
        env.storage().persistent().remove(&pk_key);
        
        // Decrement count
        let current_count = Self::get_attestor_count(env.clone());
        if current_count > 0 {
            env.storage().instance().set(&Self::attestor_count_key(&env), &(current_count - 1));
            env.storage().instance().extend_ttl(INSTANCE_TTL, INSTANCE_TTL);
        }
        
        env.events().publish(
            (symbol_short!("attestor"), symbol_short!("removed"), attestor),
            (),
        );
    }

    /// Check if an address is a registered attestor.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    /// * `attestor` - Address to check.
    ///
    /// # Returns
    ///
    /// `true` if the address is registered as an attestor, `false` otherwise.
    ///
    /// # Errors
    ///
    /// None
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use soroban_sdk::{Address, Env};
    /// use anchorkit::AnchorKitContract;
    ///
    /// let env = Env::default();
    /// let attestor = Address::random(&env);
    /// let is_registered = AnchorKitContract::is_attestor(env, attestor);
    /// ```
    pub fn is_attestor(env: Env, attestor: Address) -> bool {
        let xdr = attestor.clone().to_xdr(&env);
        let raw = xdr_to_vec(&xdr);
        env.storage()
            .persistent()
            .get::<_, bool>(&make_storage_key(&env, &[b"ATTESTOR", &raw]))
            .unwrap_or(false)
    }

    // -----------------------------------------------------------------------
    // Attestor profile helpers
    // -----------------------------------------------------------------------

    fn profile_key(attestor: &Address) -> (Symbol, Address) {
        (symbol_short!("PROFILE"), attestor.clone())
    }

    fn load_or_init_profile(env: &Env, attestor: &Address) -> AttestorProfile {
        let key = Self::profile_key(attestor);
        env.storage()
            .persistent()
            .get::<_, AttestorProfile>(&key)
            .unwrap_or(AttestorProfile {
                attestor: attestor.clone(),
                endpoint: String::from_str(env, ""),
                webhook_url: String::from_str(env, ""),
                services: Vec::new(env),
                enabled: true,
                updated_at: 0,
            })
    }

    fn save_profile(env: &Env, profile: &AttestorProfile) {
        let key = Self::profile_key(&profile.attestor);
        env.storage().persistent().set(&key, profile);
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_TTL, PERSISTENT_TTL);
    }

    /// Get the complete profile for an attestor.
    ///
    /// Returns all profile information including endpoint, webhook URL, supported services,
    /// and enabled status.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    /// * `attestor` - Address of the attestor.
    ///
    /// # Returns
    ///
    /// [`AttestorProfile`] with endpoint, webhook_url, services, enabled, updated_at
    ///
    /// # Errors
    ///
    /// Panics with [`ErrorCode::AttestorNotRegistered`] if attestor not registered.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use soroban_sdk::{Address, Env};
    /// use anchorkit::AnchorKitContract;
    ///
    /// let env = Env::default();
    /// let attestor = Address::random(&env);
    /// let profile = AnchorKitContract::get_attestor_profile(env, attestor);
    /// println!("Endpoint: {}", profile.endpoint);
    /// ```
    pub fn get_attestor_profile(env: Env, attestor: Address) -> AttestorProfile {
        if !Self::is_attestor(env.clone(), attestor.clone()) {
            panic_with_error!(&env, ErrorCode::AttestorNotRegistered);
        }
        Self::load_or_init_profile(&env, &attestor)
    }

    // -----------------------------------------------------------------------
    // Attestor endpoint management
    // -----------------------------------------------------------------------

    /// Set the HTTPS endpoint URL for an attestor.
    ///
    /// Updates the attestor's endpoint URL used for external API calls.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    /// * `attestor` - Address of the attestor.
    /// * `endpoint` - HTTPS URL for the attestor's API.
    ///
    /// # Authorization
    ///
    /// Requires the attestor to authorize this call.
    ///
    /// # Errors
    ///
    /// Panics with:
    /// - [`ErrorCode::AttestorNotRegistered`] if attestor not registered
    /// - [`ErrorCode::InvalidEndpointFormat`] if endpoint URL format invalid
    ///
    /// # Side effects
    ///
    /// - Updates attestor profile
    /// - Records update timestamp
    /// - Emits endpoint.updated event
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use soroban_sdk::{Address, Env, String};
    /// use anchorkit::AnchorKitContract;
    ///
    /// let env = Env::default();
    /// let attestor = Address::random(&env);
    /// let endpoint = String::from_str(&env, "https://api.example.com");
    /// AnchorKitContract::set_endpoint(env, attestor, endpoint);
    /// ```
    pub fn set_endpoint(env: Env, attestor: Address, endpoint: String) {
        attestor.require_auth();
        Self::check_attestor(&env, &attestor);
        let endpoint_str = Self::soroban_string_to_rust_string(&env, &endpoint);
        crate::validate_anchor_domain(&endpoint_str)
            .unwrap_or_else(|_| panic_with_error!(&env, ErrorCode::InvalidEndpointFormat));
        let now = env.ledger().timestamp();
        let mut profile = Self::load_or_init_profile(&env, &attestor);
        profile.endpoint = endpoint.clone();
        profile.updated_at = now;
        Self::save_profile(&env, &profile);
        env.events().publish(
            (symbol_short!("endpoint"), symbol_short!("updated")),
            EndpointUpdated { attestor, endpoint },
        );
    }

    /// Get the HTTPS endpoint URL for an attestor.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    /// * `attestor` - Address of the attestor.
    ///
    /// # Returns
    ///
    /// The endpoint URL string (empty if not set).
    ///
    /// # Errors
    ///
    /// Panics with [`ErrorCode::AttestorNotRegistered`] if attestor not registered.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use soroban_sdk::{Address, Env};
    /// use anchorkit::AnchorKitContract;
    ///
    /// let env = Env::default();
    /// let attestor = Address::random(&env);
    /// let endpoint = AnchorKitContract::get_endpoint(env, attestor);
    /// ```
    pub fn get_endpoint(env: Env, attestor: Address) -> String {
        if !Self::is_attestor(env.clone(), attestor.clone()) {
            panic_with_error!(&env, ErrorCode::AttestorNotRegistered);
        }
        let profile = Self::load_or_init_profile(&env, &attestor);
        if profile.endpoint.len() == 0 {
            panic_with_error!(&env, ErrorCode::AttestorNotRegistered);
        }
        profile.endpoint
    }

    /// Register a webhook URL for an attestor.
    ///
    /// Sets the URL where webhook events will be delivered for this attestor.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    /// * `attestor` - Address of the attestor.
    /// * `webhook_url` - URL where webhooks will be delivered.
    ///
    /// # Authorization
    ///
    /// Requires the attestor to authorize this call.
    ///
    /// # Errors
    ///
    /// Panics with [`ErrorCode::AttestorNotRegistered`] if attestor not registered.
    ///
    /// # Side effects
    ///
    /// - Updates attestor profile with webhook URL
    /// - Records update timestamp
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use soroban_sdk::{Address, Env, String};
    /// use anchorkit::AnchorKitContract;
    ///
    /// let env = Env::default();
    /// let attestor = Address::random(&env);
    /// let webhook = String::from_str(&env, "https://api.example.com/webhooks");
    /// AnchorKitContract::register_webhook(env, attestor, webhook);
    /// ```
    pub fn register_webhook(env: Env, attestor: Address, webhook_url: String) {
        attestor.require_auth();
        Self::check_attestor(&env, &attestor);
        let webhook_url_str = Self::soroban_string_to_rust_string(&env, &webhook_url);
        crate::validate_anchor_domain(&webhook_url_str)
            .unwrap_or_else(|_| panic_with_error!(&env, ErrorCode::InvalidEndpointFormat));
        let now = env.ledger().timestamp();
        let mut profile = Self::load_or_init_profile(&env, &attestor);
        profile.webhook_url = webhook_url.clone();
        profile.updated_at = now;
        Self::save_profile(&env, &profile);
        env.events().publish(
            (symbol_short!("webhook"), symbol_short!("reg")),
            EndpointUpdated {
                attestor,
                endpoint: webhook_url,
            },
        );
    }

    /// Get the webhook URL for an attestor.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    /// * `attestor` - Address of the attestor.
    ///
    /// # Returns
    ///
    /// The webhook URL string (empty if not set).
    ///
    /// # Errors
    ///
    /// Panics with [`ErrorCode::AttestorNotRegistered`] if attestor not registered.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use soroban_sdk::{Address, Env};
    /// use anchorkit::AnchorKitContract;
    ///
    /// let env = Env::default();
    /// let attestor = Address::random(&env);
    /// let webhook = AnchorKitContract::get_webhook_url(env, attestor);
    /// ```
    pub fn get_webhook_url(env: Env, attestor: Address) -> String {
        if !Self::is_attestor(env.clone(), attestor.clone()) {
            panic_with_error!(&env, ErrorCode::AttestorNotRegistered);
        }
        let profile = Self::load_or_init_profile(&env, &attestor);
        if profile.webhook_url.len() == 0 {
            panic_with_error!(&env, ErrorCode::AttestorNotRegistered);
        }
        profile.webhook_url
    }

    // -----------------------------------------------------------------------
    // Service configuration
    // -----------------------------------------------------------------------

    /// Configure an anchor's supported services using the contract's current
    /// capability version ([`SERVICE_CAPABILITY_VERSION`]). Equivalent to
    /// [`configure_services_versioned`](Self::configure_services_versioned) with
    /// `version = SERVICE_CAPABILITY_VERSION`.
    /// Configure which services an anchor supports.
    ///
    /// Registers the service types (deposits, withdrawals, quotes, KYC) that an anchor
    /// can provide. Uses the current schema version.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    /// * `anchor` - Address of the anchor.
    /// * `services` - Vector of service type codes:
    ///   - 1 = Deposits
    ///   - 2 = Withdrawals
    ///   - 3 = Quotes
    ///   - 4 = KYC
    ///
    /// # Errors
    ///
    /// Panics with [`ErrorCode::InvalidServiceType`] if any service code is not recognized.
    ///
    /// # Side effects
    ///
    /// - Stores service configuration with current schema version
    /// - Overwrites previous configuration
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use soroban_sdk::{Address, Env, Vec};
    /// use anchorkit::AnchorKitContract;
    ///
    /// let env = Env::default();
    /// let anchor = Address::random(&env);
    /// let services = Vec::from_array(&env, [1u32, 3u32]); // deposits + quotes
    /// AnchorKitContract::configure_services(env, anchor, services);
    /// ```
    pub fn configure_services(env: Env, anchor: Address, services: Vec<u32>) {
        let retirements = Vec::new(&env);
        Self::configure_services_versioned(env, anchor, services, retirements, SERVICE_CAPABILITY_VERSION);
    }

    /// Configure an anchor's supported services and retirement metadata (simple version).
    pub fn configure_services_with_retire(env: Env, anchor: Address, services: Vec<u32>, service_retirements: Vec<ServiceRetirementInfo>) {
        Self::configure_services_versioned(env, anchor, services, service_retirements, SERVICE_CAPABILITY_VERSION);
    }

    /// Configure an anchor's supported services under an explicit capability
    /// version (#239).
    ///
    /// Rejects (panics) when:
    /// - the anchor is not a registered attestor (`AttestorNotRegistered`)
    /// - `version` is `0` or newer than [`SERVICE_CAPABILITY_VERSION`]
    ///   (`UnsupportedCapabilityVersion`) — the contract refuses capability sets
    ///   it cannot interpret
    /// - the service list is empty, contains duplicates, or contains a code the
    ///   current version does not recognise (`InvalidServiceType`)
    ///
    /// Services are stored in deterministic sorted order (ascending) regardless
    /// of submission order, ensuring consistent storage and event emission (#258).
    ///
    /// On success the record is stored stamped with `version` so capability
    /// discovery is explicit. Re-configuring overwrites the previous record,
    /// which is how an anchor migrates to a newer version.
    /// Configure services with explicit schema version.
    ///
    /// Registers service types with a specific schema version for forward compatibility.
    /// Rejects versions newer than the current contract version.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    /// * `anchor` - Address of the anchor.
    /// * `services` - Vector of service type codes.
    /// * `version` - Schema version for this configuration.
    ///
    /// # Errors
    ///
    /// Panics with:
    /// - [`ErrorCode::InvalidServiceType`] if any service code not recognized
    /// - [`ErrorCode::UnsupportedCapabilityVersion`] if version newer than current
    ///
    /// # Side effects
    ///
    /// - Stores versioned service configuration
    /// - Overwrites previous configuration
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use soroban_sdk::{Address, Env, Vec};
    /// use anchorkit::AnchorKitContract;
    ///
    /// let env = Env::default();
    /// let anchor = Address::random(&env);
    /// let services = Vec::from_array(&env, [1u32, 2u32]); // deposits + withdrawals
    /// AnchorKitContract::configure_services_versioned(env, anchor, services, 1);
    /// ```
    pub fn configure_services_versioned(
        env: Env,
        anchor: Address,
        services: Vec<u32>,
        service_retirements: Vec<ServiceRetirementInfo>,
        version: u32,
    ) {
        anchor.require_auth();
        let xdr = anchor.clone().to_xdr(&env);
        let raw = xdr_to_vec(&xdr);
        if !env.storage().persistent().has(&make_storage_key(&env, &[b"ATTESTOR", &raw])) {
            panic_with_error!(&env, ErrorCode::AttestorNotRegistered);
        }
        if version == 0 || version > SERVICE_CAPABILITY_VERSION {
            panic_with_error!(&env, ErrorCode::UnsupportedCapabilityVersion);
        }
        if services.is_empty() {
            panic_with_error!(&env, ErrorCode::InvalidServiceType);
        }
        
        // Validate and normalize services: check for duplicates, validate codes,
        // and sort deterministically for consistent storage and event emission.
        let mut seen = Vec::new(&env);
        let mut normalized = Vec::new(&env);
        
        for s in services.iter() {
            if seen.contains(&s) {
                panic_with_error!(&env, ErrorCode::InvalidServiceType);
            }
            if !Self::is_known_service_code(s) {
                panic_with_error!(&env, ErrorCode::InvalidServiceType);
            }
            seen.push_back(s);
            normalized.push_back(s);
        }
        
        // Validate service retirements
        let mut seen_retirements = Vec::new(&env);
        for retirement in service_retirements.iter() {
            if seen_retirements.contains(&retirement.service_code) {
                panic_with_error!(&env, ErrorCode::InvalidServiceType);
            }
            if !Self::is_known_service_code(retirement.service_code) {
                panic_with_error!(&env, ErrorCode::InvalidServiceType);
            }
            seen_retirements.push_back(retirement.service_code);
        }
        
        // Sort services deterministically (ascending order) for consistent storage
        // and predictable behavior regardless of submission order.
        Self::sort_services(&env, &mut normalized);
        
        let record = AnchorServices {
            anchor: anchor.clone(),
            services: normalized.clone(),
            service_capability_version: version,
            service_retirements,
        };
        let key = make_storage_key(&env, &[b"SERVICES", &raw]);
        env.storage().persistent().set(&key, &record);
        env.storage().persistent().extend_ttl(&key, PERSISTENT_TTL, PERSISTENT_TTL);

        // Also sync services into the unified AttestorProfile.
        let mut profile = Self::load_or_init_profile(&env, &anchor);
        profile.services = normalized;
        profile.updated_at = env.ledger().timestamp();
        Self::save_profile(&env, &profile);

        env.events().publish((symbol_short!("services"), symbol_short!("config")), record);
    }

    /// The service-capability schema version this contract understands.
    /// Off-chain capability discovery can read this to learn which service
    /// codes the contract will accept.
    /// Get the current service capability schema version.
    ///
    /// Returns the version constant that the contract recognizes for service configurations.
    ///
    /// # Arguments
    ///
    /// * `_env` - The Soroban environment context.
    ///
    /// # Returns
    ///
    /// Current capability version (currently 1).
    ///
    /// # Errors
    ///
    /// None
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use soroban_sdk::Env;
    /// use anchorkit::AnchorKitContract;
    ///
    /// let env = Env::default();
    /// let version = AnchorKitContract::current_capability_version(env);
    /// assert_eq!(version, 1);
    /// ```
    pub fn current_capability_version(_env: Env) -> u32 {
        SERVICE_CAPABILITY_VERSION
    }

    /// Return the capability version an anchor's stored service set was
    /// configured under. Panics with `ServicesNotConfigured` if absent.
    /// Get the schema version of an anchor's service configuration.
    ///
    /// Returns the version under which the anchor's service configuration was stored.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    /// * `anchor` - Address of the anchor.
    ///
    /// # Returns
    ///
    /// Schema version of the anchor's service configuration (0 if not configured).
    ///
    /// # Errors
    ///
    /// None
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use soroban_sdk::{Address, Env};
    /// use anchorkit::AnchorKitContract;
    ///
    /// let env = Env::default();
    /// let anchor = Address::random(&env);
    /// let version = AnchorKitContract::get_service_capability_version(env, anchor);
    /// ```
    pub fn get_service_capability_version(env: Env, anchor: Address) -> u32 {
        env.storage()
            .persistent()
            .get::<_, AnchorServices>(&(symbol_short!("SERVICES"), anchor))
            .map(|r| r.service_capability_version)
            .unwrap_or_else(|| panic_with_error!(&env, ErrorCode::ServicesNotConfigured))
    }

    /// Get all services supported by an anchor.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    /// * `anchor` - Address of the anchor.
    ///
    /// # Returns
    ///
    /// [`AnchorServices`] with service codes and schema version.
    ///
    /// # Errors
    ///
    /// None
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use soroban_sdk::{Address, Env};
    /// use anchorkit::AnchorKitContract;
    ///
    /// let env = Env::default();
    /// let anchor = Address::random(&env);
    /// let services = AnchorKitContract::get_supported_services(env, anchor);
    /// ```
    pub fn get_supported_services(env: Env, anchor: Address) -> AnchorServices {
        let xdr = anchor.clone().to_xdr(&env);
        let raw = xdr_to_vec(&xdr);
        env.storage()
            .persistent()
            .get::<_, AnchorServices>(&make_storage_key(&env, &[b"SERVICES", &raw]))
            .unwrap_or_else(|| panic_with_error!(&env, ErrorCode::ServicesNotConfigured))
    }

    /// Get active (non-retired) services for an anchor.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    /// * `anchor` - Address of the anchor.
    ///
    /// # Returns
    ///
    /// Vector of active service type codes.
    pub fn get_active_services(env: Env, anchor: Address) -> Vec<u32> {
        let record = Self::get_supported_services(env.clone(), anchor);
        let mut active = Vec::new(&env);
        for service in record.services.iter() {
            if !Self::is_service_retired(&record, service) {
                active.push_back(service);
            }
        }
        active
    }

    /// Helper to check if a service is retired in an AnchorServices record.
    fn is_service_retired(record: &AnchorServices, service: u32) -> bool {
        for retirement in record.service_retirements.iter() {
            if retirement.service_code == service && retirement.retired {
                return true;
            }
        }
        false
    }

    /// Check if an anchor supports a specific service and it is not retired.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    /// * `anchor` - Address of the anchor.
    /// * `service` - Service type code to check (1=deposits, 2=withdrawals, 3=quotes, 4=kyc).
    ///
    /// # Returns
    ///
    /// `true` if the anchor supports the service and it is not retired, `false` otherwise.
    ///
    /// # Errors
    ///
    /// None
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use soroban_sdk::{Address, Env};
    /// use anchorkit::AnchorKitContract;
    ///
    /// let env = Env::default();
    /// let anchor = Address::random(&env);
    /// let supports_deposits = AnchorKitContract::supports_service(env, anchor, 1);
    /// ```
    pub fn supports_service(env: Env, anchor: Address, service: u32) -> bool {
        let xdr = anchor.clone().to_xdr(&env);
        let raw = xdr_to_vec(&xdr);
        let record = env
            .storage()
            .persistent()
            .get::<_, AnchorServices>(&make_storage_key(&env, &[b"SERVICES", &raw]))
            .unwrap_or_else(|| panic_with_error!(&env, ErrorCode::ServicesNotConfigured));
        record.services.contains(&service) && !Self::is_service_retired(&record, service)
    }

    /// Get retirement info for a specific service.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    /// * `anchor` - Address of the anchor.
    /// * `service` - Service type code to check.
    ///
    /// # Returns
    ///
    /// `Option<ServiceRetirementInfo>` with retirement metadata if found.
    pub fn get_service_retirement_info(env: Env, anchor: Address, service: u32) -> Option<ServiceRetirementInfo> {
        let record = Self::get_supported_services(env, anchor);
        for retirement in record.service_retirements.iter() {
            if retirement.service_code == service {
                return Some(retirement);
            }
        }
        None
    }

    /// Retire a specific service for an anchor.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    /// * `anchor` - Address of the anchor.
    /// * `service` - Service type code to retire.
    /// * `retirement_timestamp` - Optional timestamp when retirement takes effect.
    /// * `deprecation_notice` - Optional notice about the retirement.
    pub fn retire_service(env: Env, anchor: Address, service: u32, retirement_timestamp: Option<u64>, deprecation_notice: Option<String>) {
        anchor.require_auth();
        let xdr = anchor.clone().to_xdr(&env);
        let raw = xdr_to_vec(&xdr);
        let key = make_storage_key(&env, &[b"SERVICES", &raw]);
        let mut record = env
            .storage()
            .persistent()
            .get::<_, AnchorServices>(&key)
            .unwrap_or_else(|| panic_with_error!(&env, ErrorCode::ServicesNotConfigured));
        
        // Check if retirement info already exists for this service
        let mut found = false;
        let mut new_retirements = Vec::new(&env);
        for retirement in record.service_retirements.iter() {
            if retirement.service_code == service {
                // Update existing retirement info
                new_retirements.push_back(ServiceRetirementInfo {
                    service_code: service,
                    retired: true,
                    retirement_timestamp: retirement_timestamp.or(retirement.retirement_timestamp),
                    deprecation_notice: deprecation_notice.clone().or(retirement.deprecation_notice),
                });
                found = true;
            } else {
                new_retirements.push_back(retirement);
            }
        }

        if !found {
            // Add new retirement info
            new_retirements.push_back(ServiceRetirementInfo {
                service_code: service,
                retired: true,
                retirement_timestamp,
                deprecation_notice,
            });
        }
        
        record.service_retirements = new_retirements;
        env.storage().persistent().set(&key, &record);
        env.storage().persistent().extend_ttl(&key, PERSISTENT_TTL, PERSISTENT_TTL);
        env.events().publish((symbol_short!("services"), symbol_short!("retire")), (anchor, service));
    }

    /// Unretire a specific service for an anchor.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    /// * `anchor` - Address of the anchor.
    /// * `service` - Service type code to unretire.
    pub fn unretire_service(env: Env, anchor: Address, service: u32) {
        anchor.require_auth();
        let xdr = anchor.clone().to_xdr(&env);
        let raw = xdr_to_vec(&xdr);
        let key = make_storage_key(&env, &[b"SERVICES", &raw]);
        let mut record = env
            .storage()
            .persistent()
            .get::<_, AnchorServices>(&key)
            .unwrap_or_else(|| panic_with_error!(&env, ErrorCode::ServicesNotConfigured));
        
        let mut new_retirements = Vec::new(&env);
        for retirement in record.service_retirements.iter() {
            if retirement.service_code == service {
                // Mark as not retired but keep the metadata for history
                new_retirements.push_back(ServiceRetirementInfo {
                    service_code: service,
                    retired: false,
                    retirement_timestamp: None,
                    deprecation_notice: retirement.deprecation_notice,
                });
            } else {
                new_retirements.push_back(retirement);
            }
        }
        
        record.service_retirements = new_retirements;
        env.storage().persistent().set(&key, &record);
        env.storage().persistent().extend_ttl(&key, PERSISTENT_TTL, PERSISTENT_TTL);
        env.events().publish((symbol_short!("services"), symbol_short!("unretire")), (anchor, service));
    }

    // -----------------------------------------------------------------------
    // Attestation submission (plain)
    // -----------------------------------------------------------------------

    pub fn submit_attestation(
        env: Env,
        issuer: Address,
        subject: Address,
        timestamp: u64,
        payload_hash: Bytes,
        signature: Bytes,
    ) -> u64 {
        issuer.require_auth();
        Self::check_attestor(&env, &issuer);
        Self::verify_attestation_signature(&env, &issuer, &payload_hash, &signature);
        Self::enforce_rate_limit(&env, &issuer);
        Self::check_timestamp(&env, timestamp);

        let config = RateLimiter::get_config(&env);
        if RateLimiter::check_and_increment(&env, &issuer, &config).is_err() {
            panic_with_error!(&env, ErrorCode::RateLimitExceeded);
        }

        let issuer_xdr = issuer.clone().to_xdr(&env);
        let issuer_raw = xdr_to_vec(&issuer_xdr);
        let hash_raw = xdr_to_vec(&payload_hash);
        let used_key = make_storage_key(&env, &[b"USED", &issuer_raw, &hash_raw]);
        if env.storage().persistent().has(&used_key) {
            // Record replay detection event and metrics before panicking
            let replay_event = replay_detection::record_replay_detection(&env, &payload_hash, &issuer);
            replay_detection::emit_replay_detection_log(&env, &replay_event);
            panic_with_error!(&env, ErrorCode::ReplayAttack);
        }

        let id = Self::next_attestation_id(&env);
        Self::store_attestation(
            &env, id, issuer.clone(), subject.clone(), timestamp,
            payload_hash.clone(), signature,
        );

        env.storage().persistent().set(&used_key, &timestamp);
        env.storage().persistent().extend_ttl(&used_key, REPLAY_TTL, REPLAY_TTL);

        env.events().publish(
            (symbol_short!("attest"), symbol_short!("recorded"), id, subject),
            AttestEvent { payload_hash, timestamp },
        );
        id
    }

    // -----------------------------------------------------------------------
    // Attestation submission with KYC enforcement
    // -----------------------------------------------------------------------

    pub fn submit_attestation_kyc_check(
        env: Env,
        issuer: Address,
        subject: Address,
        timestamp: u64,
        payload_hash: Bytes,
        signature: Bytes,
        require_kyc: bool,
    ) -> u64 {
        issuer.require_auth();
        Self::check_attestor(&env, &issuer);
        Self::verify_attestation_signature(&env, &issuer, &payload_hash, &signature);
        Self::check_timestamp(&env, timestamp);

        if require_kyc {
            let kyc_status = Self::get_kyc_status(env.clone(), subject.clone());
            if kyc_status != KycStatus::Approved {
                match kyc_status {
                    KycStatus::Pending    => panic_with_error!(&env, ErrorCode::KycPending),
                    KycStatus::Rejected   => panic_with_error!(&env, ErrorCode::KycRejected),
                    KycStatus::Expired    => panic_with_error!(&env, ErrorCode::ComplianceNotMet),
                    KycStatus::NotSubmitted => panic_with_error!(&env, ErrorCode::KycNotFound),
                    _ => panic_with_error!(&env, ErrorCode::ComplianceNotMet),
                }
            }
        }

        let issuer_xdr = issuer.clone().to_xdr(&env);
        let issuer_raw = xdr_to_vec(&issuer_xdr);
        let hash_raw = xdr_to_vec(&payload_hash);
        let used_key = make_storage_key(&env, &[b"USED", &issuer_raw, &hash_raw]);
        if env.storage().persistent().has(&used_key) {
            // Record replay detection event and metrics before panicking
            let replay_event = replay_detection::record_replay_detection(&env, &payload_hash, &issuer);
            replay_detection::emit_replay_detection_log(&env, &replay_event);
            panic_with_error!(&env, ErrorCode::ReplayAttack);
        }

        let id = Self::next_attestation_id(&env);
        Self::store_attestation(
            &env, id, issuer.clone(), subject.clone(), timestamp,
            payload_hash.clone(), signature,
        );

        env.storage().persistent().set(&used_key, &timestamp);
        env.storage().persistent().extend_ttl(&used_key, REPLAY_TTL, REPLAY_TTL);

        let _now = env.ledger().timestamp();
        env.events().publish(
            (symbol_short!("attest"), symbol_short!("recorded"), id, subject),
            AttestEvent { payload_hash: payload_hash.clone(), timestamp },
        );
        env.events().publish(
            (symbol_short!("webhook"), symbol_short!("event")),
            WebhookEvent {
                event_type: String::from_str(&env, "attestation_submitted"),
                transaction_id: id, timestamp, payload_hash,
            },
        );
        id
    }

    // -----------------------------------------------------------------------
    // Attestation submission with request ID + tracing span
    // -----------------------------------------------------------------------

    pub fn submit_with_request_id(
        env: Env,
        request_id: RequestId,
        issuer: Address,
        subject: Address,
        timestamp: u64,
        payload_hash: Bytes,
        signature: Bytes,
    ) -> u64 {
        issuer.require_auth();
        Self::check_attestor(&env, &issuer);
        Self::verify_attestation_signature(&env, &issuer, &payload_hash, &signature);
        Self::enforce_rate_limit(&env, &issuer);
        Self::check_timestamp(&env, timestamp);

        let issuer_xdr = issuer.clone().to_xdr(&env);
        let issuer_raw = xdr_to_vec(&issuer_xdr);
        let hash_raw = xdr_to_vec(&payload_hash);
        let used_key = make_storage_key(&env, &[b"USED", &issuer_raw, &hash_raw]);
        if env.storage().persistent().has(&used_key) {
            // Record replay detection event and metrics before panicking
            let replay_event = replay_detection::record_replay_detection(&env, &payload_hash, &issuer);
            replay_detection::emit_replay_detection_log(&env, &replay_event);
            panic_with_error!(&env, ErrorCode::ReplayAttack);
        }

        let id = Self::next_attestation_id(&env);
        Self::store_attestation(
            &env, id, issuer.clone(), subject.clone(), timestamp,
            payload_hash.clone(), signature,
        );

        env.storage().persistent().set(&used_key, &timestamp);
        env.storage().persistent().extend_ttl(&used_key, REPLAY_TTL, REPLAY_TTL);

        let now = env.ledger().timestamp();
        Self::store_span(
            &env, &request_id,
            String::from_str(&env, "submit_attestation"),
            issuer.clone(), now,
            String::from_str(&env, "success"),
        );

        // Propagate operation name into RequestContext
        Self::record_operation_in_context(&env, &request_id.id, String::from_str(&env, "submit_attestation"));

        env.events().publish(
            (symbol_short!("attest"), symbol_short!("recorded"), id, subject),
            AttestEvent { payload_hash: payload_hash.clone(), timestamp },
        );
        env.events().publish(
            (symbol_short!("webhook"), symbol_short!("event")),
            WebhookEvent {
                event_type: String::from_str(&env, "attestation_submitted"),
                transaction_id: id, timestamp, payload_hash,
            },
        );
        id
    }

    // -----------------------------------------------------------------------
    // Quote submission with request ID + tracing span
    // -----------------------------------------------------------------------

    #[allow(unused_variables)]
    pub fn quote_with_request_id(
        env: Env,
        request_id: RequestId,
        anchor: Address,
        from_asset: String,
        to_asset: String,
        amount: u64,
        fee_bps: u32,
        min_amount: u64,
        max_amount: u64,
        expires_at: u64,
    ) {
        anchor.require_auth();
        let xdr = anchor.clone().to_xdr(&env);
        let raw = xdr_to_vec(&xdr);
        let services_record = env
            .storage()
            .persistent()
            .get::<_, AnchorServices>(&make_storage_key(&env, &[b"SERVICES", &raw]))
            .unwrap_or_else(|| panic_with_error!(&env, ErrorCode::ServicesNotConfigured));
        if !services_record.services.contains(&SERVICE_QUOTES) {
            panic_with_error!(&env, ErrorCode::ServicesNotConfigured);
        }
        let now = env.ledger().timestamp();
        Self::store_span(
            &env, &request_id,
            String::from_str(&env, "submit_quote"),
            anchor, now,
            String::from_str(&env, "success"),
        );

        // Propagate operation name into RequestContext
        Self::record_operation_in_context(&env, &request_id.id, String::from_str(&env, "submit_quote"));
    }

    /// Record a tracing span for a quote submission, including optional routing
    /// reason metadata in the span operation name (#298).
    ///
    /// Behaves exactly like [`quote_with_request_id`] but when `routing_reason`
    /// is `Some`, the span operation is annotated as
    /// `"submit_quote_with_reason"` and the reason is recorded in the
    /// [`RequestContext`] operation chain so downstream audit consumers can
    /// correlate the reason with the request.
    ///
    /// # Arguments
    ///
    /// * `routing_reason` – Optional routing reason to attach to the span.
    ///   When `None` the behaviour is identical to [`quote_with_request_id`].
    #[allow(unused_variables)]
    pub fn quote_with_request_id_and_reason(
        env: Env,
        request_id: RequestId,
        anchor: Address,
        from_asset: String,
        to_asset: String,
        amount: u64,
        fee_bps: u32,
        min_amount: u64,
        max_amount: u64,
        expires_at: u64,
        routing_reason: Option<String>,
    ) {
        anchor.require_auth();
        let xdr = anchor.clone().to_xdr(&env);
        let raw = xdr_to_vec(&xdr);
        let services_record = env
            .storage()
            .persistent()
            .get::<_, AnchorServices>(&make_storage_key(&env, &[b"SERVICES", &raw]))
            .unwrap_or_else(|| panic_with_error!(&env, ErrorCode::ServicesNotConfigured));
        if !services_record.services.contains(&SERVICE_QUOTES) {
            panic_with_error!(&env, ErrorCode::ServicesNotConfigured);
        }
        let now = env.ledger().timestamp();

        // Choose the operation label based on whether a reason was supplied so
        // the span is self-describing in audit queries.
        let operation = if routing_reason.is_some() {
            String::from_str(&env, "submit_quote_with_reason")
        } else {
            String::from_str(&env, "submit_quote")
        };

        Self::store_span(
            &env, &request_id,
            operation.clone(),
            anchor, now,
            String::from_str(&env, "success"),
        );

        Self::record_operation_in_context(&env, &request_id.id, operation);
    }

    // -----------------------------------------------------------------------
    // Tracing span retrieval
    // -----------------------------------------------------------------------

    pub fn get_tracing_span(env: Env, request_id_bytes: Bytes) -> Option<TracingSpan> {
        env.storage()
            .temporary()
            .get::<_, TracingSpan>(&(symbol_short!("SPAN"), request_id_bytes))
    }

    /// Create a child span under a parent span, setting parent_request_id and
    /// incrementing the span_index from the TracingContext stored for the root.
    ///
    /// The TracingContext for the root must have been initialised by a prior
    /// `submit_with_request_id` call (which stores span_index = 0).
    ///
    /// Panics with `ValidationError` if:
    /// - the parent span does not exist (no root context found)
    /// - `child_request_id` bytes are identical to `parent_request_id` bytes
    /// - the child span index would not increment correctly
    pub fn propagate_span(
        env: Env,
        parent_request_id: RequestId,
        child_request_id: RequestId,
        operation: String,
        actor: Address,
    ) {
        actor.require_auth();

        // Validate: child must differ from parent
        if child_request_id.id == parent_request_id.id {
            panic_with_error!(&env, ErrorCode::ValidationError);
        }

        // Validate: a root context must exist for the parent (i.e. the parent
        // span was created by submit_with_request_id or a prior propagate_span).
        let ctx_key = (symbol_short!("TRACECTX"), parent_request_id.id.clone());
        let mut ctx: TracingContext = env
            .storage()
            .temporary()
            .get(&ctx_key)
            .unwrap_or_else(|| panic_with_error!(&env, ErrorCode::ValidationError));

        let span_index = ctx.next_span_index;
        ctx.next_span_index += 1;

        let now = env.ledger().timestamp();
        env.storage().temporary().set(&ctx_key, &ctx);
        env.storage().temporary().extend_ttl(&ctx_key, SPAN_TTL, SPAN_TTL);

        // Register child span ID under the root so get_trace can find it
        let child_list_key = (symbol_short!("TRACEIDS"), parent_request_id.id.clone(), span_index);
        env.storage().temporary().set(&child_list_key, &child_request_id.id.clone());
        env.storage().temporary().extend_ttl(&child_list_key, SPAN_TTL, SPAN_TTL);

        Self::store_span_with_parent(
            &env,
            &child_request_id,
            operation,
            actor,
            now,
            String::from_str(&env, "success"),
            parent_request_id.id.clone(),
            span_index,
        );
    }

    /// Retrieve all spans associated with a root request ID, ordered by span_index.
    /// Returns the root span first, followed by child spans in creation order.
    pub fn get_trace(env: Env, root_request_id_bytes: Bytes) -> Vec<TracingSpan> {
        let mut spans = Vec::new(&env);

        // Root span (span_index = 0)
        if let Some(root_span) = env
            .storage()
            .temporary()
            .get::<_, TracingSpan>(&(symbol_short!("SPAN"), root_request_id_bytes.clone()))
        {
            spans.push_back(root_span);
        }

        // Child spans registered via propagate_span
        let ctx_key = (symbol_short!("TRACECTX"), root_request_id_bytes.clone());
        let ctx: Option<TracingContext> = env.storage().temporary().get(&ctx_key);
        if let Some(ctx) = ctx {
            for i in 1..ctx.next_span_index {
                let child_list_key = (symbol_short!("TRACEIDS"), root_request_id_bytes.clone(), i);
                if let Some(child_id) = env
                    .storage()
                    .temporary()
                    .get::<_, Bytes>(&child_list_key)
                {
                    if let Some(child_span) = env
                        .storage()
                        .temporary()
                        .get::<_, TracingSpan>(&(symbol_short!("SPAN"), child_id))
                    {
                        spans.push_back(child_span);
                    }
                }
            }
        }

        spans
    }

    // -----------------------------------------------------------------------
    // RequestContext — propagation and querying
    // -----------------------------------------------------------------------

    /// Create a new `RequestContext` for a root request ID.
    ///
    /// Panics with `ValidationError` if `root_request_id.id` is empty.
    pub fn create_request_context(env: Env, root_request_id: RequestId) -> RequestContext {
        if root_request_id.id.is_empty() {
            panic_with_error!(&env, ErrorCode::ValidationError);
        }
        Self::require_valid_timestamp(&env, root_request_id.created_at);
        let now = env.ledger().timestamp();
        let ctx = RequestContext {
            root_request_id: root_request_id.clone(),
            operation_chain: Vec::new(&env),
            created_at: now,
        };
        let key = (symbol_short!("REQCTX"), root_request_id.id.clone());
        env.storage().temporary().set(&key, &ctx);
        env.storage()
            .temporary()
            .extend_ttl(&key, SPAN_TTL, SPAN_TTL);
        ctx
    }

    /// Append `operation_name` to the `operation_chain` of the context identified
    /// by `root_request_id_bytes`. Creates the context if it does not yet exist.
    ///
    /// Panics with `ValidationError` if `operation_name` is empty.
    pub fn append_operation(
        env: Env,
        root_request_id_bytes: Bytes,
        operation_name: String,
    ) {
        Self::require_non_empty_string(&env, &operation_name);
        let key = (symbol_short!("REQCTX"), root_request_id_bytes.clone());
        let mut ctx: RequestContext = env
            .storage()
            .temporary()
            .get(&key)
            .unwrap_or_else(|| {
                // Auto-create a minimal context if none exists yet
                let now = env.ledger().timestamp();
                RequestContext {
                    root_request_id: RequestId {
                        id: root_request_id_bytes.clone(),
                        created_at: now,
                    },
                    operation_chain: Vec::new(&env),
                    created_at: now,
                }
            });
        ctx.operation_chain.push_back(operation_name);
        env.storage().temporary().set(&key, &ctx);
        env.storage()
            .temporary()
            .extend_ttl(&key, SPAN_TTL, SPAN_TTL);
    }

    /// Return the full `RequestContext` (including `operation_chain`) for a
    /// given root request ID, or `None` if no context has been stored.
    pub fn get_request_context(env: Env, root_request_id_bytes: Bytes) -> Option<RequestContext> {
        env.storage()
            .temporary()
            .get::<_, RequestContext>(&(symbol_short!("REQCTX"), root_request_id_bytes))
    }

    // -----------------------------------------------------------------------
    // Attestation retrieval
    // -----------------------------------------------------------------------

    pub fn get_attestation(env: Env, id: u64) -> Attestation {
        env.storage()
            .persistent()
            .get::<_, Attestation>(&make_storage_key(&env, &[b"ATTEST", &id.to_be_bytes()]))
            .unwrap_or_else(|| panic_with_error!(&env, ErrorCode::AttestationNotFound))
    }

    pub fn get_attestation_by_hash(env: Env, issuer: Address, payload_hash: Bytes) -> u64 {
        let issuer_xdr = issuer.clone().to_xdr(&env);
        let issuer_raw = xdr_to_vec(&issuer_xdr);
        let hash_raw = xdr_to_vec(&payload_hash);
        let used_key = make_storage_key(&env, &[b"USED", &issuer_raw, &hash_raw]);
        env.storage()
            .persistent()
            .get::<_, u64>(&used_key)
            .unwrap_or_else(|| panic_with_error!(&env, ErrorCode::AttestationNotFound))
    }

    // -----------------------------------------------------------------------
    // Deterministic hash utilities (#192)
    // -----------------------------------------------------------------------

    /// Compute a canonical SHA-256 hash for an attestation payload.
    /// Field order: subject || timestamp (8-byte BE) || data.
    pub fn compute_payload_hash(
        env: Env,
        subject: Address,
        timestamp: u64,
        data: Bytes,
    ) -> BytesN<32> {
        compute_payload_hash(&env, &subject, timestamp, &data)
    }

    /// Verify that the hash stored in an attestation matches the expected hash.
    pub fn verify_payload_hash(env: Env, attestation_id: u64, expected_hash: BytesN<32>) -> bool {
        let attestation = env
            .storage()
            .persistent()
            .get::<_, Attestation>(&make_storage_key(&env, &[b"ATTEST", &attestation_id.to_be_bytes()]))
            .unwrap_or_else(|| panic_with_error!(&env, ErrorCode::AttestationNotFound));

        let stored: BytesN<32> = attestation.payload_hash.try_into()
            .unwrap_or_else(|_| panic_with_error!(&env, ErrorCode::ValidationError));
        verify_payload_hash(&stored, &expected_hash)
    }

    // -----------------------------------------------------------------------
    // Session management
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // KYC data management
    // -----------------------------------------------------------------------

    pub fn submit_kyc(env: Env, subject: Address, data_hash: Bytes, attestor: Address) {
        attestor.require_auth();
        Self::check_attestor(&env, &attestor);
        let now = env.ledger().timestamp();
        let key = kyc_record_key(&env, &subject);
        if env.storage().persistent().has(&key) {
            let existing: KycRecord = env.storage().persistent().get(&key)
                .unwrap_or_else(|| panic_with_error!(&env, ErrorCode::ComplianceNotMet));
            let current_status = current_kyc_status(&env, &existing);
            if !validate_kyc_transition(current_status, KycStatus::Pending, &existing, now) {
                panic_with_error!(&env, ErrorCode::ComplianceNotMet);
            }
        }
        let record = KycRecord {
            subject: subject.clone(), status: KycStatus::Pending as u32,
            submitted_at: now, reviewed_at: None, expiry: None,
            rejection_reason_hash: None,
            schema_version: SCHEMA_V1,
        };
        env.storage().persistent().set(&key, &record);
        env.storage().persistent().extend_ttl(&key, PERSISTENT_TTL, PERSISTENT_TTL);
        let data_key = make_storage_key(&env, &[b"KYCDATA", &xdr_to_vec(&subject.clone().to_xdr(&env))]);
        env.storage().persistent().set(&data_key, &data_hash);
        env.storage().persistent().extend_ttl(&data_key, PERSISTENT_TTL, PERSISTENT_TTL);
        env.events().publish(
            (symbol_short!("kyc"), symbol_short!("submitted"), subject),
            WebhookEvent {
                event_type: String::from_str(&env, "kyc_submitted"),
                transaction_id: 0, timestamp: now, payload_hash: data_hash,
            },
        );
    }

    /// Approve a pending KYC record.
    ///
    /// `operator` must be the primary admin or hold [`AdminRole::KycAdmin`].
    pub fn approve_kyc(env: Env, operator: Address, subject: Address) {
        Self::require_admin_or_role(&env, &operator, AdminRole::KycAdmin);
        let now = env.ledger().timestamp();
        let key = kyc_record_key(&env, &subject);
        let mut record: KycRecord = env.storage().persistent().get(&key)
            .unwrap_or_else(|| panic_with_error!(&env, ErrorCode::KycNotFound));
        let current_status = current_kyc_status(&env, &record);
        if !validate_kyc_transition(current_status, KycStatus::Approved, &record, now) {
            panic_with_error!(&env, ErrorCode::IllegalTransition);
        }
        record.status = KycStatus::Approved as u32;
        record.reviewed_at = Some(now);
        record.expiry = Some(now + KYC_EXPIRY_SECONDS);
        env.storage().persistent().set(&key, &record);
        env.storage().persistent().extend_ttl(&key, PERSISTENT_TTL, PERSISTENT_TTL);
        env.events().publish(
            (symbol_short!("kyc"), symbol_short!("approved"), subject),
            WebhookEvent {
                event_type: String::from_str(&env, "kyc_approved"),
                transaction_id: 0, timestamp: now, payload_hash: Bytes::new(&env),
            },
        );
    }

    /// Reject a pending KYC record.
    ///
    /// `operator` must be the primary admin or hold [`AdminRole::KycAdmin`].
    pub fn reject_kyc(env: Env, operator: Address, subject: Address, reason_hash: Bytes) {
        Self::require_admin_or_role(&env, &operator, AdminRole::KycAdmin);
        let now = env.ledger().timestamp();
        let key = kyc_record_key(&env, &subject);
        let mut record: KycRecord = env.storage().persistent().get(&key)
            .unwrap_or_else(|| panic_with_error!(&env, ErrorCode::KycNotFound));
        let current_status = current_kyc_status(&env, &record);
        if !validate_kyc_transition(current_status, KycStatus::Rejected, &record, now) {
            panic_with_error!(&env, ErrorCode::IllegalTransition);
        }
        record.status = KycStatus::Rejected as u32;
        record.reviewed_at = Some(now);
        record.expiry = None;
        record.rejection_reason_hash = Some(reason_hash.clone());
        env.storage().persistent().set(&key, &record);
        env.storage().persistent().extend_ttl(&key, PERSISTENT_TTL, PERSISTENT_TTL);
        env.events().publish(
            (symbol_short!("kyc"), symbol_short!("rejected"), subject),
            WebhookEvent {
                event_type: String::from_str(&env, "kyc_rejected"),
                transaction_id: 0, timestamp: now, payload_hash: reason_hash,
            },
        );
    }

    pub fn get_kyc_status(env: Env, subject: Address) -> KycStatus {
        let key = kyc_record_key(&env, &subject);
        if !env.storage().persistent().has(&key) {
            return KycStatus::NotSubmitted;
        }
        let record: KycRecord = env.storage().persistent().get(&key)
            .unwrap_or_else(|| panic_with_error!(&env, ErrorCode::KycNotFound));
        if let Some(expiry) = record.expiry {
            if env.ledger().timestamp() > expiry {
                return KycStatus::Expired;
            }
        }
        match record.status {
            0 => KycStatus::NotSubmitted,
            1 => KycStatus::Pending,
            2 => KycStatus::Approved,
            3 => KycStatus::Rejected,
            4 => KycStatus::Expired,
            _ => KycStatus::NotSubmitted,
        }
    }

    // -----------------------------------------------------------------------
    // Compliance check recording (#37)
    // -----------------------------------------------------------------------

    /// Record a compliance check result for a subject (admin-only).
    /// Stores the latest `ComplianceCheck` record, appends to history, and updates the
    /// per-subject check-type index so auditors can query decision histories.
    pub fn record_compliance_check(
        env: Env,
        subject: Address,
        check_type: String,
        passed: bool,
        score: Option<u32>,
    ) {
        Self::require_admin(&env);
        let now = env.ledger().timestamp();
        let record = ComplianceCheck {
            subject: subject.clone(),
            check_type: check_type.clone(),
            result: if passed { 1u32 } else { 0u32 },
            score,
            timestamp: now,
        };

        // Store latest (keyed by subject + check_type)
        let key = compliance_check_key(&env, &subject, &check_type);
        env.storage().persistent().set(&key, &record);
        env.storage().persistent().extend_ttl(&key, PERSISTENT_TTL, PERSISTENT_TTL);

        // Append to ordered history
        let hist_cnt_key = compliance_history_count_key(&env, &subject, &check_type);
        let idx: u64 = env
            .storage()
            .persistent()
            .get::<_, u64>(&hist_cnt_key)
            .unwrap_or(0u64);
        let hist_key = compliance_history_entry_key(&env, &subject, &check_type, idx);
        env.storage().persistent().set(&hist_key, &record);
        env.storage().persistent().extend_ttl(&hist_key, PERSISTENT_TTL, PERSISTENT_TTL);
        env.storage().persistent().set(&hist_cnt_key, &(idx + 1));
        env.storage().persistent().extend_ttl(&hist_cnt_key, PERSISTENT_TTL, PERSISTENT_TTL);

        // Update per-subject check-type index
        let idx_key = compliance_subject_index_key(&env, &subject);
        let mut check_types: Vec<String> = env
            .storage()
            .persistent()
            .get::<_, Vec<String>>(&idx_key)
            .unwrap_or_else(|| Vec::new(&env));
        if !check_types.contains(&check_type) {
            check_types.push_back(check_type.clone());
            env.storage().persistent().set(&idx_key, &check_types);
            env.storage().persistent().extend_ttl(&idx_key, PERSISTENT_TTL, PERSISTENT_TTL);
        }

        env.events().publish(
            (symbol_short!("comp"), symbol_short!("checked"), subject),
            record,
        );
    }

    /// Return the most recent compliance check record for `(subject, check_type)`, or
    /// `None` if no check has been recorded.
    pub fn get_latest_compliance_check(
        env: Env,
        subject: Address,
        check_type: String,
    ) -> Option<ComplianceCheck> {
        let key = compliance_check_key(&env, &subject, &check_type);
        env.storage().persistent().get(&key)
    }

    /// Return the ordered history of compliance checks for `(subject, check_type)`.
    /// Returns up to `limit` records (capped at 50), most-recent last.
    pub fn get_compliance_check_history(
        env: Env,
        subject: Address,
        check_type: String,
        limit: u64,
    ) -> Vec<ComplianceCheck> {
        let hist_cnt_key = compliance_history_count_key(&env, &subject, &check_type);
        let total: u64 = env
            .storage()
            .persistent()
            .get::<_, u64>(&hist_cnt_key)
            .unwrap_or(0u64);
        let effective_limit = limit.min(50);
        let start = if total > effective_limit { total - effective_limit } else { 0 };
        let mut results = Vec::new(&env);
        for i in start..total {
            let hist_key = compliance_history_entry_key(&env, &subject, &check_type, i);
            if let Some(entry) = env.storage().persistent().get::<_, ComplianceCheck>(&hist_key) {
                results.push_back(entry);
            }
        }
        results
    }

    /// Return all check types that have been recorded for a given subject.
    pub fn list_subject_compliance_checks(env: Env, subject: Address) -> Vec<String> {
        let idx_key = compliance_subject_index_key(&env, &subject);
        env.storage()
            .persistent()
            .get::<_, Vec<String>>(&idx_key)
            .unwrap_or_else(|| Vec::new(&env))
    }

    // -----------------------------------------------------------------------
    // Input validation helpers (#243)
    // -----------------------------------------------------------------------

    /// Panic with `ValidationError` if `s` is empty.
    fn require_non_empty_string(env: &Env, s: &String) {
        if s.len() == 0 {
            panic_with_error!(env, ErrorCode::ValidationError);
        }
    }

    /// Panic with `InvalidTimestamp` if `ts` is zero.
    fn require_valid_timestamp(env: &Env, ts: u64) {
        if ts == 0 {
            panic_with_error!(env, ErrorCode::InvalidTimestamp);
        }
    }

    pub fn create_session(env: Env, initiator: Address) -> u64 {
        initiator.require_auth();
        let inst = env.storage().instance();
        let scnt_key = make_storage_key(&env, &[b"SCNT"]);
        let session_id: u64 = inst.get(&scnt_key).unwrap_or(0u64);
        inst.set(&scnt_key, &(session_id + 1));
        inst.extend_ttl(INSTANCE_TTL, INSTANCE_TTL);

        let now = env.ledger().timestamp();
        let session = Session {
            session_id,
            initiator: initiator.clone(),
            created_at: now,
            nonce: 0,
            operation_count: 0,
            session_ttl_seconds: DEFAULT_SESSION_TTL,
            closed: false,
        };
        let sess_key = make_storage_key(&env, &[b"SESS", &session_id.to_be_bytes()]);
        env.storage().persistent().set(&sess_key, &session);
        env.storage().persistent().extend_ttl(&sess_key, PERSISTENT_TTL, PERSISTENT_TTL);

        let snonce_key = make_storage_key(&env, &[b"SNONCE", &session_id.to_be_bytes()]);
        env.storage().persistent().set(&snonce_key, &0u64);
        env.storage().persistent().extend_ttl(&snonce_key, PERSISTENT_TTL, PERSISTENT_TTL);

        env.events().publish(
            (symbol_short!("session"), symbol_short!("created"), session_id),
            SessionCreatedEvent { session_id, initiator, timestamp: now },
        );
        session_id
    }

    pub fn close_session(env: Env, session_id: u64, initiator: Address) {
        initiator.require_auth();
        let sess_key = make_storage_key(&env, &[b"SESS", &session_id.to_be_bytes()]);
        let mut session: Session = env
            .storage()
            .persistent()
            .get(&sess_key)
            .unwrap_or_else(|| panic_with_error!(&env, ErrorCode::AttestationNotFound));
        Self::validate_session(&env, &session);
        session.closed = true;
        env.storage().persistent().set(&sess_key, &session);
        let now = env.ledger().timestamp();
        env.events().publish(
            (symbol_short!("session"), symbol_short!("closed"), session_id),
            SessionClosedEvent { session_id, initiator, timestamp: now },
        );
    }

    fn require_session_open(env: &Env, session_id: u64) {
        let sess_key = make_storage_key(env, &[b"SESS", &session_id.to_be_bytes()]);
        let session: Session = env
            .storage()
            .persistent()
            .get(&sess_key)
            .unwrap_or_else(|| panic_with_error!(env, ErrorCode::AttestationNotFound));
        Self::validate_session(env, &session);
        // #232: enforce per-session operation limit
        let op_count: u64 = env
            .storage()
            .persistent()
            .get(&make_storage_key(env, &[b"SOPCNT", &session_id.to_be_bytes()]))
            .unwrap_or(0u64);
        if op_count >= MAX_OPS_PER_SESSION {
            panic_with_error!(env, ErrorCode::SessionOperationLimitExceeded);
        }
    }

    // -----------------------------------------------------------------------
    // Quote management
    // -----------------------------------------------------------------------

    pub fn submit_quote(
        env: Env,
        anchor: Address,
        base_asset: String,
        quote_asset: String,
        rate: u64,
        fee_percentage: u32,
        minimum_amount: u64,
        maximum_amount: u64,
        valid_until: u64,
    ) -> u64 {
        anchor.require_auth();
        validate_currency_code(&env, &base_asset);
        validate_currency_code(&env, &quote_asset);
        validate_fee_percent(&env, fee_percentage);
        validate_amount_limits(&env, minimum_amount, maximum_amount);
        let inst = env.storage().instance();
        let qcnt_key = make_storage_key(&env, &[b"QCNT"]);
        let next: u64 = inst.get(&qcnt_key).unwrap_or(0u64) + 1;
        inst.set(&qcnt_key, &next);
        inst.extend_ttl(INSTANCE_TTL, INSTANCE_TTL);

        let anchor_xdr = anchor.clone().to_xdr(&env);
        let anchor_raw = xdr_to_vec(&anchor_xdr);
        let quote = Quote {
            quote_id: next, anchor: anchor.clone(),
            base_asset: base_asset.clone(), quote_asset: quote_asset.clone(),
            rate, fee_percentage, minimum_amount, maximum_amount, valid_until,
            schema_version: SCHEMA_V1,
            routing_reason: None,
        };
        let q_key = make_storage_key(&env, &[b"QUOTE", &anchor_raw, &next.to_be_bytes()]);
        env.storage().persistent().set(&q_key, &quote);
        env.storage().persistent().extend_ttl(&q_key, PERSISTENT_TTL, PERSISTENT_TTL);

        let lq_key = make_storage_key(&env, &[b"LATESTQ", &anchor_raw]);
        env.storage().persistent().set(&lq_key, &next);
        env.storage().persistent().extend_ttl(&lq_key, PERSISTENT_TTL, PERSISTENT_TTL);

        env.events().publish(
            (symbol_short!("quote"), symbol_short!("submit"), next),
            QuoteSubmitEvent { quote_id: next, anchor, base_asset, quote_asset, rate, valid_until, routing_reason: None },
        );
        next
    }

    /// Submit a quote with optional routing reason metadata (#298).
    ///
    /// Identical to [`submit_quote`] but records an optional `routing_reason`
    /// alongside the quote for audit and customer-support purposes. The reason
    /// is persisted in the [`Quote`] record and emitted in the submit event so
    /// it is available for off-chain audit consumers.
    ///
    /// # Arguments
    ///
    /// * `routing_reason` – Human-readable code explaining why this anchor/route
    ///   was chosen (e.g. `"lowest_fee"`, `"referral"`, `"preferred_anchor"`).
    ///   Pass `None` when no reason applies.
    pub fn submit_quote_with_reason(
        env: Env,
        anchor: Address,
        base_asset: String,
        quote_asset: String,
        rate: u64,
        fee_percentage: u32,
        minimum_amount: u64,
        maximum_amount: u64,
        valid_until: u64,
        routing_reason: Option<String>,
    ) -> u64 {
        anchor.require_auth();
        validate_currency_code(&env, &base_asset);
        validate_currency_code(&env, &quote_asset);
        validate_fee_percent(&env, fee_percentage);
        validate_amount_limits(&env, minimum_amount, maximum_amount);
        let inst = env.storage().instance();
        let qcnt_key = make_storage_key(&env, &[b"QCNT"]);
        let next: u64 = inst.get(&qcnt_key).unwrap_or(0u64) + 1;
        inst.set(&qcnt_key, &next);
        inst.extend_ttl(INSTANCE_TTL, INSTANCE_TTL);

        let anchor_xdr = anchor.clone().to_xdr(&env);
        let anchor_raw = xdr_to_vec(&anchor_xdr);
        let quote = Quote {
            quote_id: next, anchor: anchor.clone(),
            base_asset: base_asset.clone(), quote_asset: quote_asset.clone(),
            rate, fee_percentage, minimum_amount, maximum_amount, valid_until,
            schema_version: SCHEMA_V1,
            routing_reason: routing_reason.clone(),
        };
        let q_key = make_storage_key(&env, &[b"QUOTE", &anchor_raw, &next.to_be_bytes()]);
        env.storage().persistent().set(&q_key, &quote);
        env.storage().persistent().extend_ttl(&q_key, PERSISTENT_TTL, PERSISTENT_TTL);

        let lq_key = make_storage_key(&env, &[b"LATESTQ", &anchor_raw]);
        env.storage().persistent().set(&lq_key, &next);
        env.storage().persistent().extend_ttl(&lq_key, PERSISTENT_TTL, PERSISTENT_TTL);

        env.events().publish(
            (symbol_short!("quote"), symbol_short!("submit"), next),
            QuoteSubmitEvent { quote_id: next, anchor, base_asset, quote_asset, rate, valid_until, routing_reason },
        );
        next
    }

    /// Retrieve the routing reason stored with a quote (#298).
    ///
    /// Returns `None` when the quote was submitted without a reason or does not
    /// exist. Callers that need the full quote record should use [`get_quote`].
    pub fn get_quote_routing_reason(env: Env, anchor: Address, quote_id: u64) -> Option<String> {
        let anchor_xdr = anchor.to_xdr(&env);
        let anchor_raw = xdr_to_vec(&anchor_xdr);
        let key = make_storage_key(&env, &[b"QUOTE", &anchor_raw, &quote_id.to_be_bytes()]);
        env.storage()
            .persistent()
            .get::<_, Quote>(&key)
            .and_then(|q| q.routing_reason)
    }

    pub fn receive_quote(env: Env, receiver: Address, anchor: Address, quote_id: u64) -> Quote {
        receiver.require_auth();
        let anchor_xdr = anchor.clone().to_xdr(&env);
        let anchor_raw = xdr_to_vec(&anchor_xdr);
        let q_key = make_storage_key(&env, &[b"QUOTE", &anchor_raw, &quote_id.to_be_bytes()]);
        let quote: Quote = env.storage().persistent().get(&q_key)
            .unwrap_or_else(|| panic_with_error!(&env, ErrorCode::AttestationNotFound));
        env.events().publish(
            (symbol_short!("quote"), symbol_short!("received"), quote_id),
            QuoteReceivedEvent { quote_id, receiver, timestamp: env.ledger().timestamp() },
        );
        quote
    }

    /// Accept a quote with compliance gating (#297).
    ///
    /// Verifies that the subject has passed compliance checks before accepting the quote.
    /// If the subject or corridor requires compliance checks, they must be passed first.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    /// * `receiver` - The address accepting the quote.
    /// * `anchor` - The anchor providing the quote.
    /// * `quote_id` - The quote identifier.
    /// * `require_compliance` - Whether to enforce compliance checks.
    ///
    /// # Returns
    ///
    /// The accepted [`Quote`].
    ///
    /// # Errors
    ///
    /// Panics with [`ErrorCode::ComplianceNotMet`] if compliance is required but not passed.
    pub fn accept_quote_with_compliance(
        env: Env,
        receiver: Address,
        anchor: Address,
        quote_id: u64,
        require_compliance: bool,
    ) -> Quote {
        receiver.require_auth();
        
        // Get the quote
        let anchor_xdr = anchor.clone().to_xdr(&env);
        let anchor_raw = xdr_to_vec(&anchor_xdr);
        let q_key = make_storage_key(&env, &[b"QUOTE", &anchor_raw, &quote_id.to_be_bytes()]);
        let quote: Quote = env.storage().persistent().get(&q_key)
            .unwrap_or_else(|| panic_with_error!(&env, ErrorCode::AttestationNotFound));

        // #297: Enforce compliance gating if required
        if require_compliance {
            let comp_key = compliance_check_key(&env, &receiver, &String::from_str(&env, "kyc"));
            let passed = env.storage().persistent()
                .get::<_, ComplianceCheck>(&comp_key)
                .map(|r| r.result == 1u32)
                .unwrap_or(false);
            if !passed {
                panic_with_error!(&env, ErrorCode::ComplianceNotMet);
            }
        }

        env.events().publish(
            (symbol_short!("quote"), symbol_short!("accepted"), quote_id),
            QuoteReceivedEvent { quote_id, receiver, timestamp: env.ledger().timestamp() },
        );
        quote
    }

    // -----------------------------------------------------------------------
    // Session-aware attestation
    // -----------------------------------------------------------------------

    pub fn submit_attestation_with_session(
        env: Env,
        session_id: u64,
        issuer: Address,
        subject: Address,
        timestamp: u64,
        payload_hash: Bytes,
        signature: Bytes,
    ) -> u64 {
        issuer.require_auth();
        Self::require_session_open(&env, session_id);
        Self::check_attestor(&env, &issuer);
        Self::verify_attestation_signature(&env, &issuer, &payload_hash, &signature);
        Self::enforce_rate_limit(&env, &issuer);
        Self::check_timestamp(&env, timestamp);

        // #232: per-session request-ID replay protection
        let hash_raw = xdr_to_vec(&payload_hash);
        let sess_req_key = make_storage_key(
            &env, &[b"SESSREQ", &session_id.to_be_bytes(), &hash_raw],
        );
        if env.storage().persistent().has(&sess_req_key) {
            // Record replay detection event and metrics before panicking
            let replay_event = replay_detection::record_replay_detection(&env, &payload_hash, &issuer);
            replay_detection::emit_replay_detection_log(&env, &replay_event);
            panic_with_error!(&env, ErrorCode::ReplayAttack);
        }
        env.storage().persistent().set(&sess_req_key, &true);
        env.storage().persistent().extend_ttl(&sess_req_key, REPLAY_TTL, REPLAY_TTL);

        let issuer_xdr = issuer.clone().to_xdr(&env);
        let issuer_raw = xdr_to_vec(&issuer_xdr);
        let used_key = make_storage_key(&env, &[b"USED", &issuer_raw, &hash_raw]);
        if env.storage().persistent().has(&used_key) {
            // Record replay detection event and metrics before panicking
            let replay_event = replay_detection::record_replay_detection(&env, &payload_hash, &issuer);
            replay_detection::emit_replay_detection_log(&env, &replay_event);
            panic_with_error!(&env, ErrorCode::ReplayAttack);
        }

        let id = Self::next_attestation_id(&env);
        Self::store_attestation(
            &env, id, issuer.clone(), subject.clone(), timestamp,
            payload_hash.clone(), signature,
        );

        env.storage().persistent().set(&used_key, &timestamp);
        env.storage().persistent().extend_ttl(&used_key, REPLAY_TTL, REPLAY_TTL);

        // Increment session nonce
        let sess_key = make_storage_key(&env, &[b"SESS", &session_id.to_be_bytes()]);
        let mut session: Session = env
            .storage().persistent().get(&sess_key)
            .unwrap_or_else(|| panic_with_error!(&env, ErrorCode::AttestationNotFound));
        session.nonce += 1;
        env.storage().persistent().set(&sess_key, &session);
        env.storage().persistent().extend_ttl(&sess_key, PERSISTENT_TTL, PERSISTENT_TTL);

        let sopcnt_key = make_storage_key(&env, &[b"SOPCNT", &session_id.to_be_bytes()]);
        let op_index: u64 = env.storage().persistent().get(&sopcnt_key).unwrap_or(0u64);
        env.storage().persistent().set(&sopcnt_key, &(op_index + 1));
        env.storage().persistent().extend_ttl(&sopcnt_key, PERSISTENT_TTL, PERSISTENT_TTL);

        let inst = env.storage().instance();
        let acnt_key = make_storage_key(&env, &[b"ACNT"]);
        let log_id: u64 = inst.get(&acnt_key).unwrap_or(0u64);
        inst.set(&acnt_key, &(log_id + 1));
        inst.extend_ttl(INSTANCE_TTL, INSTANCE_TTL);

        let now = env.ledger().timestamp();
        let audit = AuditLog {
            log_id, session_id, actor: issuer.clone(),
            operation: OperationContext {
                session_id, operation_index: op_index,
                operation_type: String::from_str(&env, "attest"),
                timestamp: now, status: String::from_str(&env, "success"),
                result_data: id,
            },
        };
        let audit_key = make_storage_key(&env, &[b"AUDIT", &log_id.to_be_bytes()]);
        env.storage().persistent().set(&audit_key, &audit);
        env.storage().persistent().extend_ttl(&audit_key, PERSISTENT_TTL, PERSISTENT_TTL);
        let slog_key = make_storage_key(&env, &[b"SLOG", &session_id.to_be_bytes(), &op_index.to_be_bytes()]);
        env.storage().persistent().set(&slog_key, &log_id);
        env.storage().persistent().extend_ttl(&slog_key, PERSISTENT_TTL, PERSISTENT_TTL);

        env.events().publish(
            (symbol_short!("attest"), symbol_short!("recorded"), id, subject),
            AttestEvent { payload_hash, timestamp },
        );
        env.events().publish(
            (symbol_short!("audit"), symbol_short!("logged"), log_id),
            AuditLogEvent {
                log_id, session_id, operation_index: op_index,
                operation_type: String::from_str(&env, "attest"),
                status: String::from_str(&env, "success"),
            },
        );
        id
    }

    /// Register an attestor within a session.
    ///
    /// `operator` must be the primary admin or hold [`AdminRole::AttestorAdmin`].
    pub fn register_attestor_with_session(env: Env, operator: Address, session_id: u64, attestor: Address, public_key: BytesN<32>) {
        Self::require_admin_or_role(&env, &operator, AdminRole::AttestorAdmin);
        Self::require_session_open(&env, session_id);
        let xdr = attestor.clone().to_xdr(&env);
        let raw = xdr_to_vec(&xdr);
        let key = make_storage_key(&env, &[b"ATTESTOR", &raw]);
        if env.storage().persistent().has(&key) {
            panic_with_error!(&env, ErrorCode::AttestorAlreadyRegistered);
        }
        env.storage().persistent().set(&key, &true);
        env.storage().persistent().extend_ttl(&key, PERSISTENT_TTL, PERSISTENT_TTL);
        let pk_key = make_storage_key(&env, &[b"ATPUBKEY", &raw]);
        env.storage().persistent().set(&pk_key, &public_key);
        env.storage().persistent().extend_ttl(&pk_key, PERSISTENT_TTL, PERSISTENT_TTL);

        let sopcnt_key = make_storage_key(&env, &[b"SOPCNT", &session_id.to_be_bytes()]);
        let op_index: u64 = env.storage().persistent().get(&sopcnt_key).unwrap_or(0u64);
        env.storage().persistent().set(&sopcnt_key, &(op_index + 1));
        env.storage().persistent().extend_ttl(&sopcnt_key, PERSISTENT_TTL, PERSISTENT_TTL);

        let inst = env.storage().instance();
        let acnt_key = make_storage_key(&env, &[b"ACNT"]);
        let log_id: u64 = inst.get(&acnt_key).unwrap_or(0u64);
        inst.set(&acnt_key, &(log_id + 1));
        inst.extend_ttl(INSTANCE_TTL, INSTANCE_TTL);

        let now = env.ledger().timestamp();
        let audit = AuditLog {
            log_id, session_id, actor: operator.clone(),
            operation: OperationContext {
                session_id, operation_index: op_index,
                operation_type: String::from_str(&env, "register"),
                timestamp: now, status: String::from_str(&env, "success"),
                result_data: 0,
            },
        };
        let audit_key = make_storage_key(&env, &[b"AUDIT", &log_id.to_be_bytes()]);
        env.storage().persistent().set(&audit_key, &audit);
        env.storage().persistent().extend_ttl(&audit_key, PERSISTENT_TTL, PERSISTENT_TTL);
        let slog_key = make_storage_key(&env, &[b"SLOG", &session_id.to_be_bytes(), &op_index.to_be_bytes()]);
        env.storage().persistent().set(&slog_key, &log_id);
        env.storage().persistent().extend_ttl(&slog_key, PERSISTENT_TTL, PERSISTENT_TTL);

        env.events().publish((symbol_short!("attestor"), symbol_short!("added"), attestor), ());
        env.events().publish(
            (symbol_short!("audit"), symbol_short!("logged"), log_id),
            AuditLogEvent {
                log_id, session_id, operation_index: op_index,
                operation_type: String::from_str(&env, "register"),
                status: String::from_str(&env, "success"),
            },
        );
    }

    /// Revoke an attestor within a session.
    ///
    /// `operator` must be the primary admin or hold [`AdminRole::AttestorAdmin`].
    pub fn revoke_attestor_with_session(env: Env, operator: Address, session_id: u64, attestor: Address) {
        Self::require_admin_or_role(&env, &operator, AdminRole::AttestorAdmin);
        Self::require_session_open(&env, session_id);
        let xdr = attestor.clone().to_xdr(&env);
        let raw = xdr_to_vec(&xdr);
        let key = make_storage_key(&env, &[b"ATTESTOR", &raw]);
        if !env.storage().persistent().has(&key) {
            panic_with_error!(&env, ErrorCode::AttestorNotRegistered);
        }
        env.storage().persistent().remove(&key);
        let pk_key = make_storage_key(&env, &[b"ATPUBKEY", &raw]);
        env.storage().persistent().remove(&pk_key);

        let sopcnt_key = make_storage_key(&env, &[b"SOPCNT", &session_id.to_be_bytes()]);
        let op_index: u64 = env.storage().persistent().get(&sopcnt_key).unwrap_or(0u64);
        env.storage().persistent().set(&sopcnt_key, &(op_index + 1));
        env.storage().persistent().extend_ttl(&sopcnt_key, PERSISTENT_TTL, PERSISTENT_TTL);

        let inst = env.storage().instance();
        let acnt_key = make_storage_key(&env, &[b"ACNT"]);
        let log_id: u64 = inst.get(&acnt_key).unwrap_or(0u64);
        inst.set(&acnt_key, &(log_id + 1));
        inst.extend_ttl(INSTANCE_TTL, INSTANCE_TTL);

        let now = env.ledger().timestamp();
        let audit = AuditLog {
            log_id, session_id, actor: operator.clone(),
            operation: OperationContext {
                session_id, operation_index: op_index,
                operation_type: String::from_str(&env, "revoke"),
                timestamp: now, status: String::from_str(&env, "success"),
                result_data: 0,
            },
        };
        let audit_key = make_storage_key(&env, &[b"AUDIT", &log_id.to_be_bytes()]);
        env.storage().persistent().set(&audit_key, &audit);
        env.storage().persistent().extend_ttl(&audit_key, PERSISTENT_TTL, PERSISTENT_TTL);
        let slog_key = make_storage_key(&env, &[b"SLOG", &session_id.to_be_bytes(), &op_index.to_be_bytes()]);
        env.storage().persistent().set(&slog_key, &log_id);
        env.storage().persistent().extend_ttl(&slog_key, PERSISTENT_TTL, PERSISTENT_TTL);

        env.events().publish((symbol_short!("attestor"), symbol_short!("removed"), attestor), ());
        env.events().publish(
            (symbol_short!("audit"), symbol_short!("logged"), log_id),
            AuditLogEvent {
                log_id, session_id, operation_index: op_index,
                operation_type: String::from_str(&env, "revoke"),
                status: String::from_str(&env, "success"),
            },
        );
    }

    pub fn get_session(env: Env, session_id: u64) -> Session {
        env.storage()
            .persistent()
            .get::<_, Session>(&make_storage_key(&env, &[b"SESS", &session_id.to_be_bytes()]))
            .unwrap_or_else(|| panic_with_error!(&env, ErrorCode::AttestationNotFound))
    }

    pub fn get_audit_log(env: Env, log_id: u64) -> AuditLog {
        env.storage()
            .persistent()
            .get::<_, AuditLog>(&make_storage_key(&env, &[b"AUDIT", &log_id.to_be_bytes()]))
            .unwrap_or_else(|| panic_with_error!(&env, ErrorCode::AttestationNotFound))
    }

    pub fn get_session_audit_logs(env: Env, session_id: u64, limit: u64) -> Vec<AuditLog> {
        let total: u64 = env
            .storage()
            .persistent()
            .get(&make_storage_key(&env, &[b"SOPCNT", &session_id.to_be_bytes()]))
            .unwrap_or(0u64);
        let mut results = Vec::new(&env);
        let start = if total > limit { total - limit } else { 0 };
        for i in start..total {
            let slog_key = make_storage_key(&env, &[b"SLOG", &session_id.to_be_bytes(), &i.to_be_bytes()]);
            if let Some(log_id) = env.storage().persistent().get::<_, u64>(&slog_key) {
                let audit_key = make_storage_key(&env, &[b"AUDIT", &log_id.to_be_bytes()]);
                if let Some(entry) = env.storage().persistent().get::<_, AuditLog>(&audit_key) {
                    results.push_back(entry);
                }
            }
        }
        results
    }

    pub fn get_session_operation_count(env: Env, session_id: u64) -> u64 {
        env.storage()
            .persistent()
            .get::<_, u64>(&make_storage_key(&env, &[b"SOPCNT", &session_id.to_be_bytes()]))
            .unwrap_or(0)
    }

    // -----------------------------------------------------------------------
    // Audit log retention and pagination (#251)
    // -----------------------------------------------------------------------

    /// Set the audit log retention policy in days (admin-only).
    /// A value of 0 means no automatic retention limit is enforced.
    pub fn set_audit_log_retention(env: Env, retention_days: u64) {
        Self::require_admin(&env);
        let key = audit_retention_key(&env);
        env.storage().instance().set(&key, &retention_days);
        env.storage().instance().extend_ttl(INSTANCE_TTL, INSTANCE_TTL);
    }

    /// Return the configured audit log retention policy in days (0 = unlimited).
    pub fn get_audit_log_retention(env: Env) -> u64 {
        let key = audit_retention_key(&env);
        env.storage().instance().get::<_, u64>(&key).unwrap_or(0u64)
    }

    /// Return the total number of audit log entries ever written.
    pub fn get_audit_log_count(env: Env) -> u64 {
        let acnt_key = make_storage_key(&env, &[b"ACNT"]);
        env.storage().instance().get::<_, u64>(&acnt_key).unwrap_or(0u64)
    }

    /// Return a page of audit log entries starting at `offset`, up to `limit` entries
    /// (capped at 50 per call to bound WASM execution).
    pub fn get_audit_logs_paginated(env: Env, offset: u64, limit: u64) -> Vec<AuditLog> {
        let acnt_key = make_storage_key(&env, &[b"ACNT"]);
        let total: u64 = env.storage().instance().get::<_, u64>(&acnt_key).unwrap_or(0u64);
        let effective_limit = limit.min(50);
        let end = (offset + effective_limit).min(total);
        let mut results = Vec::new(&env);
        for i in offset..end {
            let audit_key = make_storage_key(&env, &[b"AUDIT", &i.to_be_bytes()]);
            if let Some(entry) = env.storage().persistent().get::<_, AuditLog>(&audit_key) {
                results.push_back(entry);
            }
        }
        results
    }

    /// Paginated retrieval of audit logs scoped to a specific session.
    /// Returns up to `limit` entries (capped at 50) starting at `offset` within the session.
    pub fn get_session_logs_paginated(
        env: Env,
        session_id: u64,
        offset: u64,
        limit: u64,
    ) -> Vec<AuditLog> {
        let total: u64 = env
            .storage()
            .persistent()
            .get(&make_storage_key(&env, &[b"SOPCNT", &session_id.to_be_bytes()]))
            .unwrap_or(0u64);
        let effective_limit = limit.min(50);
        let end = (offset + effective_limit).min(total);
        let mut results = Vec::new(&env);
        for i in offset..end {
            let slog_key = make_storage_key(&env, &[b"SLOG", &session_id.to_be_bytes(), &i.to_be_bytes()]);
            if let Some(log_id) = env.storage().persistent().get::<_, u64>(&slog_key) {
                let audit_key = make_storage_key(&env, &[b"AUDIT", &log_id.to_be_bytes()]);
                if let Some(entry) = env.storage().persistent().get::<_, AuditLog>(&audit_key) {
                    results.push_back(entry);
                }
            }
        }
        results
    }

    /// Remove audit log entries whose `operation.timestamp` is strictly before
    /// `before_timestamp`. Scans up to the first 100 log IDs to remain WASM-safe.
    /// Returns the number of entries pruned.
    pub fn prune_audit_logs(env: Env, before_timestamp: u64) -> u64 {
        Self::require_admin(&env);
        let acnt_key = make_storage_key(&env, &[b"ACNT"]);
        let total: u64 = env.storage().instance().get::<_, u64>(&acnt_key).unwrap_or(0u64);
        let scan_limit = total.min(100);
        let mut pruned: u64 = 0;
        for i in 0..scan_limit {
            let audit_key = make_storage_key(&env, &[b"AUDIT", &i.to_be_bytes()]);
            if let Some(entry) = env.storage().persistent().get::<_, AuditLog>(&audit_key) {
                if entry.operation.timestamp < before_timestamp {
                    env.storage().persistent().remove(&audit_key);
                    pruned += 1;
                }
            }
        }
        pruned
    }

    // -----------------------------------------------------------------------
    // Metadata cache
    // -----------------------------------------------------------------------

    pub fn cache_metadata(env: Env, anchor: Address, metadata: AnchorMetadata, ttl_seconds: u64) {
        Self::require_admin(&env);
        let key = (symbol_short!("METACACHE"), anchor.clone());
        let entry_exists = env.storage().temporary().has(&key);
        
        // Check capacity only if adding a new entry
        if !entry_exists {
            let config = Self::get_capacity_config(env.clone());
            let current_count = Self::get_cache_count(env.clone());
            if current_count >= config.max_cache_entries {
                panic_with_error!(&env, ErrorCode::CacheCapacityExceeded);
            }
        }
        
        let now = env.ledger().timestamp();
        let cfg = Self::get_cache_config(env.clone());
        let ttl = Self::effective_ttl(ttl_seconds, cfg.metadata_ttl_seconds);
        let entry = MetadataCache {
            metadata,
            cached_at: now,
            ttl_seconds: ttl,
            stale_ttl_seconds: 0,
            needs_refresh: false,
        };
        let ledger_ttl = if ttl as u32 > MIN_TEMP_TTL { ttl as u32 } else { MIN_TEMP_TTL };
        env.storage().temporary().set(&key, &entry);
        env.storage().temporary().extend_ttl(&key, ledger_ttl, ledger_ttl);
        
        // Increment count if new entry
        if !entry_exists {
            let current_count = Self::get_cache_count(env.clone());
            env.storage().instance().set(&Self::cache_count_key(&env), &(current_count + 1));
            env.storage().instance().extend_ttl(INSTANCE_TTL, INSTANCE_TTL);
        }
    }

    pub fn get_cached_metadata(env: Env, anchor: Address) -> AnchorMetadata {
        let key = (symbol_short!("METACACHE"), anchor);
        let entry: MetadataCache = env.storage().temporary().get(&key)
            .unwrap_or_else(|| panic_with_error!(&env, ErrorCode::CacheNotFound));
        let now = env.ledger().timestamp();
        if entry.cached_at + entry.ttl_seconds <= now {
            panic_with_error!(&env, ErrorCode::CacheExpired);
        }
        entry.metadata
    }

    pub fn refresh_metadata_cache(env: Env, anchor: Address) {
        Self::require_admin(&env);
        let key = (symbol_short!("METACACHE"), anchor.clone());
        let had_cached_entry = env.storage().temporary().has(&key);
        Self::record_refresh_diagnostic(
            &env,
            &anchor,
            String::from_str(&env, "metadata"),
            RefreshStatus::Failed,
            had_cached_entry,
            String::from_str(&env, "refresh failed before replacement metadata was available"),
        );
    }

    /// Store a metadata entry with a stale-while-revalidate grace period.
    /// After `ttl_seconds` the entry becomes stale; after `ttl_seconds + stale_ttl_seconds`
    /// it is fully expired and `get_cached_metadata_swr` will return an error.
    pub fn cache_metadata_swr(
        env: Env,
        anchor: Address,
        metadata: AnchorMetadata,
        ttl_seconds: u64,
        stale_ttl_seconds: u64,
    ) {
        Self::require_admin(&env);
        let key = (symbol_short!("METACACHE"), anchor.clone());
        let entry_exists = env.storage().temporary().has(&key);
        
        // Check capacity only if adding a new entry
        if !entry_exists {
            let config = Self::get_capacity_config(env.clone());
            let current_count = Self::get_cache_count(env.clone());
            if current_count >= config.max_cache_entries {
                panic_with_error!(&env, ErrorCode::CacheCapacityExceeded);
            }
        }
        
        let now = env.ledger().timestamp();
        let cfg = Self::get_cache_config(env.clone());
        let ttl = Self::effective_ttl(ttl_seconds, cfg.metadata_ttl_seconds);
        let stale = Self::effective_ttl(stale_ttl_seconds, cfg.swr_ttl_seconds);
        let entry = MetadataCache {
            metadata,
            cached_at: now,
            ttl_seconds: ttl,
            stale_ttl_seconds: stale,
            needs_refresh: false,
        };
        let total_ttl = ttl.saturating_add(stale);
        let ledger_ttl = if total_ttl as u32 > MIN_TEMP_TTL { total_ttl as u32 } else { MIN_TEMP_TTL };
        env.storage().temporary().set(&key, &entry);
        env.storage().temporary().extend_ttl(&key, ledger_ttl, ledger_ttl);
        
        // Increment count if new entry
        if !entry_exists {
            let current_count = Self::get_cache_count(env.clone());
            env.storage().instance().set(&Self::cache_count_key(&env), &(current_count + 1));
            env.storage().instance().extend_ttl(INSTANCE_TTL, INSTANCE_TTL);
        }
    }

    /// Retrieve a metadata entry using the stale-while-revalidate policy.
    ///
    /// Returns `(metadata, needs_refresh)`:
    /// - `needs_refresh = false` → entry is fresh (within primary TTL)
    /// - `needs_refresh = true`  → entry is stale (within grace period); caller should refresh
    ///
    /// Panics with `CacheExpired` once both TTLs have elapsed, or `CacheNotFound` if absent.
    pub fn get_cached_metadata_swr(env: Env, anchor: Address) -> (AnchorMetadata, bool) {
        let key = (symbol_short!("METACACHE"), anchor.clone());
        let mut entry: MetadataCache = env.storage().temporary().get(&key)
            .unwrap_or_else(|| panic_with_error!(&env, ErrorCode::CacheNotFound));
        let now = env.ledger().timestamp();
        let age = now.saturating_sub(entry.cached_at);

        if age <= entry.ttl_seconds {
            // Fresh
            (entry.metadata, false)
        } else if age <= entry.ttl_seconds.saturating_add(entry.stale_ttl_seconds) {
            // Stale — mark needs_refresh and persist the flag
            entry.needs_refresh = true;
            env.storage().temporary().set(&key, &entry);
            (entry.metadata, true)
        } else {
            panic_with_error!(&env, ErrorCode::CacheExpired);
        }
    }

    /// Unconditionally replace the cached metadata entry, resetting both TTL clocks.
    pub fn force_refresh_metadata(
        env: Env,
        anchor: Address,
        metadata: AnchorMetadata,
        ttl_seconds: u64,
        stale_ttl_seconds: u64,
    ) {
        Self::require_admin(&env);
        let now = env.ledger().timestamp();
        let cfg = Self::get_cache_config(env.clone());
        let ttl = Self::effective_ttl(ttl_seconds, cfg.metadata_ttl_seconds);
        let stale = Self::effective_ttl(stale_ttl_seconds, cfg.swr_ttl_seconds);
        let entry = MetadataCache {
            metadata,
            cached_at: now,
            ttl_seconds: ttl,
            stale_ttl_seconds: stale,
            needs_refresh: false,
        };
        let key = (symbol_short!("METACACHE"), anchor.clone());
        let total_ttl = ttl.saturating_add(stale);
        let ledger_ttl = if total_ttl as u32 > MIN_TEMP_TTL { total_ttl as u32 } else { MIN_TEMP_TTL };
        env.storage().temporary().set(&key, &entry);
        env.storage().temporary().extend_ttl(&key, ledger_ttl, ledger_ttl);
    }

    /// Report the SWR lifecycle state of an anchor's metadata cache entry
    /// without panicking. This makes both fresh and stale availability explicit:
    /// callers can distinguish `Fresh`, `Stale` (serve-but-refresh), `Expired`
    /// (do not serve), and `Missing` rather than relying on a thrown error.
    ///
    /// Unlike [`get_cached_metadata_swr`](Self::get_cached_metadata_swr) this is a
    /// pure read — it never mutates the stored `needs_refresh` flag.
    pub fn get_metadata_cache_state(env: Env, anchor: Address) -> MetadataCacheState {
        let key = (symbol_short!("METACACHE"), anchor);
        let entry: MetadataCache = match env.storage().temporary().get(&key) {
            Some(e) => e,
            None => return MetadataCacheState::Missing,
        };
        let now = env.ledger().timestamp();
        let age = now.saturating_sub(entry.cached_at);
        if age <= entry.ttl_seconds {
            MetadataCacheState::Fresh
        } else if age <= entry.ttl_seconds.saturating_add(entry.stale_ttl_seconds) {
            MetadataCacheState::Stale
        } else {
            MetadataCacheState::Expired
        }
    }

    /// Complete an in-flight stale-while-revalidate refresh with freshly-fetched
    /// metadata, preserving the last-known-good entry until the new data is
    /// validated.
    ///
    /// Refresh semantics (issue #236):
    /// - **Last-known-good preservation** — incoming metadata is validated
    ///   *before* any storage write (see [`validate_metadata`]). If validation
    ///   fails the call panics and the previously cached entry is left
    ///   untouched, so a failed refresh never drops a usable cache entry.
    /// - **Idempotent** — if the supplied metadata is byte-for-byte identical to
    ///   the currently cached metadata *and* the entry is still `Fresh`, the call
    ///   is a no-op: the `cached_at` clock is not reset, so repeated refreshes
    ///   with unchanged data are stable. A refresh of a `Stale`/`Expired` entry
    ///   (or with changed data) always rewrites and resets both TTL clocks.
    ///
    /// This is the SWR-aware counterpart to the destructive
    /// [`refresh_metadata_cache`](Self::refresh_metadata_cache), which only
    /// invalidates. Prefer this when you have replacement data in hand.
    pub fn refresh_metadata_cache_swr(
        env: Env,
        anchor: Address,
        metadata: AnchorMetadata,
        ttl_seconds: u64,
        stale_ttl_seconds: u64,
    ) {
        Self::require_admin(&env);
        // Validate before touching storage so last-known-good survives a bad refresh.
        Self::validate_metadata(&env, &anchor, &metadata);

        let key = (symbol_short!("METACACHE"), anchor.clone());
        let now = env.ledger().timestamp();

        if let Some(existing) = env
            .storage()
            .temporary()
            .get::<_, MetadataCache>(&key)
        {
            let age = now.saturating_sub(existing.cached_at);
            let still_fresh = age <= existing.ttl_seconds;
            if still_fresh && existing.metadata == metadata {
                // Idempotent no-op: nothing changed and the entry is still fresh.
                return;
            }
        }

        let entry = MetadataCache {
            metadata,
            cached_at: now,
            ttl_seconds,
            stale_ttl_seconds,
            needs_refresh: false,
        };
        let total_ttl = ttl_seconds.saturating_add(stale_ttl_seconds);
        let ledger_ttl = if total_ttl as u32 > MIN_TEMP_TTL { total_ttl as u32 } else { MIN_TEMP_TTL };
        env.storage().temporary().set(&key, &entry);
        env.storage().temporary().extend_ttl(&key, ledger_ttl, ledger_ttl);
    }

    // -----------------------------------------------------------------------
    // Capabilities cache
    // -----------------------------------------------------------------------

    pub fn cache_capabilities(env: Env, anchor: Address, toml_url: String, capabilities: String, ttl_seconds: u64) {
        Self::require_admin(&env);
        let key = (symbol_short!("CAPCACHE"), anchor.clone());
        let entry_exists = env.storage().temporary().has(&key);
        
        // Check capacity only if adding a new entry
        if !entry_exists {
            let config = Self::get_capacity_config(env.clone());
            let current_count = Self::get_cache_count(env.clone());
            if current_count >= config.max_cache_entries {
                panic_with_error!(&env, ErrorCode::CacheCapacityExceeded);
            }
        }
        
        let now = env.ledger().timestamp();
        let cfg = Self::get_cache_config(env.clone());
        let ttl = Self::effective_ttl(ttl_seconds, cfg.capabilities_ttl_seconds);
        let entry = CapabilitiesCache { toml_url, capabilities, cached_at: now, ttl_seconds: ttl };
        let ledger_ttl = if ttl as u32 > MIN_TEMP_TTL { ttl as u32 } else { MIN_TEMP_TTL };
        env.storage().temporary().set(&key, &entry);
        env.storage().temporary().extend_ttl(&key, ledger_ttl, ledger_ttl);
        
        // Increment count if new entry
        if !entry_exists {
            let current_count = Self::get_cache_count(env.clone());
            env.storage().instance().set(&Self::cache_count_key(&env), &(current_count + 1));
            env.storage().instance().extend_ttl(INSTANCE_TTL, INSTANCE_TTL);
        }
    }

    pub fn get_cached_capabilities(env: Env, anchor: Address) -> CapabilitiesCache {
        let key = (symbol_short!("CAPCACHE"), anchor);
        let entry: CapabilitiesCache = env.storage().temporary().get(&key)
            .unwrap_or_else(|| panic_with_error!(&env, ErrorCode::CacheNotFound));
        let now = env.ledger().timestamp();
        if entry.cached_at + entry.ttl_seconds <= now {
            panic_with_error!(&env, ErrorCode::CacheExpired);
        }
        entry
    }

    pub fn refresh_capabilities_cache(env: Env, anchor: Address) {
        Self::require_admin(&env);
        let key = (symbol_short!("CAPCACHE"), anchor.clone());
        let had_cached_entry = env.storage().temporary().has(&key);
        Self::record_refresh_diagnostic(
            &env,
            &anchor,
            String::from_str(&env, "capabilities"),
            RefreshStatus::Failed,
            had_cached_entry,
            String::from_str(&env, "refresh failed before replacement capabilities were available"),
        );
    }

    // -----------------------------------------------------------------------
    // Routing
    // -----------------------------------------------------------------------

    pub fn get_quote(env: Env, anchor: Address, quote_id: u64) -> Quote {
        let anchor_xdr = anchor.clone().to_xdr(&env);
        let anchor_raw = xdr_to_vec(&anchor_xdr);
        let key = make_storage_key(&env, &[b"QUOTE", &anchor_raw, &quote_id.to_be_bytes()]);
        env.storage().persistent().get::<_, Quote>(&key)
            .unwrap_or_else(|| panic_with_error!(&env, ErrorCode::NoQuotesAvailable))
    }

    pub fn set_anchor_metadata(
        env: Env,
        anchor: Address,
        reputation_score: u32,
        average_settlement_time: u64,
        liquidity_score: u32,
        uptime_percentage: u32,
        total_volume: u64,
    ) {
        Self::require_admin(&env);
        let meta = RoutingAnchorMeta {
            anchor: anchor.clone(),
            reputation_score,
            average_settlement_time,
            liquidity_score,
            uptime_percentage,
            total_volume,
            is_active: true,
        };
        let meta_key = anchor_meta_key(&env, &anchor);
        env.storage().persistent().set(&meta_key, &meta);
        env.storage().persistent().extend_ttl(&meta_key, PERSISTENT_TTL, PERSISTENT_TTL);

        // ── Version history ──────────────────────────────────────────────────
        // Increment the per-anchor version counter and append a history entry.
        let xdr = anchor.clone().to_xdr(&env);
        let raw = xdr_to_vec(&xdr);
        let vcnt_key = make_storage_key(&env, &[b"METAVCNT", &raw]);
        let version: u32 = env
            .storage()
            .persistent()
            .get::<_, u32>(&vcnt_key)
            .unwrap_or(0)
            + 1;
        env.storage().persistent().set(&vcnt_key, &version);
        env.storage().persistent().extend_ttl(&vcnt_key, PERSISTENT_TTL, PERSISTENT_TTL);

        let history_entry = AnchorMetadataVersion {
            version,
            updated_at: env.ledger().timestamp(),
            reputation_score,
            average_settlement_time,
            liquidity_score,
            uptime_percentage,
            total_volume,
            is_active: true,
        };
        let hkey = make_storage_key(&env, &[b"METAHIST", &raw, &version.to_be_bytes()]);
        env.storage().persistent().set(&hkey, &history_entry);
        env.storage().persistent().extend_ttl(&hkey, PERSISTENT_TTL, PERSISTENT_TTL);

        // Maintain ANCHLIST — stored under a deterministic key (#229)
        let list_key = make_storage_key(&env, &[b"ANCHLIST"]);
        let mut list: Vec<Address> = env.storage().persistent()
            .get::<_, Vec<Address>>(&list_key)
            .unwrap_or_else(|| Vec::new(&env));
        if !list.contains(&anchor) {
            list.push_back(anchor);
            env.storage().persistent().set(&list_key, &list);
            env.storage().persistent().extend_ttl(&list_key, PERSISTENT_TTL, PERSISTENT_TTL);
        }
    }

    // -----------------------------------------------------------------------
    // Anchor metadata version history
    // -----------------------------------------------------------------------

    /// Return the current version number for an anchor's metadata history.
    ///
    /// Returns `0` when no metadata has ever been set for the anchor.
    pub fn get_anchor_meta_version_count(env: Env, anchor: Address) -> u32 {
        let xdr = anchor.clone().to_xdr(&env);
        let raw = xdr_to_vec(&xdr);
        let vcnt_key = make_storage_key(&env, &[b"METAVCNT", &raw]);
        env.storage()
            .persistent()
            .get::<_, u32>(&vcnt_key)
            .unwrap_or(0)
    }

    /// Retrieve a specific historical version of an anchor's metadata.
    ///
    /// Versions are 1-based and increase monotonically with each call to
    /// [`set_anchor_metadata`](Self::set_anchor_metadata).
    ///
    /// # Errors
    ///
    /// Panics with [`ErrorCode::AttestorNotRegistered`] when the requested
    /// version does not exist (never written or TTL expired).
    pub fn get_anchor_metadata_at_version(
        env: Env,
        anchor: Address,
        version: u32,
    ) -> AnchorMetadataVersion {
        let xdr = anchor.clone().to_xdr(&env);
        let raw = xdr_to_vec(&xdr);
        let hkey = make_storage_key(&env, &[b"METAHIST", &raw, &version.to_be_bytes()]);
        env.storage()
            .persistent()
            .get::<_, AnchorMetadataVersion>(&hkey)
            .unwrap_or_else(|| panic_with_error!(&env, ErrorCode::AttestorNotRegistered))
    }

    /// Return the full ordered metadata history for an anchor, from version 1
    /// to the current version, capped at 50 entries to prevent unbounded reads.
    ///
    /// Entries are returned in ascending version order (oldest first).
    /// Versions whose storage entries have expired are silently omitted.
    ///
    /// # Returns
    ///
    /// A [`Vec`] of [`AnchorMetadataVersion`] records, oldest first.
    pub fn get_anchor_metadata_history(
        env: Env,
        anchor: Address,
    ) -> Vec<AnchorMetadataVersion> {
        const MAX_HISTORY: u32 = 50;
        let xdr = anchor.clone().to_xdr(&env);
        let raw = xdr_to_vec(&xdr);
        let vcnt_key = make_storage_key(&env, &[b"METAVCNT", &raw]);
        let total: u32 = env
            .storage()
            .persistent()
            .get::<_, u32>(&vcnt_key)
            .unwrap_or(0);

        let mut history = Vec::new(&env);
        // Start from the oldest version that fits within the cap.
        let start = if total > MAX_HISTORY { total - MAX_HISTORY + 1 } else { 1 };
        for v in start..=total {
            let hkey = make_storage_key(&env, &[b"METAHIST", &raw, &v.to_be_bytes()]);
            if let Some(entry) = env
                .storage()
                .persistent()
                .get::<_, AnchorMetadataVersion>(&hkey)
            {
                history.push_back(entry);
            }
        }
        history
    }

    /// Reactivate a previously deactivated anchor (admin-only). Sets `is_active = true`.
    pub fn reactivate_anchor(env: Env, anchor: Address) {
        Self::require_admin(&env);
        let meta_key = anchor_meta_key(&env, &anchor);
        let mut meta: RoutingAnchorMeta = env
            .storage()
            .persistent()
            .get(&meta_key)
            .unwrap_or_else(|| panic_with_error!(&env, ErrorCode::AttestorNotRegistered));
        meta.is_active = true;
        env.storage().persistent().set(&meta_key, &meta);
        env.storage()
            .persistent()
            .extend_ttl(&meta_key, PERSISTENT_TTL, PERSISTENT_TTL);
    }

    /// Return the full `RoutingAnchorMeta` for an anchor.
    pub fn get_anchor_metadata(env: Env, anchor: Address) -> RoutingAnchorMeta {
        env.storage()
            .persistent()
            .get::<_, RoutingAnchorMeta>(&anchor_meta_key(&env, &anchor))
            .unwrap_or_else(|| panic_with_error!(&env, ErrorCode::AttestorNotRegistered))
    }

    /// Return all anchors in ANCHLIST where `is_active == true`.
    pub fn list_active_anchors(env: Env) -> Vec<Address> {
        let list_key = make_storage_key(&env, &[b"ANCHLIST"]);
        let anchors: Vec<Address> = env
            .storage()
            .persistent()
            .get::<_, Vec<Address>>(&list_key)
            .unwrap_or_else(|| Vec::new(&env));
        let mut active = Vec::new(&env);
        for anchor in anchors.iter() {
            if let Some(meta) = env
                .storage()
                .persistent()
                .get::<_, RoutingAnchorMeta>(&anchor_meta_key(&env, &anchor))
            {
                if meta.is_active {
                    active.push_back(anchor);
                }
            }
        }
        active
    }

    // -----------------------------------------------------------------------
    // Anchor Blacklist Management (#296)
    // -----------------------------------------------------------------------

    /// Add an anchor to the blacklist.
    ///
    /// Blacklisted anchors are excluded from routing and quote selection.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    /// * `anchor` - Address of the anchor to blacklist.
    /// * `reason` - Reason for blacklisting.
    ///
    /// # Authorization
    ///
    /// Requires admin privileges.
    pub fn blacklist_anchor(env: Env, anchor: Address, reason: String) {
        Self::require_admin(&env);
        let entry = AnchorBlacklistEntry {
            anchor: anchor.clone(),
            reason,
            blacklisted_at: env.ledger().timestamp(),
        };
        let key = anchor_blacklist_key(&env, &anchor);
        env.storage().persistent().set(&key, &entry);
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_TTL, PERSISTENT_TTL);
        env.events().publish(
            (symbol_short!("anchor"), symbol_short!("blacklist")),
            anchor,
        );
    }

    /// Remove an anchor from the blacklist.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    /// * `anchor` - Address of the anchor to remove from blacklist.
    ///
    /// # Authorization
    ///
    /// Requires admin privileges.
    pub fn remove_from_blacklist(env: Env, anchor: Address) {
        Self::require_admin(&env);
        let key = anchor_blacklist_key(&env, &anchor);
        env.storage().persistent().remove(&key);
        env.events().publish(
            (symbol_short!("anchor"), symbol_short!("unblklist")),
            anchor,
        );
    }

    /// Check if an anchor is blacklisted.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    /// * `anchor` - Address to check.
    ///
    /// # Returns
    ///
    /// `true` if the anchor is blacklisted, `false` otherwise.
    pub fn is_anchor_blacklisted(env: Env, anchor: Address) -> bool {
        let key = anchor_blacklist_key(&env, &anchor);
        env.storage()
            .persistent()
            .get::<_, AnchorBlacklistEntry>(&key)
            .is_some()
    }

    // -----------------------------------------------------------------------
    // Anchor Cluster Management (#296)
    // -----------------------------------------------------------------------

    /// Create a new anchor cluster.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    /// * `cluster_id` - Unique identifier for the cluster.
    /// * `name` - Human-readable name for the cluster.
    /// * `anchors` - Initial list of anchors in the cluster.
    ///
    /// # Authorization
    ///
    /// Requires admin privileges.
    pub fn create_anchor_cluster(env: Env, cluster_id: String, name: String, anchors: Vec<Address>) {
        Self::require_admin(&env);
        let cluster = AnchorCluster {
            cluster_id: cluster_id.clone(),
            name,
            anchors,
            created_at: env.ledger().timestamp(),
        };
        let key = anchor_cluster_key(&env, &cluster_id);
        env.storage().persistent().set(&key, &cluster);
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_TTL, PERSISTENT_TTL);

        // Add to cluster list
        let list_key = anchor_cluster_list_key(&env);
        let mut cluster_ids: Vec<String> = env
            .storage()
            .persistent()
            .get(&list_key)
            .unwrap_or_else(|| Vec::new(&env));
        cluster_ids.push_back(cluster_id);
        env.storage().persistent().set(&list_key, &cluster_ids);
        env.storage()
            .persistent()
            .extend_ttl(&list_key, PERSISTENT_TTL, PERSISTENT_TTL);

        env.events().publish(
            (symbol_short!("cluster"), symbol_short!("created")),
            (),
        );
    }

    /// Get a cluster by ID.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    /// * `cluster_id` - The cluster identifier.
    ///
    /// # Returns
    ///
    /// The [`AnchorCluster`] if found.
    pub fn get_anchor_cluster(env: Env, cluster_id: String) -> AnchorCluster {
        let key = anchor_cluster_key(&env, &cluster_id);
        env.storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| panic_with_error!(&env, ErrorCode::CacheNotFound))
    }

    /// List all anchor clusters.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment context.
    ///
    /// # Returns
    ///
    /// Vector of cluster IDs.
    pub fn list_anchor_clusters(env: Env) -> Vec<String> {
        let list_key = anchor_cluster_list_key(&env);
        env.storage()
            .persistent()
            .get(&list_key)
            .unwrap_or_else(|| Vec::new(&env))
    }

    pub fn route_transaction(env: Env, options: RoutingOptions) -> Quote {
        validate_currency_code(&env, &options.request.base_asset);
        validate_currency_code(&env, &options.request.quote_asset);
        let now = env.ledger().timestamp();
        let list_key = make_storage_key(&env, &[b"ANCHLIST"]);
        let anchors: Vec<Address> = env.storage().persistent()
            .get::<_, Vec<Address>>(&list_key)
            .unwrap_or_else(|| Vec::new(&env));

        // Collect valid quotes from active anchors
        let mut candidates: Vec<Quote> = Vec::new(&env);
        for anchor in anchors.iter() {
            // #296: Skip blacklisted anchors
            if Self::is_anchor_blacklisted(env.clone(), anchor.clone()) {
                continue;
            }

            // Check reputation filter
            let meta: RoutingAnchorMeta = match env.storage().persistent().get(&anchor_meta_key(&env, &anchor)) {
                Some(m) => m,
                None => continue,
            };
            if !meta.is_active { continue; }
            if meta.reputation_score < options.min_reputation { continue; }

            // Get latest quote for this anchor
            let anchor_xdr = anchor.clone().to_xdr(&env);
            let anchor_raw = xdr_to_vec(&anchor_xdr);
            let lq_key = make_storage_key(&env, &[b"LATESTQ", &anchor_raw]);
            let quote_id: u64 = match env.storage().persistent().get(&lq_key) {
                Some(id) => id,
                None => continue,
            };
            let q_key = make_storage_key(&env, &[b"QUOTE", &anchor_raw, &quote_id.to_be_bytes()]);
            let quote: Quote = match env.storage().persistent().get(&q_key) {
                Some(q) => q,
                None => continue,
            };

            // #238: the anchor must advertise the quote service. An anchor that
            // never configured SERVICE_QUOTES is excluded before scoring even if
            // a stale quote happens to be stored for it.
            if !Self::advertises_quote_service(&env, &anchor) {
                continue;
            }

            // #238: the quote must be for the requested asset pair. Quotes whose
            // base/quote assets differ from the request are not a valid route.
            if quote.base_asset != options.request.base_asset
                || quote.quote_asset != options.request.quote_asset
            {
                continue;
            }

            // Filter expired quotes
            if quote.valid_until <= now { continue; }

            // Filter by amount limits
            if options.request.amount < quote.minimum_amount
                || options.request.amount > quote.maximum_amount
            {
                continue;
            }

            candidates.push_back(quote);
        }

        if candidates.is_empty() {
            panic_with_error!(&env, ErrorCode::NoQuotesAvailable);
        }

        // Enforce compliance check (#38)
        if options.require_compliance {
            // Look for any passing compliance record for this subject
            // We check the generic "kyc" check_type as the standard compliance gate
            let comp_key = compliance_check_key(&env, &options.subject, &String::from_str(&env, "kyc"));
            let passed = env.storage().persistent()
                .get::<_, ComplianceCheck>(&comp_key)
                .map(|r| r.result == 1u32)
                .unwrap_or(false);
            if !passed {
                panic_with_error!(&env, ErrorCode::ComplianceNotMet);
            }
        }

        // Apply strategy: pick best candidate
        let strategy_sym = options.strategy.get(0)
            .unwrap_or_else(|| panic_with_error!(&env, ErrorCode::NoQuotesAvailable));

        let lowest_fee_sym = Symbol::new(&env, "LowestFee");
        let fastest_sym = Symbol::new(&env, "FastestSettlement");
        let reputation_sym = Symbol::new(&env, "HighestReputation");

        let mut best: Quote = match candidates.get(0) {
            Some(q) => q,
            None => panic_with_error!(&env, ErrorCode::NoQuotesAvailable),
        };

        if strategy_sym == lowest_fee_sym {
            for q in candidates.iter() {
                if q.fee_percentage < best.fee_percentage {
                    best = q;
                }
            }
        } else if strategy_sym == fastest_sym {
            // Need settlement time from metadata
            let mut best_time: u64 = anchor_meta_opt(&env, &best.anchor)
                .map(|m| m.average_settlement_time)
                .unwrap_or(u64::MAX);
            for q in candidates.iter() {
                let t = anchor_meta_opt(&env, &q.anchor)
                    .map(|m| m.average_settlement_time)
                    .unwrap_or(u64::MAX);
                if t < best_time {
                    best_time = t;
                    best = q;
                }
            }
        } else if strategy_sym == reputation_sym {
            let mut best_rep: u32 = anchor_meta_opt(&env, &best.anchor)
                .map(|m| m.reputation_score)
                .unwrap_or(0);
            for q in candidates.iter() {
                let rep = anchor_meta_opt(&env, &q.anchor)
                    .map(|m| m.reputation_score)
                    .unwrap_or(0);
                if rep > best_rep {
                    best_rep = rep;
                    best = q;
                }
            }
        } else if strategy_sym == Symbol::new(&env, "WeightedScore") {
            // Issue #55: weighted score = reputation(40%) + liquidity(30%) + uptime(20%) - fee(10%)
            let weighted_score = |meta: &RoutingAnchorMeta, fee_pct: u32| -> u64 {
                let fee_factor = if fee_pct <= 100 { 100 - fee_pct } else { 0 };
                (meta.reputation_score as u64) * 40
                    + (meta.liquidity_score as u64) * 30
                    + (meta.uptime_percentage as u64) * 20
                    + (fee_factor as u64) * 10
            };
            let mut best_score: u64 = anchor_meta_opt(&env, &best.anchor)
                .map(|m| weighted_score(&m, best.fee_percentage))
                .unwrap_or(0);
            for q in candidates.iter() {
                let score = anchor_meta_opt(&env, &q.anchor)
                    .map(|m| weighted_score(&m, q.fee_percentage))
                    .unwrap_or(0);
                if score > best_score {
                    best_score = score;
                    best = q;
                }
            }
        }

        env.events().publish(
            (symbol_short!("webhook"), symbol_short!("event")),
            WebhookEvent {
                event_type: String::from_str(&env, "transaction_routed"),
                transaction_id: best.quote_id,
                timestamp: now,
                payload_hash: Bytes::new(&env),
            },
        );

        best
    }

    /// Return up to `max_results` quotes sorted by descending weighted composite score.
    /// Weights (scaled ×1000) must sum to 1000; panics with `InvalidWeights` otherwise.
    pub fn route_anchors(
        env: Env,
        fee_weight: u32,       // scaled ×1000, e.g. 333 = 0.333
        speed_weight: u32,
        reputation_weight: u32,
        max_results: u32,
        min_reputation: u32,
    ) -> Vec<Quote> {
        let fw = fee_weight as f32 / 1000.0_f32;
        let sw = speed_weight as f32 / 1000.0_f32;
        let rw = reputation_weight as f32 / 1000.0_f32;
        let strategy = WeightedRoutingStrategy {
            fee_weight: fw,
            speed_weight: sw,
            reputation_weight: rw,
        };
        if !strategy.validate() {
            panic_with_error!(&env, ErrorCode::ValidationError);
        }

        let now = env.ledger().timestamp();
        let list_key = make_storage_key(&env, &[b"ANCHLIST"]);
        let anchors: Vec<Address> = env.storage().persistent()
            .get::<_, Vec<Address>>(&list_key)
            .unwrap_or_else(|| Vec::new(&env));

        // First pass: find max values for normalisation
        let mut max_fee: u32 = 1;
        let mut max_settlement: u64 = 1;
        let mut max_reputation: u32 = 1;

        for anchor in anchors.iter() {
            let meta: RoutingAnchorMeta = match anchor_meta_opt(&env, &anchor) {
                Some(m) if m.is_active && m.reputation_score >= min_reputation => m,
                _ => continue,
            };
            let anchor_xdr = anchor.clone().to_xdr(&env);
            let anchor_raw = xdr_to_vec(&anchor_xdr);
            let lq_key = make_storage_key(&env, &[b"LATESTQ", &anchor_raw]);
            let quote_id: u64 = match env.storage().persistent().get(&lq_key) {
                Some(id) => id,
                None => continue,
            };
            let q_key = make_storage_key(&env, &[b"QUOTE", &anchor_raw, &quote_id.to_be_bytes()]);
            let quote: Quote = match env.storage().persistent().get(&q_key) {
                Some(q) => q,
                None => continue,
            };
            if quote.valid_until <= now { continue; }
            if meta.average_settlement_time > max_settlement { max_settlement = meta.average_settlement_time; }
            if meta.reputation_score > max_reputation { max_reputation = meta.reputation_score; }
            if quote.fee_percentage > max_fee { max_fee = quote.fee_percentage; }
        }

        // Second pass: score into a native vec, then sort
        let mut scored: alloc::vec::Vec<(u32, Quote)> = alloc::vec::Vec::new();

        for anchor in anchors.iter() {
            let meta: RoutingAnchorMeta = match anchor_meta_opt(&env, &anchor) {
                Some(m) if m.is_active && m.reputation_score >= min_reputation => m,
                _ => continue,
            };
            let anchor_xdr = anchor.clone().to_xdr(&env);
            let anchor_raw = xdr_to_vec(&anchor_xdr);
            let lq_key = make_storage_key(&env, &[b"LATESTQ", &anchor_raw]);
            let quote_id: u64 = match env.storage().persistent().get(&lq_key) {
                Some(id) => id,
                None => continue,
            };
            let q_key = make_storage_key(&env, &[b"QUOTE", &anchor_raw, &quote_id.to_be_bytes()]);
            let quote: Quote = match env.storage().persistent().get(&q_key) {
                Some(q) => q,
                None => continue,
            };
            if quote.valid_until <= now { continue; }

            let score = strategy.score_anchor(
                quote.fee_percentage,
                meta.average_settlement_time,
                meta.reputation_score,
                max_fee,
                max_settlement,
                max_reputation,
            );
            scored.push(((score * 1_000_000.0_f32) as u32, quote));
        }

        // Sort descending by score
        scored.sort_unstable_by(|a, b| b.0.cmp(&a.0));

        // Return top max_results quotes as a Soroban Vec
        let limit = if max_results == 0 { 3u32 } else { max_results };
        let mut result: Vec<Quote> = Vec::new(&env);
        for (_, quote) in scored.into_iter().take(limit as usize) {
            result.push_back(quote);
        }
        result
    }

    // -----------------------------------------------------------------------
    // Anchor Info Discovery
    // -----------------------------------------------------------------------

    pub fn fetch_anchor_info(
        env: Env,
        anchor: Address,
        toml_data: StellarToml,
        ttl_seconds: u64,
    ) {
        anchor.require_auth();
        for asset in toml_data.currencies.iter() {
            validate_asset_info(&env, &asset);
        }
        let now = env.ledger().timestamp();
        let cfg = Self::get_cache_config(env.clone());
        let ttl = Self::effective_ttl(ttl_seconds, cfg.capabilities_ttl_seconds);
        let cached = CachedToml {
            toml: toml_data,
            cached_at: now,
            ttl_seconds: ttl,
        };
        let key = (symbol_short!("TOMLCACHE"), anchor.clone());
        let ledger_ttl = if ttl as u32 > MIN_TEMP_TTL { ttl as u32 } else { MIN_TEMP_TTL };
        env.storage().temporary().set(&key, &cached);
        env.storage().temporary().extend_ttl(&key, ledger_ttl, ledger_ttl);
    }

    pub fn get_anchor_toml(env: Env, anchor: Address) -> StellarToml {
        let key = (symbol_short!("TOMLCACHE"), anchor);
        let cached: CachedToml = env.storage().temporary().get(&key)
            .unwrap_or_else(|| panic_with_error!(&env, ErrorCode::CacheNotFound));
        let now = env.ledger().timestamp();
        if cached.cached_at + cached.ttl_seconds <= now {
            panic_with_error!(&env, ErrorCode::CacheExpired);
        }
        cached.toml
    }

    pub fn refresh_anchor_info(env: Env, anchor: Address) {
        anchor.require_auth();
        let key = (symbol_short!("TOMLCACHE"), anchor.clone());
        let had_cached_entry = env.storage().temporary().has(&key);
        Self::record_refresh_diagnostic(
            &env,
            &anchor,
            String::from_str(&env, "anchor_info"),
            RefreshStatus::Failed,
            had_cached_entry,
            String::from_str(&env, "refresh failed before replacement anchor info was available"),
        );
    }

    pub fn get_anchor_assets(env: Env, anchor: Address) -> Vec<String> {
        let toml = Self::get_anchor_toml(env.clone(), anchor);
        let mut assets = Vec::new(&env);
        for asset in toml.currencies.iter() {
            assets.push_back(asset.code.clone());
        }
        assets
    }

    pub fn get_anchor_asset_info(
        env: Env,
        anchor: Address,
        asset_code: String,
    ) -> AssetInfo {
        let toml = Self::get_anchor_toml(env.clone(), anchor);
        for asset in toml.currencies.iter() {
            if asset.code == asset_code {
                return asset;
            }
        }
        panic_with_error!(&env, ErrorCode::ValidationError);
    }

    pub fn get_anchor_deposit_limits(
        env: Env,
        anchor: Address,
        asset_code: String,
    ) -> (u64, u64) {
        let asset = Self::get_anchor_asset_info(env, anchor, asset_code);
        (asset.deposit_min_amount, asset.deposit_max_amount)
    }

    pub fn get_anchor_withdrawal_limits(
        env: Env,
        anchor: Address,
        asset_code: String,
    ) -> (u64, u64) {
        let asset = Self::get_anchor_asset_info(env, anchor, asset_code);
        (asset.withdrawal_min_amount, asset.withdrawal_max_amount)
    }

    pub fn get_anchor_deposit_fees(
        env: Env,
        anchor: Address,
        asset_code: String,
    ) -> (u64, u32) {
        let asset = Self::get_anchor_asset_info(env, anchor, asset_code);
        (asset.deposit_fee_fixed, asset.deposit_fee_percent)
    }

    pub fn get_anchor_withdrawal_fees(
        env: Env,
        anchor: Address,
        asset_code: String,
    ) -> (u64, u32) {
        let asset = Self::get_anchor_asset_info(env, anchor, asset_code);
        (asset.withdrawal_fee_fixed, asset.withdrawal_fee_percent)
    }

    pub fn anchor_supports_deposits(
        env: Env,
        anchor: Address,
        asset_code: String,
    ) -> bool {
        match Self::get_anchor_asset_info(env, anchor, asset_code) {
            asset => asset.deposit_enabled,
        }
    }

    pub fn anchor_supports_withdrawals(
        env: Env,
        anchor: Address,
        asset_code: String,
    ) -> bool {
        match Self::get_anchor_asset_info(env, anchor, asset_code) {
            asset => asset.withdrawal_enabled,
        }
    }

    // -----------------------------------------------------------------------
    // Transaction state
    // -----------------------------------------------------------------------

    pub fn create_transaction_record(
        env: Env,
        transaction_id: u64,
        initiator: Address,
    ) -> TransactionStateRecord {
        Self::create_transaction_record_internal(&env, transaction_id, initiator, None)
    }

    /// Create a transaction record with optional routing reason metadata (#298).
    ///
    /// Identical to [`create_transaction_record`] but attaches an optional
    /// `routing_reason` to the record so callers can store why a particular
    /// route or anchor was chosen. The reason persists through all subsequent
    /// state transitions and can be retrieved for auditing via
    /// [`get_transaction_record`].
    ///
    /// # Arguments
    ///
    /// * `routing_reason` – Human-readable code or description explaining why
    ///   this route was chosen (e.g. `"referral"`, `"lowest_fee"`). `None`
    ///   when no reason applies.
    pub fn create_txn_record_with_reason(
        env: Env,
        transaction_id: u64,
        initiator: Address,
        routing_reason: Option<String>,
    ) -> TransactionStateRecord {
        Self::create_transaction_record_internal(&env, transaction_id, initiator, routing_reason)
    }

    fn create_transaction_record_internal(
        env: &Env,
        transaction_id: u64,
        initiator: Address,
        routing_reason: Option<String>,
    ) -> TransactionStateRecord {
        let now = env.ledger().timestamp();
        let current_ledger = env.ledger().sequence();
        let mut history = soroban_sdk::Vec::new(env);
        history.push_back((TransactionState::Pending, now));
        let record = TransactionStateRecord {
            transaction_id,
            state: TransactionState::Pending,
            initiator,
            timestamp: now,
            last_updated: now,
            last_updated_ledger: current_ledger,
            error_message: None,
            state_history: history,
            recovery_metadata: OptRecovery::None,
            routing_reason,
        };
        let key = (symbol_short!("TXSTATE"), transaction_id);
        env.storage().persistent().set(&key, &record);
        env.storage().persistent().extend_ttl(&key, PERSISTENT_TTL, PERSISTENT_TTL);
        // Track in TXIDS list for summarize_transactions_by_status
        let ids_key = symbol_short!("TXIDS");
        let mut ids: soroban_sdk::Vec<u64> = env
            .storage().persistent().get(&ids_key)
            .unwrap_or_else(|| soroban_sdk::Vec::new(env));
        ids.push_back(transaction_id);
        env.storage().persistent().set(&ids_key, &ids);
        env.storage().persistent().extend_ttl(&ids_key, PERSISTENT_TTL, PERSISTENT_TTL);
        record
    }

    /// Advance a transaction from Pending to InProgress.
    pub fn start_transaction_record(env: Env, transaction_id: u64) -> TransactionStateRecord {
        Self::advance_transaction_state_internal(&env, transaction_id, TransactionState::InProgress, None)
    }

    /// Advance a transaction from InProgress to Completed.
    pub fn complete_transaction_record(env: Env, transaction_id: u64) -> TransactionStateRecord {
        Self::advance_transaction_state_internal(&env, transaction_id, TransactionState::Completed, None)
    }

    /// Advance a transaction to Failed with an error message.
    pub fn fail_transaction_record(env: Env, transaction_id: u64, error_message: String) -> TransactionStateRecord {
        Self::advance_transaction_state_internal(&env, transaction_id, TransactionState::Failed, Some(error_message))
    }

    fn advance_transaction_state_internal(
        env: &Env,
        transaction_id: u64,
        new_state: TransactionState,
        error_message: Option<String>,
    ) -> TransactionStateRecord {
        let key = (symbol_short!("TXSTATE"), transaction_id);
        let mut record: TransactionStateRecord = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| panic_with_error!(env, ErrorCode::AttestationNotFound));

        let from_state = record.state;
        if !from_state.is_valid_transition(new_state) {
            panic_with_error!(env, ErrorCode::IllegalTransition);
        }

        let now = env.ledger().timestamp();
        let current_ledger = env.ledger().sequence();
        record.state = new_state;
        record.last_updated = now;
        record.last_updated_ledger = current_ledger;
        record.error_message = error_message.clone();
        record.state_history.push_back((new_state, now));

        if new_state == TransactionState::Failed {
            let reason = error_message
                .unwrap_or_else(|| String::from_str(env, "unspecified failure"));
            record.recovery_metadata = OptRecovery::Some(
                crate::transaction_state_tracker::RecoveryMetadata {
                    failure_reason: reason,
                    last_updated_ledger: current_ledger,
                    failed_from_state: from_state,
                    retry_count: 0,
                },
            );
        }

        let ttl = if new_state.is_terminal() {
            518_400u32 // ~30 days terminal TTL (matches TXSTATE_TTL_TERMINAL)
        } else {
            PERSISTENT_TTL
        };
        env.storage().persistent().set(&key, &record);
        env.storage().persistent().extend_ttl(&key, ttl, ttl);
        record
    }

    // -----------------------------------------------------------------------
    // Rate limit configuration
    // -----------------------------------------------------------------------

    pub fn set_rate_limit_config(env: Env, max_submissions: u32, window_length: u32) {
        Self::require_admin(&env);
        let config = crate::rate_limiter::RateLimitConfig { max_submissions, window_length };
        RateLimiter::update_config(&env, &Self::get_admin(env.clone()), &config)
            .unwrap_or_else(|_| panic_with_error!(&env, ErrorCode::ValidationError));
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Validate that a session is neither expired nor closed.
    /// Panics with `SessionExpired` if `current_time > created_at + ttl`,
    /// or `SessionClosed` if `session.closed == true`.
    fn validate_session(env: &Env, session: &Session) {
        let ttl = if session.session_ttl_seconds == 0 {
            DEFAULT_SESSION_TTL
        } else {
            session.session_ttl_seconds
        };
        let now = env.ledger().timestamp();
        if now > session.created_at + ttl {
            panic_with_error!(env, ErrorCode::SessionExpired);
        }
        if session.closed {
            panic_with_error!(env, ErrorCode::SessionClosed);
        }
    }

    fn enforce_rate_limit(env: &Env, attestor: &Address) {
        let config = RateLimiter::get_config(env);
        if RateLimiter::check_and_increment(env, attestor, &config).is_err() {
            panic_with_error!(env, ErrorCode::RateLimitExceeded);
        }
    }

    fn require_admin(env: &Env) {
        let admin: Address = env
            .storage()
            .instance()
            .get::<_, Address>(&admin_key(env))
            .unwrap_or_else(|| panic_with_error!(env, ErrorCode::NotInitialized));
        admin.require_auth();
    }

    /// Returns `true` if `address` holds `role` OR is the primary admin.
    fn has_role_internal(env: &Env, address: &Address, role: AdminRole) -> bool {
        // Primary admin implicitly has every role.
        if let Some(admin) = env.storage().instance().get::<_, Address>(&admin_key(env)) {
            if *address == admin {
                return true;
            }
        }
        env.storage()
            .persistent()
            .get::<_, bool>(&role_key(env, role, address))
            .unwrap_or(false)
    }

    /// Require that `caller` is either the primary admin or holds `role`.
    ///
    /// Panics with `NotInitialized` if the contract has not been initialised,
    /// or with `Unauthorized` if the caller has neither admin status nor the
    /// required role.
    fn require_admin_or_role(env: &Env, caller: &Address, role: AdminRole) {
        if !Self::has_role_internal(env, caller, role) {
            panic_with_error!(env, ErrorCode::Unauthorized);
        }
        caller.require_auth();
    }

    /// Validate freshly-fetched anchor metadata before it is written to the
    /// SWR cache. Panics with `ValidationError` on any problem so the caller's
    /// last-known-good entry is preserved (no partial writes occur).
    ///
    /// Checks:
    /// - the embedded `metadata.anchor` matches the key `anchor`
    /// - `uptime_percentage` is within range (basis points, 0..=10000)
    fn validate_metadata(env: &Env, anchor: &Address, metadata: &AnchorMetadata) {
        if metadata.anchor != *anchor {
            panic_with_error!(env, ErrorCode::ValidationError);
        }
        if metadata.uptime_percentage > 10_000 {
            panic_with_error!(env, ErrorCode::ValidationError);
        }
    }

    /// Returns `true` if `code` is a service identifier recognised by the
    /// current [`SERVICE_CAPABILITY_VERSION`] (#239).
    fn is_known_service_code(code: u32) -> bool {
        code >= SERVICE_DEPOSITS && code <= MAX_KNOWN_SERVICE_CODE
    }

    /// Sort services in ascending order for deterministic storage.
    /// This ensures consistent behavior regardless of submission order.
    fn sort_services(_env: &Env, services: &mut Vec<u32>) {
        // Simple bubble sort for small vectors (typically 1-4 elements)
        let len = services.len();
        for i in 0..len {
            for j in 0..len - i - 1 {
                let a = services.get(j).unwrap();
                let b = services.get(j + 1).unwrap();
                if a > b {
                    services.set(j, b);
                    services.set(j + 1, a);
                }
            }
        }
    }

    /// Returns `true` iff `anchor` has configured services that include
    /// `SERVICE_QUOTES`. Used by routing (#238) to exclude anchors that do not
    /// advertise the quote service before scoring.
    fn advertises_quote_service(env: &Env, anchor: &Address) -> bool {
        env.storage()
            .persistent()
            .get::<_, AnchorServices>(&(symbol_short!("SERVICES"), anchor.clone()))
            .map(|s| s.services.contains(&SERVICE_QUOTES))
            .unwrap_or(false)
    }

    fn check_attestor(env: &Env, attestor: &Address) {
        let xdr = attestor.clone().to_xdr(env);
        let raw = xdr_to_vec(&xdr);
        if !env
            .storage()
            .persistent()
            .has(&make_storage_key(env, &[b"ATTESTOR", &raw]))
        {
            panic_with_error!(env, ErrorCode::AttestorNotRegistered);
        }
    }

    fn soroban_string_to_rust_string(env: &Env, value: &String) -> RustString {
        let len = value.len() as usize;
        let mut buffer = RustVec::new();
        buffer.resize(len, 0u8);
        value.copy_into_slice(&mut buffer);
        RustString::from_utf8(buffer).unwrap_or_else(|_| {
            panic_with_error!(env, ErrorCode::InvalidEndpointFormat)
        })
    }

    fn verify_attestation_signature(env: &Env, issuer: &Address, payload_hash: &Bytes, signature: &Bytes) {
        let xdr = issuer.clone().to_xdr(env);
        let raw = xdr_to_vec(&xdr);
        let pk: BytesN<32> = env
            .storage()
            .persistent()
            .get(&make_storage_key(env, &[b"ATPUBKEY", &raw]))
            .unwrap_or_else(|| panic_with_error!(env, ErrorCode::UnauthorizedAttestor));
        if signature.len() != 64 {
            panic_with_error!(env, ErrorCode::UnauthorizedAttestor);
        }
        let signature_bytes: BytesN<64> = signature.clone().try_into().unwrap_or_else(|_| {
            panic_with_error!(env, ErrorCode::UnauthorizedAttestor)
        });
        env.crypto()
            .ed25519_verify(&pk, payload_hash, &signature_bytes);
    }

    fn check_timestamp(env: &Env, timestamp: u64) {
        if timestamp == 0 {
            panic_with_error!(env, ErrorCode::InvalidTimestamp);
        }
    }

    fn next_attestation_id(env: &Env) -> u64 {
        let inst = env.storage().instance();
        let ck = soroban_sdk::vec![env, symbol_short!("COUNTER")];
        let id: u64 = inst.get(&ck).unwrap_or(0u64);
        inst.set(&ck, &(id + 1));
        inst.extend_ttl(INSTANCE_TTL, INSTANCE_TTL);
        id
    }

    fn store_attestation(
        env: &Env,
        id: u64,
        issuer: Address,
        subject: Address,
        timestamp: u64,
        payload_hash: Bytes,
        signature: Bytes,
    ) {
        let attestation = Attestation {
            id,
            issuer,
            subject,
            timestamp,
            payload_hash,
            signature,
            schema_version: SCHEMA_V1,
        };
        let key = make_storage_key(env, &[b"ATTEST", &id.to_be_bytes()]);
        env.storage().persistent().set(&key, &attestation);
        env.storage().persistent().extend_ttl(&key, PERSISTENT_TTL, PERSISTENT_TTL);
    }

    fn store_span(
        env: &Env,
        request_id: &RequestId,
        operation: String,
        actor: Address,
        now: u64,
        status: String,
    ) {
        Self::store_span_with_parent(env, request_id, operation, actor, now, status, Bytes::new(env), 0);
    }

    fn store_span_with_parent(
        env: &Env,
        request_id: &RequestId,
        operation: String,
        actor: Address,
        now: u64,
        status: String,
        parent_request_id_bytes: Bytes,
        span_index: u32,
    ) {
        let span = TracingSpan {
            request_id: request_id.clone(),
            operation,
            actor,
            started_at: now,
            completed_at: now,
            status,
            parent_request_id_bytes,
            span_index,
        };
        let key = (symbol_short!("SPAN"), request_id.id.clone());
        env.storage().temporary().set(&key, &span);
        env.storage()
            .temporary()
            .extend_ttl(&key, SPAN_TTL, SPAN_TTL);
    }

    // -----------------------------------------------------------------------
    // Health check APIs (#268)
    // -----------------------------------------------------------------------

    /// Overall service health status.
    ///
    /// Returns `Healthy` when the contract is initialized and the rate limiter
    /// config is present. Returns `Degraded` when initialized but the rate
    /// limiter config is missing (default fallback in use). Returns
    /// `Unavailable` when the contract has not been initialized.
    pub fn get_health_status(env: Env) -> HealthStatus {
        if !env.storage().persistent().has(&initialized_key(&env)) {
            return HealthStatus::Unavailable;
        }
        let rl_key = make_storage_key(&env, &[b"RL_CONFIG"]);
        if env.storage().persistent().has(&rl_key) {
            HealthStatus::Healthy
        } else {
            HealthStatus::Degraded
        }
    }

    /// Metadata freshness report for a given anchor.
    ///
    /// Returns the cache state together with the age of the entry in seconds
    /// (zero when missing). Callers can use this to detect stale or expired
    /// metadata without triggering a panic.
    pub fn get_metadata_freshness(env: Env, anchor: Address) -> MetadataFreshnessReport {
        let key = (symbol_short!("METACACHE"), anchor.clone());
        match env.storage().temporary().get::<_, MetadataCache>(&key) {
            None => MetadataFreshnessReport {
                anchor,
                state: MetadataCacheState::Missing,
                age_seconds: 0,
                needs_refresh: false,
            },
            Some(entry) => {
                let now = env.ledger().timestamp();
                let age = now.saturating_sub(entry.cached_at);
                let state = if age <= entry.ttl_seconds {
                    MetadataCacheState::Fresh
                } else if age <= entry.ttl_seconds.saturating_add(entry.stale_ttl_seconds) {
                    MetadataCacheState::Stale
                } else {
                    MetadataCacheState::Expired
                };
                MetadataFreshnessReport {
                    anchor,
                    state,
                    age_seconds: age,
                    needs_refresh: entry.needs_refresh || state != MetadataCacheState::Fresh,
                }
            }
        }
    }

    /// Rate limiter health for a given attestor.
    ///
    /// Returns the current submission count, window start ledger, configured
    /// limits, and whether the attestor is currently throttled.
    pub fn get_rate_limiter_health(env: Env, attestor: Address) -> RateLimiterHealth {
        let config = RateLimiter::get_config(&env);
        let state = RateLimiter::get_state(&env, &attestor);
        let current_ledger = env.ledger().sequence();
        let window_expired = state.window_start_ledger.saturating_add(config.window_length) <= current_ledger;
        let effective_count = if window_expired { 0 } else { state.submission_count };
        RateLimiterHealth {
            attestor,
            submission_count: effective_count,
            max_submissions: config.max_submissions,
            window_length: config.window_length,
            window_start_ledger: state.window_start_ledger,
            is_throttled: !window_expired && effective_count >= config.max_submissions,
        }
    }

    /// Append `operation_name` to the `RequestContext` stored under `root_id_bytes`.
    /// Creates a minimal context if none exists yet (e.g. for the root operation itself).
    fn record_operation_in_context(env: &Env, root_id_bytes: &Bytes, operation_name: String) {
        let key = (symbol_short!("REQCTX"), root_id_bytes.clone());
        let now = env.ledger().timestamp();
        let mut ctx: RequestContext = env
            .storage()
            .temporary()
            .get(&key)
            .unwrap_or_else(|| RequestContext {
                root_request_id: RequestId {
                    id: root_id_bytes.clone(),
                    created_at: now,
                },
                operation_chain: Vec::new(env),
                created_at: now,
            });
        ctx.operation_chain.push_back(operation_name);
        env.storage().temporary().set(&key, &ctx);
        env.storage()
            .temporary()
            .extend_ttl(&key, SPAN_TTL, SPAN_TTL);
    }

    // -----------------------------------------------------------------------
    // Anchor health and service readiness (#348)
    // -----------------------------------------------------------------------

    /// Return a readiness snapshot for `anchor`, aggregating registration status
    /// and per-service availability. Does not mutate any contract state.
    pub fn get_anchor_readiness(env: Env, anchor: Address) -> AnchorReadinessReport {
        let now = env.ledger().timestamp();
        let is_registered = Self::is_attestor(env.clone(), anchor.clone());

        let xdr = anchor.clone().to_xdr(&env);
        let raw = xdr_to_vec(&xdr);
        let services_opt: Option<AnchorServices> = env
            .storage()
            .persistent()
            .get(&make_storage_key(&env, &[b"SERVICES", &raw]));

        let deposit_ready = services_opt
            .as_ref()
            .map(|s| s.services.contains(&SERVICE_DEPOSITS))
            .unwrap_or(false);
        let withdrawal_ready = services_opt
            .as_ref()
            .map(|s| s.services.contains(&SERVICE_WITHDRAWALS))
            .unwrap_or(false);
        let kyc_ready = services_opt
            .as_ref()
            .map(|s| s.services.contains(&SERVICE_KYC))
            .unwrap_or(false);

        let advertises_quotes = services_opt
            .as_ref()
            .map(|s| s.services.contains(&SERVICE_QUOTES))
            .unwrap_or(false);
        let quote_ready = if advertises_quotes {
            let lq_key = make_storage_key(&env, &[b"LATESTQ", &raw]);
            if let Some(quote_id) = env.storage().persistent().get::<_, u64>(&lq_key) {
                let q_key = make_storage_key(&env, &[b"QUOTE", &raw, &quote_id.to_be_bytes()]);
                env.storage()
                    .persistent()
                    .get::<_, Quote>(&q_key)
                    .map(|q| q.valid_until > now)
                    .unwrap_or(false)
            } else {
                false
            }
        } else {
            false
        };

        AnchorReadinessReport {
            anchor,
            is_registered,
            deposit_ready,
            withdrawal_ready,
            quote_ready,
            kyc_ready,
            checked_at: now,
        }
    }

    /// Return `true` when `anchor` has the deposit service configured.
    /// Does not require the anchor to hold an active quote.
    pub fn is_deposit_ready(env: Env, anchor: Address) -> bool {
        let xdr = anchor.clone().to_xdr(&env);
        let raw = xdr_to_vec(&xdr);
        env.storage()
            .persistent()
            .get::<_, AnchorServices>(&make_storage_key(&env, &[b"SERVICES", &raw]))
            .map(|s| s.services.contains(&SERVICE_DEPOSITS))
            .unwrap_or(false)
    }

    /// Return `true` when `anchor` advertises the quote service AND holds a
    /// currently valid (non-expired) quote on-chain.
    pub fn is_quote_ready(env: Env, anchor: Address) -> bool {
        let now = env.ledger().timestamp();
        let xdr = anchor.clone().to_xdr(&env);
        let raw = xdr_to_vec(&xdr);
        let advertises = env
            .storage()
            .persistent()
            .get::<_, AnchorServices>(&make_storage_key(&env, &[b"SERVICES", &raw]))
            .map(|s| s.services.contains(&SERVICE_QUOTES))
            .unwrap_or(false);
        if !advertises {
            return false;
        }
        let lq_key = make_storage_key(&env, &[b"LATESTQ", &raw]);
        if let Some(quote_id) = env.storage().persistent().get::<_, u64>(&lq_key) {
            let q_key = make_storage_key(&env, &[b"QUOTE", &raw, &quote_id.to_be_bytes()]);
            env.storage()
                .persistent()
                .get::<_, Quote>(&q_key)
                .map(|q| q.valid_until > now)
                .unwrap_or(false)
        } else {
            false
        }
    }

    // -----------------------------------------------------------------------
    // Batch transaction queries and summaries
    // -----------------------------------------------------------------------

    /// Return up to `limit` transaction records whose IDs fall in the inclusive
    /// range `[from_id, to_id]`, ordered by ID ascending.
    ///
    /// The batch size is capped at 100 to prevent unbounded on-chain iteration.
    /// This method reads directly from persistent storage and skips IDs that
    /// have expired (TTL elapsed).
    ///
    /// # Arguments
    ///
    /// * `from_id` - Inclusive lower bound of the transaction ID range.
    /// * `to_id`   - Inclusive upper bound of the transaction ID range.
    /// * `limit`   - Maximum records to return (capped at 100).
    ///
    /// # Returns
    ///
    /// A [`Vec`] of [`TransactionStateRecord`]s sorted by ID ascending.
    pub fn get_transactions_in_range(
        env: Env,
        from_id: u64,
        to_id: u64,
        limit: u32,
    ) -> Vec<TransactionStateRecord> {
        const MAX_BATCH: u32 = 100;
        let effective_limit = limit.min(MAX_BATCH);
        let mut results = Vec::new(&env);

        if from_id > to_id {
            return results;
        }

        let mut id = from_id;
        let mut count = 0u32;
        while id <= to_id && count < effective_limit {
            let key = (symbol_short!("TXSTATE"), id);
            if let Some(record) = env
                .storage()
                .persistent()
                .get::<_, TransactionStateRecord>(&key)
            {
                results.push_back(record);
                count += 1;
            }
            id += 1;
        }
        results
    }

    /// Return aggregated transaction counts grouped by current state.
    ///
    /// Reads the known-IDs list from persistent storage and counts each live
    /// record by its current [`TransactionState`]. Records whose TTL has
    /// elapsed are silently excluded from the totals.
    ///
    /// # Returns
    ///
    /// A [`TransactionStatusSummary`] with per-state counts and a `total_count`.
    pub fn summarize_transactions_by_status(env: Env) -> TransactionStatusSummary {
        use crate::transaction_state_tracker::TransactionState as TxState;

        let ids_key = symbol_short!("TXIDS");
        let ids: Vec<u64> = env
            .storage()
            .persistent()
            .get(&ids_key)
            .unwrap_or_else(|| Vec::new(&env));

        let mut pending_count: u64 = 0;
        let mut in_progress_count: u64 = 0;
        let mut completed_count: u64 = 0;
        let mut failed_count: u64 = 0;
        let mut total_count: u64 = 0;

        for id in ids.iter() {
            let key = (symbol_short!("TXSTATE"), id);
            if let Some(record) = env
                .storage()
                .persistent()
                .get::<_, TransactionStateRecord>(&key)
            {
                match record.state {
                    TxState::Pending    => pending_count += 1,
                    TxState::InProgress => in_progress_count += 1,
                    TxState::Completed  => completed_count += 1,
                    TxState::Failed     => failed_count += 1,
                }
                total_count += 1;
            }
        }

        TransactionStatusSummary {
            pending_count,
            in_progress_count,
            completed_count,
            failed_count,
            total_count,
        }
    }

    // -----------------------------------------------------------------------
    // Read-only diagnostics (#350)
    // -----------------------------------------------------------------------

    /// Return a rate-limiter snapshot for `attestor`. Does not consume a
    /// submission slot or modify any state.
    pub fn get_rate_limiter_diagnostics(env: Env, attestor: Address) -> RateLimiterDiagnostics {
        let config = RateLimiter::get_config(&env);
        let state = RateLimiter::get_state(&env, &attestor);
        let is_at_limit = state.submission_count >= config.max_submissions;
        RateLimiterDiagnostics {
            attestor,
            submission_count: state.submission_count,
            window_start_ledger: state.window_start_ledger,
            max_submissions: config.max_submissions,
            window_length: config.window_length,
            is_at_limit,
            checked_at: env.ledger().timestamp(),
        }
    }

    /// Return cache freshness information for `anchor`. Does not modify any
    /// cache entries.
    pub fn get_cache_diagnostics(env: Env, anchor: Address) -> CacheDiagnostics {
        let now = env.ledger().timestamp();
        let meta_key = (symbol_short!("METACACHE"), anchor.clone());
        let (metadata_cached, metadata_age_seconds, metadata_ttl_seconds) =
            if let Some(entry) = env
                .storage()
                .temporary()
                .get::<_, MetadataCache>(&meta_key)
            {
                let age = now.saturating_sub(entry.cached_at);
                (true, age, entry.ttl_seconds)
            } else {
                (false, 0u64, 0u64)
            };

        let cap_key = (symbol_short!("CAPCACHE"), anchor.clone());
        let (capabilities_cached, capabilities_age_seconds, capabilities_ttl_seconds) =
            if let Some(entry) = env
                .storage()
                .temporary()
                .get::<_, CapabilitiesCache>(&cap_key)
            {
                let age = now.saturating_sub(entry.cached_at);
                (true, age, entry.ttl_seconds)
            } else {
                (false, 0u64, 0u64)
            };

        CacheDiagnostics {
            anchor,
            metadata_cached,
            metadata_age_seconds,
            metadata_ttl_seconds,
            capabilities_cached,
            capabilities_age_seconds,
            capabilities_ttl_seconds,
            checked_at: now,
        }
    }

    /// Return session creation counters. Does not modify any session state.
    pub fn get_session_diagnostics(env: Env) -> SessionDiagnostics {
        let scnt_key = make_storage_key(&env, &[b"SCNT"]);
        let total_sessions_created: u64 = env
            .storage()
            .instance()
            .get(&scnt_key)
            .unwrap_or(0u64);
        SessionDiagnostics {
            total_sessions_created,
            checked_at: env.ledger().timestamp(),
        }
    }

    // -----------------------------------------------------------------------
    // Anchor health metrics
    // -----------------------------------------------------------------------

    /// Storage key for an anchor's health metric counters.
    fn health_metrics_key(env: &Env, anchor: &Address) -> BytesN<32> {
        let xdr = anchor.clone().to_xdr(env);
        let raw = xdr_to_vec(&xdr);
        make_storage_key(env, &[b"HLTHCNT", &raw])
    }

    /// Record a single endpoint health event for `anchor`.
    ///
    /// Pass `success = true` for a successful call (discovery, quote fetch,
    /// capability check) and `false` for a failure. Counters are accumulated
    /// persistently so uptime percentages survive across ledgers.
    ///
    /// Admin-only — callers that integrate AnchorKit into a monitoring loop
    /// should call this after every outbound anchor interaction.
    pub fn record_health_event(env: Env, anchor: Address, success: bool) {
        Self::require_admin(&env);
        let key = Self::health_metrics_key(&env, &anchor);
        let now = env.ledger().timestamp();

        let mut metrics: AnchorHealthMetrics = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or(AnchorHealthMetrics {
                anchor: anchor.clone(),
                success_count: 0,
                failure_count: 0,
                total_calls: 0,
                uptime_bps: 0,
                last_event_at: 0,
            });

        if success {
            metrics.success_count += 1;
        } else {
            metrics.failure_count += 1;
        }
        metrics.total_calls = metrics.success_count + metrics.failure_count;
        metrics.uptime_bps = if metrics.total_calls == 0 {
            0
        } else {
            (metrics.success_count.saturating_mul(10_000) / metrics.total_calls) as u32
        };
        metrics.last_event_at = now;

        env.storage().persistent().set(&key, &metrics);
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_TTL, PERSISTENT_TTL);

        env.events().publish(
            (symbol_short!("health"), symbol_short!("event"), anchor),
            (success, metrics.uptime_bps),
        );
    }

    /// Return the accumulated health metrics for `anchor`.
    ///
    /// Returns a zeroed [`AnchorHealthMetrics`] when no events have been
    /// recorded yet (never panics).
    pub fn get_anchor_health(env: Env, anchor: Address) -> AnchorHealthMetrics {
        let key = Self::health_metrics_key(&env, &anchor);
        env.storage()
            .persistent()
            .get(&key)
            .unwrap_or(AnchorHealthMetrics {
                anchor: anchor.clone(),
                success_count: 0,
                failure_count: 0,
                total_calls: 0,
                uptime_bps: 0,
                last_event_at: 0,
            })
    }

    /// Reset all health counters for `anchor` to zero. Admin-only.
    ///
    /// Useful after a maintenance window or anchor migration where historical
    /// failure counts should not skew the new baseline.
    pub fn reset_anchor_health(env: Env, anchor: Address) {
        Self::require_admin(&env);
        let key = Self::health_metrics_key(&env, &anchor);
        let now = env.ledger().timestamp();
        let metrics = AnchorHealthMetrics {
            anchor: anchor.clone(),
            success_count: 0,
            failure_count: 0,
            total_calls: 0,
            uptime_bps: 0,
            last_event_at: now,
        };
        env.storage().persistent().set(&key, &metrics);
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_TTL, PERSISTENT_TTL);
    }

    // -----------------------------------------------------------------------
    // Proof-of-possession for anchor endpoints
    // -----------------------------------------------------------------------

    /// Storage key for an anchor's proof-of-possession record.
    fn pop_key(env: &Env, anchor: &Address) -> BytesN<32> {
        let xdr = anchor.clone().to_xdr(env);
        let raw = xdr_to_vec(&xdr);
        make_storage_key(env, &[b"ANCHPOP", &raw])
    }

    /// Register a proof-of-possession for `anchor`'s `endpoint`.
    ///
    /// The anchor computes `proof_hash = SHA-256(challenge_bytes || endpoint_bytes)`
    /// where `challenge_bytes` is a nonce the anchor controls (e.g. a value
    /// published in its `stellar.toml` under `ANCHOR_PROOF_CHALLENGE`).
    /// Storing the hash on-chain binds the anchor's Stellar identity to the
    /// endpoint URL without revealing the raw challenge.
    ///
    /// The anchor must authorize this call (`anchor.require_auth()`).
    ///
    /// # Errors
    ///
    /// Panics with [`ErrorCode::AttestorNotRegistered`] when `anchor` is not
    /// a registered attestor.
    /// Panics with [`ErrorCode::InvalidEndpointFormat`] when `endpoint` fails
    /// HTTPS domain validation.
    pub fn register_endpoint_proof(
        env: Env,
        anchor: Address,
        endpoint: String,
        proof_hash: BytesN<32>,
    ) {
        anchor.require_auth();
        Self::check_attestor(&env, &anchor);

        // Validate the endpoint URL before storing.
        let endpoint_str = Self::soroban_string_to_rust_string(&env, &endpoint);
        crate::validate_anchor_domain(&endpoint_str)
            .unwrap_or_else(|_| panic_with_error!(&env, ErrorCode::InvalidEndpointFormat));

        let now = env.ledger().timestamp();
        let record = AnchorProofRecord {
            anchor: anchor.clone(),
            endpoint,
            proof_hash,
            registered_at: now,
            verified: false,
        };
        let key = Self::pop_key(&env, &anchor);
        env.storage().persistent().set(&key, &record);
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_TTL, PERSISTENT_TTL);

        env.events().publish(
            (symbol_short!("pop"), symbol_short!("register"), anchor),
            now,
        );
    }

    /// Verify a proof-of-possession by comparing `proof_hash` against the
    /// stored record for `anchor`.
    ///
    /// Returns `true` and marks the record as `verified = true` when the
    /// supplied hash matches the stored one. Returns `false` on mismatch or
    /// when no proof has been registered.
    ///
    /// This is a pure verification call — it does **not** require admin auth
    /// so that off-chain monitors can call it freely.
    pub fn verify_endpoint_proof(
        env: Env,
        anchor: Address,
        proof_hash: BytesN<32>,
    ) -> bool {
        let key = Self::pop_key(&env, &anchor);
        let mut record: AnchorProofRecord = match env.storage().persistent().get(&key) {
            Some(r) => r,
            None => return false,
        };

        if record.proof_hash != proof_hash {
            env.events().publish(
                (symbol_short!("pop"), symbol_short!("failed"), anchor),
                env.ledger().timestamp(),
            );
            return false;
        }

        // Mark as verified and persist.
        record.verified = true;
        env.storage().persistent().set(&key, &record);
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_TTL, PERSISTENT_TTL);

        env.events().publish(
            (symbol_short!("pop"), symbol_short!("verified"), anchor),
            env.ledger().timestamp(),
        );
        true
    }

    /// Return the stored proof-of-possession record for `anchor`, or `None`
    /// when no proof has been registered.
    pub fn get_endpoint_proof(env: Env, anchor: Address) -> Option<AnchorProofRecord> {
        env.storage()
            .persistent()
            .get(&Self::pop_key(&env, &anchor))
    }

    /// Return an aggregated health snapshot for the contract's key subsystems.
    /// Does not modify any contract state.
    pub fn get_contract_diagnostics(env: Env) -> ContractDiagnostics {
        let now = env.ledger().timestamp();
        let is_initialized = env.storage().persistent().has(&initialized_key(&env));

        let ck = soroban_sdk::vec![&env, symbol_short!("COUNTER")];
        let total_attestations: u64 = env.storage().instance().get(&ck).unwrap_or(0u64);

        let qcnt_key = make_storage_key(&env, &[b"QCNT"]);
        let total_quotes: u64 = env.storage().instance().get(&qcnt_key).unwrap_or(0u64);

        let scnt_key = make_storage_key(&env, &[b"SCNT"]);
        let total_sessions: u64 = env.storage().instance().get(&scnt_key).unwrap_or(0u64);

        let config = RateLimiter::get_config(&env);

        ContractDiagnostics {
            is_initialized,
            total_attestations,
            total_quotes,
            total_sessions,
            rate_limit_max_submissions: config.max_submissions,
            rate_limit_window_length: config.window_length,
            checked_at: now,
        }
    }

    /// Retrieve current replay detection metrics.
    /// Returns aggregated statistics about detected replay attacks.
    pub fn get_replay_metrics(env: Env) -> ReplayMetrics {
        replay_detection::get_replay_metrics(&env)
    }

    /// Retrieve the attempt count for a specific request ID that was replayed.
    /// Returns 0 if no replay attempts have been recorded for this ID.
    pub fn get_replay_count_for_id(env: Env, request_id: Bytes) -> u64 {
        replay_detection::get_replay_count_for_id(&env, &request_id)
    }
}
