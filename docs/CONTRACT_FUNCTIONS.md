# AnchorKit Contract API Reference

This document provides comprehensive documentation for all exported contract functions, including arguments, return values, and failure scenarios.

## Initialization & Admin

### `initialize(env: Env, admin: Address)`

Initialize the contract with an admin address. Must be called exactly once.

**Arguments:**
- `env` - Soroban environment context
- `admin` - Address that will have admin privileges (must authorize)

**Returns:** None

**Errors:**
- `AlreadyInitialized` (code 1) - if contract is already initialized

**Side effects:** Sets up persistent storage and instance storage

---

### `is_initialized(env: Env) -> bool`

Check if the contract has been initialized.

**Arguments:**
- `env` - Soroban environment context

**Returns:** `true` if initialized, `false` otherwise

**Errors:** None

---

### `get_admin(env: Env) -> Address`

Retrieve the current admin address.

**Arguments:**
- `env` - Soroban environment context

**Returns:** The admin [`Address`]

**Errors:**
- `NotInitialized` (code 23) - if contract not initialized

---

## Contract Versioning & Upgrades

### `get_version(env: Env) -> ContractVersion`

Get the current contract version and upgrade timestamp.

**Arguments:**
- `env` - Soroban environment context

**Returns:** [`ContractVersion`] with major, minor, patch, and upgraded_at

**Errors:** None

---

### `upgrade(env: Env, new_wasm_hash: BytesN<32>)`

Upgrade the contract WASM code to a new version.

**Arguments:**
- `env` - Soroban environment context
- `new_wasm_hash` - SHA-256 hash of new WASM bytecode

**Returns:** None

**Errors:**
- `NotInitialized` (code 23) - if contract not initialized
- `UnauthorizedAttestor` (code 4) - if caller is not admin

**Side effects:**
- Increments patch version
- Records upgrade timestamp
- Emits `UpgradeEvent`

---

### `migrate(env: Env)`

Run post-upgrade migration logic (idempotent).

**Arguments:**
- `env` - Soroban environment context

**Returns:** None

**Errors:**
- `NotInitialized` (code 23) - if contract not initialized

**Side effects:** Performs any necessary data migrations

---

### `get_schema_version(_env: Env) -> u32`

Get the current on-chain data schema version.

**Arguments:**
- `_env` - Soroban environment context

**Returns:** Current schema version (currently 1)

**Errors:** None

---

## Cache Configuration

### `set_cache_config(env: Env, config: CacheConfig)`

Set the global cache configuration for TTLs.

**Arguments:**
- `env` - Soroban environment context
- `config` - [`CacheConfig`] with metadata_ttl_seconds, capabilities_ttl_seconds, swr_ttl_seconds

**Returns:** None

**Errors:**
- `UnauthorizedAttestor` (code 4) - if caller is not admin

**Side effects:** Updates instance storage with new cache configuration

---

### `get_cache_config(env: Env) -> CacheConfig`

Get the current global cache configuration.

**Arguments:**
- `env` - Soroban environment context

**Returns:** [`CacheConfig`] with current TTL settings

**Errors:** None

---

## Request ID Generation

### `generate_request_id(env: Env) -> RequestId`

Generate a deterministic request ID based on ledger timestamp and sequence.

**Arguments:**
- `env` - Soroban environment context

**Returns:** [`RequestId`] with unique ID and creation timestamp

**Errors:** None

---

### `generate_child_request_id(env: Env, root_bytes: Bytes, nonce: u64) -> RequestId`

Generate a child request ID linked to a root request.

**Arguments:**
- `env` - Soroban environment context
- `root_bytes` - The root request ID bytes
- `nonce` - Unique nonce for this child operation

**Returns:** [`RequestId`] derived from root and nonce

**Errors:** None

---

## Attestor Management

### `register_attestor(env: Env, attestor: Address, sep10_token: String, sep10_issuer: Address, public_key: BytesN<32>)`

Register a new attestor with SEP-10 verification.

**Arguments:**
- `env` - Soroban environment context
- `attestor` - Address of the attestor to register
- `sep10_token` - SEP-10 JWT token for verification
- `sep10_issuer` - Issuer address for SEP-10 token validation
- `public_key` - Ed25519 public key for attestation verification

**Returns:** None

**Errors:**
- `AttestorAlreadyRegistered` (code 2) - if attestor already registered
- `InvalidSep10Token` (code 18) - if token is invalid/expired
- `UnauthorizedAttestor` (code 4) - if caller not authorized

**Side effects:** Stores attestor profile and public key

---

### `revoke_attestor(env: Env, attestor: Address)`

Revoke an attestor's registration.

**Arguments:**
- `env` - Soroban environment context
- `attestor` - Address of attestor to revoke

**Returns:** None

**Errors:**
- `AttestorNotRegistered` (code 3) - if attestor not registered
- `UnauthorizedAttestor` (code 4) - if caller not authorized

**Side effects:** Removes attestor from registry

---

### `is_attestor(env: Env, attestor: Address) -> bool`

Check if an address is a registered attestor.

**Arguments:**
- `env` - Soroban environment context
- `attestor` - Address to check

**Returns:** `true` if registered, `false` otherwise

**Errors:** None

---

### `get_attestor_profile(env: Env, attestor: Address) -> AttestorProfile`

Get the complete profile for an attestor.

**Arguments:**
- `env` - Soroban environment context
- `attestor` - Address of attestor

**Returns:** [`AttestorProfile`] with endpoint, webhook, services, enabled status

**Errors:**
- `AttestorProfileNotFound` (code 50) - if attestor not registered

---

### `set_endpoint(env: Env, attestor: Address, endpoint: String)`

Set the HTTPS endpoint URL for an attestor.

**Arguments:**
- `env` - Soroban environment context
- `attestor` - Address of attestor
- `endpoint` - HTTPS URL for the attestor's API

**Returns:** None

**Errors:**
- `AttestorNotRegistered` (code 3) - if attestor not registered
- `InvalidEndpointFormat` (code 12) - if endpoint format invalid

**Side effects:** Updates attestor profile

---

### `get_endpoint(env: Env, attestor: Address) -> String`

Get the HTTPS endpoint URL for an attestor.

**Arguments:**
- `env` - Soroban environment context
- `attestor` - Address of attestor

**Returns:** Endpoint URL string

**Errors:**
- `AttestorNotRegistered` (code 3) - if attestor not registered

---

### `register_webhook(env: Env, attestor: Address, webhook_url: String)`

Register a webhook URL for an attestor.

**Arguments:**
- `env` - Soroban environment context
- `attestor` - Address of attestor
- `webhook_url` - URL where webhooks will be delivered

**Returns:** None

**Errors:**
- `AttestorNotRegistered` (code 3) - if attestor not registered

**Side effects:** Updates attestor profile with webhook URL

---

### `get_webhook_url(env: Env, attestor: Address) -> String`

Get the webhook URL for an attestor.

**Arguments:**
- `env` - Soroban environment context
- `attestor` - Address of attestor

**Returns:** Webhook URL string

**Errors:**
- `AttestorNotRegistered` (code 3) - if attestor not registered

---

## Service Configuration

### `configure_services(env: Env, anchor: Address, services: Vec<u32>)`

Configure which services an anchor supports.

**Arguments:**
- `env` - Soroban environment context
- `anchor` - Address of the anchor
- `services` - Vector of service type codes (1=deposits, 2=withdrawals, 3=quotes, 4=kyc)

**Returns:** None

**Errors:**
- `InvalidServiceType` (code 8) - if service code not recognized

**Side effects:** Stores service configuration with current schema version

---

### `configure_services_versioned(env: Env, anchor: Address, services: Vec<u32>, version: u32)`

Configure services with explicit schema version.

**Arguments:**
- `env` - Soroban environment context
- `anchor` - Address of the anchor
- `services` - Vector of service type codes
- `version` - Schema version for this configuration

**Returns:** None

**Errors:**
- `InvalidServiceType` (code 8) - if service code not recognized
- `UnsupportedCapabilityVersion` (code 29) - if version newer than current

**Side effects:** Stores versioned service configuration

---

### `current_capability_version(_env: Env) -> u32`

Get the current service capability schema version.

**Arguments:**
- `_env` - Soroban environment context

**Returns:** Current capability version (currently 1)

**Errors:** None

---

### `get_service_capability_version(env: Env, anchor: Address) -> u32`

Get the schema version of an anchor's service configuration.

**Arguments:**
- `env` - Soroban environment context
- `anchor` - Address of the anchor

**Returns:** Schema version of the anchor's service configuration

**Errors:** None

---

### `get_supported_services(env: Env, anchor: Address) -> AnchorServices`

Get all services supported by an anchor.

**Arguments:**
- `env` - Soroban environment context
- `anchor` - Address of the anchor

**Returns:** [`AnchorServices`] with service codes and version

**Errors:** None

---

### `supports_service(env: Env, anchor: Address, service: u32) -> bool`

Check if an anchor supports a specific service.

**Arguments:**
- `env` - Soroban environment context
- `anchor` - Address of the anchor
- `service` - Service type code to check

**Returns:** `true` if service supported, `false` otherwise

**Errors:** None

---

## Attestations

### `submit_attestation(env: Env, issuer: Address, subject: Address, payload_hash: Bytes, signature: Bytes) -> u64`

Submit an attestation with payload hash and signature.

**Arguments:**
- `env` - Soroban environment context
- `issuer` - Address of the attestor issuing the attestation
- `subject` - Address being attested to
- `payload_hash` - SHA-256 hash of the attested payload
- `signature` - Ed25519 signature over the payload hash

**Returns:** Attestation ID (u64)

**Errors:**
- `AttestorNotRegistered` (code 3) - if issuer not registered
- `InvalidTimestamp` (code 5) - if timestamp invalid
- `ReplayAttack` (code 6) - if duplicate attestation detected

**Side effects:** Stores attestation record, emits AttestEvent

---

### `get_attestation(env: Env, id: u64) -> Attestation`

Retrieve an attestation by ID.

**Arguments:**
- `env` - Soroban environment context
- `id` - Attestation ID

**Returns:** [`Attestation`] struct with issuer, subject, payload_hash, signature

**Errors:**
- `AttestationNotFound` (code 17) - if attestation not found

---

### `get_attestation_by_hash(env: Env, issuer: Address, payload_hash: Bytes) -> u64`

Look up an attestation ID by issuer and payload hash.

**Arguments:**
- `env` - Soroban environment context
- `issuer` - Address of the attestor
- `payload_hash` - SHA-256 hash of the payload

**Returns:** Attestation ID

**Errors:**
- `AttestationNotFound` (code 17) - if no matching attestation found

---

### `compute_payload_hash(env: Env, payload: Bytes) -> BytesN<32>`

Compute the SHA-256 hash of a payload.

**Arguments:**
- `env` - Soroban environment context
- `payload` - Raw bytes to hash

**Returns:** 32-byte SHA-256 hash

**Errors:** None

---

### `verify_payload_hash(env: Env, attestation_id: u64, expected_hash: BytesN<32>) -> bool`

Verify that an attestation's payload hash matches expected value.

**Arguments:**
- `env` - Soroban environment context
- `attestation_id` - ID of the attestation
- `expected_hash` - Expected SHA-256 hash

**Returns:** `true` if hashes match, `false` otherwise

**Errors:**
- `AttestationNotFound` (code 17) - if attestation not found

---

## KYC Management

### `submit_kyc(env: Env, subject: Address, data_hash: Bytes, attestor: Address)`

Submit KYC data for verification.

**Arguments:**
- `env` - Soroban environment context
- `subject` - Address undergoing KYC
- `data_hash` - Hash of KYC data
- `attestor` - Address of KYC attestor

**Returns:** None

**Errors:**
- `AttestorNotRegistered` (code 3) - if attestor not registered
- `RateLimitExceeded` (code 16) - if rate limit exceeded

**Side effects:** Creates KYC record with Pending status

---

### `approve_kyc(env: Env, subject: Address)`

Approve a KYC submission.

**Arguments:**
- `env` - Soroban environment context
- `subject` - Address whose KYC to approve

**Returns:** None

**Errors:**
- `KycNotFound` (code 19) - if no KYC record exists
- `UnauthorizedAttestor` (code 4) - if caller not authorized

**Side effects:** Updates KYC status to Approved

---

### `reject_kyc(env: Env, subject: Address, reason_hash: Bytes)`

Reject a KYC submission with reason.

**Arguments:**
- `env` - Soroban environment context
- `subject` - Address whose KYC to reject
- `reason_hash` - Hash of rejection reason

**Returns:** None

**Errors:**
- `KycNotFound` (code 19) - if no KYC record exists
- `UnauthorizedAttestor` (code 4) - if caller not authorized

**Side effects:** Updates KYC status to Rejected

---

### `get_kyc_status(env: Env, subject: Address) -> KycStatus`

Get the KYC status for an address.

**Arguments:**
- `env` - Soroban environment context
- `subject` - Address to check

**Returns:** [`KycStatus`] enum (NotSubmitted, Pending, Approved, Rejected, Expired)

**Errors:** None (returns NotSubmitted if no record)

---

## Sessions

### `create_session(env: Env, initiator: Address) -> u64`

Create a new session for multi-operation workflows.

**Arguments:**
- `env` - Soroban environment context
- `initiator` - Address initiating the session

**Returns:** Session ID (u64)

**Errors:**
- `NotInitialized` (code 23) - if contract not initialized

**Side effects:** Creates session record, emits SessionCreatedEvent

---

### `close_session(env: Env, session_id: u64, initiator: Address)`

Close an active session.

**Arguments:**
- `env` - Soroban environment context
- `session_id` - ID of session to close
- `initiator` - Address that initiated the session

**Returns:** None

**Errors:**
- `SessionClosed` (code 26) - if session already closed
- `SessionExpired` (code 25) - if session has expired

**Side effects:** Marks session as closed, emits SessionClosedEvent

---

### `get_session(env: Env, session_id: u64) -> Session`

Retrieve session details.

**Arguments:**
- `env` - Soroban environment context
- `session_id` - ID of session

**Returns:** [`Session`] struct with metadata and status

**Errors:** None (returns default if not found)

---

### `get_session_operation_count(env: Env, session_id: u64) -> u64`

Get the number of operations performed in a session.

**Arguments:**
- `env` - Soroban environment context
- `session_id` - ID of session

**Returns:** Operation count

**Errors:** None

---

## Quotes

### `submit_quote(env: Env, anchor: Address, base_asset: String, quote_asset: String, rate: u64, fee_percentage: u32, minimum_amount: u64, maximum_amount: u64, valid_until: u64) -> u64`

Submit a quote for an asset pair.

**Arguments:**
- `env` - Soroban environment context
- `anchor` - Address of the anchor providing the quote
- `base_asset` - Source asset code
- `quote_asset` - Destination asset code
- `rate` - Exchange rate (scaled)
- `fee_percentage` - Fee as basis points (0-10000)
- `minimum_amount` - Minimum transaction amount
- `maximum_amount` - Maximum transaction amount
- `valid_until` - Ledger timestamp when quote expires

**Returns:** Quote ID (u64)

**Errors:**
- `InvalidQuote` (code 7) - if fee > 100% or amount limits invalid
- `InvalidAssetCode` (code 53) - if asset codes invalid

**Side effects:** Stores quote record, emits QuoteSubmitEvent

---

### `receive_quote(env: Env, receiver: Address, anchor: Address, quote_id: u64) -> Quote`

Receive/retrieve a quote.

**Arguments:**
- `env` - Soroban environment context
- `receiver` - Address receiving the quote
- `anchor` - Address of the anchor
- `quote_id` - ID of the quote

**Returns:** [`Quote`] struct with all quote details

**Errors:**
- `StaleQuote` (code 10) - if quote has expired

**Side effects:** Emits QuoteReceivedEvent

---

### `get_quote(env: Env, anchor: Address, quote_id: u64) -> Quote`

Get a quote by ID.

**Arguments:**
- `env` - Soroban environment context
- `anchor` - Address of the anchor
- `quote_id` - ID of the quote

**Returns:** [`Quote`] struct

**Errors:** None

---

## Metadata Caching

### `cache_metadata(env: Env, anchor: Address, metadata: AnchorMetadata, ttl_seconds: u64)`

Cache anchor metadata with a TTL.

**Arguments:**
- `env` - Soroban environment context
- `anchor` - Address of the anchor
- `metadata` - [`AnchorMetadata`] to cache
- `ttl_seconds` - Time-to-live in seconds (0 = use default)

**Returns:** None

**Errors:** None

**Side effects:** Stores metadata in persistent cache

---

### `get_cached_metadata(env: Env, anchor: Address) -> AnchorMetadata`

Retrieve cached anchor metadata.

**Arguments:**
- `env` - Soroban environment context
- `anchor` - Address of the anchor

**Returns:** [`AnchorMetadata`]

**Errors:**
- `CacheNotFound` (code 49) - if no cached entry

---

### `get_metadata_cache_state(env: Env, anchor: Address) -> MetadataCacheState`

Get the freshness state of cached metadata without panicking.

**Arguments:**
- `env` - Soroban environment context
- `anchor` - Address of the anchor

**Returns:** [`MetadataCacheState`] (Missing, Fresh, Stale, or Expired)

**Errors:** None

---

### `refresh_metadata_cache(env: Env, anchor: Address)`

Refresh cached metadata (trigger external fetch).

**Arguments:**
- `env` - Soroban environment context
- `anchor` - Address of the anchor

**Returns:** None

**Errors:** None

**Side effects:** Records refresh diagnostic

---

## Tracing & Request Context

### `get_tracing_span(env: Env, request_id_bytes: Bytes) -> Option<TracingSpan>`

Retrieve a tracing span by request ID.

**Arguments:**
- `env` - Soroban environment context
- `request_id_bytes` - Raw bytes of the request ID

**Returns:** [`TracingSpan`] if found, None otherwise

**Errors:** None

---

### `get_trace(env: Env, root_request_id_bytes: Bytes) -> Vec<TracingSpan>`

Get all spans in a trace.

**Arguments:**
- `env` - Soroban environment context
- `root_request_id_bytes` - Root request ID bytes

**Returns:** Vector of [`TracingSpan`] ordered by span_index

**Errors:** None

---

### `create_request_context(env: Env, root_request_id: RequestId) -> RequestContext`

Create a request context for tracking operations.

**Arguments:**
- `env` - Soroban environment context
- `root_request_id` - Root [`RequestId`]

**Returns:** [`RequestContext`] with empty operation chain

**Errors:** None

**Side effects:** Stores request context

---

### `get_request_context(env: Env, root_request_id_bytes: Bytes) -> Option<RequestContext>`

Retrieve a request context.

**Arguments:**
- `env` - Soroban environment context
- `root_request_id_bytes` - Root request ID bytes

**Returns:** [`RequestContext`] if found, None otherwise

**Errors:** None

---

## Routing

### `route_transaction(env: Env, options: RoutingOptions) -> Quote`

Route a transaction to the best anchor based on strategy.

**Arguments:**
- `env` - Soroban environment context
- `options` - [`RoutingOptions`] with request, strategy, filters

**Returns:** [`Quote`] from the selected anchor

**Errors:**
- `NoQuotesAvailable` (code 13) - if no suitable anchors found
- `InvalidWeights` (code 31) - if routing weights invalid

---

### `route_anchors(env: Env, options: RoutingOptions) -> Vec<Address>`

Get a ranked list of anchors for a transaction.

**Arguments:**
- `env` - Soroban environment context
- `options` - [`RoutingOptions`] with request, strategy, filters

**Returns:** Vector of [`Address`] ranked by score

**Errors:**
- `InvalidWeights` (code 31) - if routing weights invalid

---

## Anchor Info Discovery

### `fetch_anchor_info(env: Env, anchor: Address, ttl_seconds: u64) -> StellarToml`

Fetch and cache anchor's stellar.toml.

**Arguments:**
- `env` - Soroban environment context
- `anchor` - Address of the anchor
- `ttl_seconds` - Cache TTL (0 = use default)

**Returns:** [`StellarToml`] with anchor capabilities and assets

**Errors:**
- `InvalidEndpointFormat` (code 12) - if endpoint URL invalid

**Side effects:** Caches stellar.toml

---

### `get_anchor_toml(env: Env, anchor: Address) -> StellarToml`

Get cached stellar.toml for an anchor.

**Arguments:**
- `env` - Soroban environment context
- `anchor` - Address of the anchor

**Returns:** [`StellarToml`]

**Errors:**
- `CacheNotFound` (code 49) - if not cached

---

### `get_anchor_assets(env: Env, anchor: Address) -> Vec<String>`

Get list of asset codes supported by an anchor.

**Arguments:**
- `env` - Soroban environment context
- `anchor` - Address of the anchor

**Returns:** Vector of asset codes

**Errors:** None

---

### `get_anchor_asset_info(env: Env, anchor: Address, asset_code: String) -> AssetInfo`

Get detailed info for a specific asset.

**Arguments:**
- `env` - Soroban environment context
- `anchor` - Address of the anchor
- `asset_code` - Asset code to look up

**Returns:** [`AssetInfo`] with fees, limits, enabled status

**Errors:** None

---

## Rate Limiting

### `set_rate_limit_config(env: Env, max_submissions: u32, window_length: u32)`

Configure rate limiting for KYC submissions.

**Arguments:**
- `env` - Soroban environment context
- `max_submissions` - Maximum submissions per window
- `window_length` - Window length in seconds

**Returns:** None

**Errors:** None

**Side effects:** Updates rate limiter configuration

---

## Error Codes Reference

| Code | Name | Description |
|------|------|-------------|
| 1 | AlreadyInitialized | Contract is already initialized |
| 2 | AttestorAlreadyRegistered | Attestor is already registered |
| 3 | AttestorNotRegistered | Attestor is not registered |
| 4 | UnauthorizedAttestor | Attestor is not authorized |
| 5 | InvalidTimestamp | Timestamp is invalid |
| 6 | ReplayAttack | Replay attack detected |
| 7 | InvalidQuote | Quote is invalid |
| 8 | InvalidServiceType | Service type is invalid |
| 9 | InvalidTransactionIntent | Transaction intent is invalid |
| 10 | StaleQuote | Quote has expired |
| 11 | ComplianceNotMet | Compliance requirements not met |
| 12 | InvalidEndpointFormat | Endpoint format is invalid |
| 13 | NoQuotesAvailable | No quotes are available |
| 14 | ServicesNotConfigured | Services are not configured |
| 15 | ValidationError | Response schema validation failed |
| 16 | RateLimitExceeded | Rate limit exceeded |
| 17 | AttestationNotFound | Attestation not found |
| 18 | InvalidSep10Token | SEP-10 JWT is missing, expired, or invalid |
| 19 | KycNotFound | KYC record not found |
| 20 | KycPending | KYC verification is pending |
| 21 | KycRejected | KYC verification was rejected |
| 22 | WebhookDeliveryFailed | Webhook delivery failed validation |
| 23 | NotInitialized | Contract is not initialized |
| 24 | IllegalTransition | Illegal transaction state transition |
| 25 | SessionExpired | Session has expired |
| 26 | SessionClosed | Session is closed |
| 29 | UnsupportedCapabilityVersion | Service capability version is unsupported |
| 30 | SessionOperationLimitExceeded | Session operation limit exceeded |
| 31 | InvalidWeights | Routing weights must sum to 1.0 |
| 48 | CacheExpired | Cache entry has expired |
| 49 | CacheNotFound | Cache entry not found |
| 50 | AttestorProfileNotFound | Attestor profile not found |
| 51 | InvalidRequestContext | Request context is invalid |
| 52 | InvalidSessionMetadata | Session metadata is invalid |
| 53 | InvalidAssetCode | Asset code is invalid |
