//! CLI Integration Test Harness
//!
//! End-to-end tests that exercise the full deployment → initialization →
//! registration → attestation workflow using the Soroban local simulation
//! environment (soroban-sdk testutils).
//!
//! # Running
//!
//! ```bash
//! # All integration harness tests (local simulation, no network required)
//! cargo test --test cli_integration_harness
//!
//! # With real testnet (requires ANCHOR_CONTRACT_ID + ANCHOR_ADMIN_SECRET)
//! SOROBAN_ANCHOR_INTEGRATION=testnet cargo test --test cli_integration_harness
//! ```
//!
//! # Environment variables
//!
//! | Variable | Purpose |
//! |----------|---------|
//! | `SOROBAN_ANCHOR_INTEGRATION` | Set to `testnet` to run live-network tests |
//! | `ANCHOR_CONTRACT_ID` | Contract ID for live-network tests |
//! | `ANCHOR_ADMIN_SECRET` | Admin signing key for live-network tests |
//! | `STELLAR_NETWORK` | Network name (default: `testnet`) |

#![cfg(test)]

// Pull in the shared SEP-10 test helpers used across all integration tests.
#[path = "sep10_test_util.rs"]
mod sep10_test_util;

extern crate std;

use std::string::{String as StdString, ToString};

use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
use soroban_sdk::{
    testutils::{Address as _, Ledger, LedgerInfo},
    Address, Bytes, BytesN, Env, String, Vec,
};

use anchorkit::contract::{
    AnchorKitContract, AnchorKitContractClient, SERVICE_DEPOSITS, SERVICE_QUOTES,
    SERVICE_WITHDRAWALS,
};

use sep10_test_util::{register_attestor_with_sep10, sign_payload};

// ---------------------------------------------------------------------------
// Harness-local helpers (thin wrappers over sep10_test_util)
// ---------------------------------------------------------------------------

/// Register an attestor via SEP-10 JWT flow using the shared test utility.
fn register_attestor(
    env: &Env,
    client: &AnchorKitContractClient,
    attestor: &Address,
    issuer: &Address,
    key: &SigningKey,
) {
    register_attestor_with_sep10(env, client, attestor, issuer, key);
}

/// Standard ledger setup used across all harness tests.
fn setup_ledger(env: &Env, timestamp: u64) {
    env.ledger().set(LedgerInfo {
        timestamp,
        protocol_version: 21,
        sequence_number: 0,
        network_id: Default::default(),
        base_reserve: 0,
        min_persistent_entry_ttl: 4096,
        min_temp_entry_ttl: 16,
        max_entry_ttl: 6_312_000,
    });
}

/// Deploy (register) the contract and initialize it with an admin.
/// Returns `(client, admin_address)`.
fn deploy_and_initialize(env: &Env) -> (AnchorKitContractClient, Address) {
    let contract_id = env.register_contract(None, AnchorKitContract);
    let client = AnchorKitContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize(&admin);
    (client, admin)
}

// ---------------------------------------------------------------------------
// STEP 1 — Contract deployment and admin initialization
// ---------------------------------------------------------------------------

/// Verifies that the contract deploys cleanly and the admin is stored.
#[test]
fn harness_step1_deploy_and_initialize() {
    let env = Env::default();
    env.mock_all_auths();
    setup_ledger(&env, 1_000_000);

    let (client, admin) = deploy_and_initialize(&env);

    // Admin must be retrievable after initialization.
    let stored_admin = client.get_admin();
    assert_eq!(stored_admin, admin, "admin address mismatch after initialization");

    // Schema version must be SCHEMA_V1 = 1.
    assert_eq!(client.get_schema_version(), 1, "unexpected schema version");

    // Contract version defaults to 0.1.0 before any upgrade.
    let version = client.get_version();
    assert_eq!(version.major, 0);
    assert_eq!(version.minor, 1);
    assert_eq!(version.patch, 0);
}

/// Verifies that calling initialize a second time panics with AlreadyInitialized.
#[test]
#[should_panic]
fn harness_step1_double_initialize_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    setup_ledger(&env, 1_000_000);

    let (client, admin) = deploy_and_initialize(&env);
    // Second call must panic.
    client.initialize(&admin);
}

// ---------------------------------------------------------------------------
// STEP 2 — Attestor registration
// ---------------------------------------------------------------------------

/// Registers an attestor and verifies it is recognized by the contract.
#[test]
fn harness_step2_register_attestor() {
    let env = Env::default();
    env.mock_all_auths();
    setup_ledger(&env, 1_000_000);

    let (client, _admin) = deploy_and_initialize(&env);
    let attestor = Address::generate(&env);
    let key = SigningKey::generate(&mut OsRng);

    register_attestor(&env, &client, &attestor, &attestor, &key);

    assert!(client.is_attestor(&attestor), "attestor should be registered");
}

/// Verifies that registering the same attestor twice panics.
#[test]
#[should_panic]
fn harness_step2_duplicate_registration_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    setup_ledger(&env, 1_000_000);

    let (client, _admin) = deploy_and_initialize(&env);
    let attestor = Address::generate(&env);
    let key = SigningKey::generate(&mut OsRng);

    register_attestor(&env, &client, &attestor, &attestor, &key);
    // Second registration must panic with AttestorAlreadyRegistered.
    register_attestor(&env, &client, &attestor, &attestor, &key);
}

// ---------------------------------------------------------------------------
// STEP 3 — Service configuration
// ---------------------------------------------------------------------------

/// Configures services for a registered attestor and verifies capability detection.
#[test]
fn harness_step3_configure_services() {
    let env = Env::default();
    env.mock_all_auths();
    setup_ledger(&env, 1_000_000);

    let (client, _admin) = deploy_and_initialize(&env);
    let attestor = Address::generate(&env);
    let key = SigningKey::generate(&mut OsRng);
    register_attestor(&env, &client, &attestor, &attestor, &key);

    let mut services = Vec::new(&env);
    services.push_back(SERVICE_DEPOSITS);
    services.push_back(SERVICE_WITHDRAWALS);
    services.push_back(SERVICE_QUOTES);
    client.configure_services(&attestor, &services);

    assert!(client.supports_service(&attestor, &SERVICE_DEPOSITS));
    assert!(client.supports_service(&attestor, &SERVICE_WITHDRAWALS));
    assert!(client.supports_service(&attestor, &SERVICE_QUOTES));

    let record = client.get_supported_services(&attestor);
    assert_eq!(record.services.len(), 3);
}

// ---------------------------------------------------------------------------
// STEP 4 — Attestation submission and retrieval
// ---------------------------------------------------------------------------

/// Submits an attestation and verifies it can be retrieved by ID.
#[test]
fn harness_step4_submit_and_retrieve_attestation() {
    let env = Env::default();
    env.mock_all_auths();
    setup_ledger(&env, 1_000_000);

    let (client, _admin) = deploy_and_initialize(&env);
    let attestor = Address::generate(&env);
    let subject = Address::generate(&env);
    let key = SigningKey::generate(&mut OsRng);
    register_attestor(&env, &client, &attestor, &attestor, &key);

    // Build a 32-byte payload hash and sign it.
    let mut payload_bytes = Bytes::new(&env);
    for b in b"anchorkit-integration-test-hash!" {
        payload_bytes.push_back(*b);
    }
    let signature = sign_payload(&env, &key, &payload_bytes);
    let timestamp = 1_000_001u64;

    let attestation_id = client.submit_attestation(
        &attestor,
        &subject,
        &timestamp,
        &payload_bytes,
        &signature,
    );

    // Retrieve and verify the stored attestation.
    let attestation = client.get_attestation(&attestation_id);
    assert_eq!(attestation.id, attestation_id);
    assert_eq!(attestation.issuer, attestor);
    assert_eq!(attestation.subject, subject);
    assert_eq!(attestation.timestamp, timestamp);
    assert_eq!(attestation.schema_version, 1, "schema version must be SCHEMA_V1");
}

/// Verifies that replaying the same payload hash is rejected.
#[test]
#[should_panic]
fn harness_step4_replay_attack_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    setup_ledger(&env, 1_000_000);

    let (client, _admin) = deploy_and_initialize(&env);
    let attestor = Address::generate(&env);
    let subject = Address::generate(&env);
    let key = SigningKey::generate(&mut OsRng);
    register_attestor(&env, &client, &attestor, &attestor, &key);

    let mut payload_bytes = Bytes::new(&env);
    for b in b"replay-test-payload-hash-32bytes" {
        payload_bytes.push_back(*b);
    }
    let signature = sign_payload(&env, &key, &payload_bytes);

    client.submit_attestation(&attestor, &subject, &1_000_001u64, &payload_bytes, &signature);
    // Second submission with the same hash must panic with ReplayAttack.
    client.submit_attestation(&attestor, &subject, &1_000_002u64, &payload_bytes, &signature);
}

// ---------------------------------------------------------------------------
// STEP 5 — Session-based workflow (register → attest → audit)
// ---------------------------------------------------------------------------

/// Full session workflow: create session → register attestor → submit attestation
/// → verify audit log → close session.
#[test]
fn harness_step5_session_workflow() {
    let env = Env::default();
    env.mock_all_auths();
    setup_ledger(&env, 1_000_000);

    let (client, _admin) = deploy_and_initialize(&env);
    let user = Address::generate(&env);
    let attestor = Address::generate(&env);
    let subject = Address::generate(&env);
    let key = SigningKey::generate(&mut OsRng);

    // Create session.
    let session_id = client.create_session(&user);
    let session = client.get_session(&session_id);
    assert_eq!(session.initiator, user);
    assert!(!session.closed);

    // Register attestor within the session.
    let pk: BytesN<32> = BytesN::from_array(&env, key.verifying_key().as_bytes());
    client.register_attestor_with_session(&session_id, &attestor, &pk);
    assert!(client.is_attestor(&attestor));
    assert_eq!(client.get_session_operation_count(&session_id), 1);

    // Submit attestation within the session.
    let mut payload_bytes = Bytes::new(&env);
    for b in b"session-workflow-test-hash-32byt" {
        payload_bytes.push_back(*b);
    }
    let signature = sign_payload(&env, &key, &payload_bytes);
    let attest_id = client.submit_attestation_with_session(
        &session_id,
        &attestor,
        &subject,
        &1_000_001u64,
        &payload_bytes,
        &signature,
    );
    assert_eq!(client.get_session_operation_count(&session_id), 2);

    // Verify audit log entries.
    let log0 = client.get_audit_log(&0u64);
    assert_eq!(log0.operation.operation_type, String::from_str(&env, "register"));
    let log1 = client.get_audit_log(&1u64);
    assert_eq!(log1.operation.operation_type, String::from_str(&env, "attest"));
    assert_eq!(log1.operation.result_data, attest_id);

    // Close session and verify it is marked closed.
    client.close_session(&session_id, &user);
    let closed = client.get_session(&session_id);
    assert!(closed.closed);
}

// ---------------------------------------------------------------------------
// STEP 6 — Quote submission and routing
// ---------------------------------------------------------------------------

/// Submits a quote and routes a transaction using the LowestFee strategy.
#[test]
fn harness_step6_quote_and_route() {
    use anchorkit::contract::{RoutingOptions, RoutingRequest};
    use soroban_sdk::Symbol;

    let env = Env::default();
    env.mock_all_auths();
    setup_ledger(&env, 1_000_000);

    let (client, _admin) = deploy_and_initialize(&env);
    let anchor = Address::generate(&env);
    let key = SigningKey::generate(&mut OsRng);
    register_attestor(&env, &client, &anchor, &anchor, &key);

    // Configure services including quotes.
    let mut services = Vec::new(&env);
    services.push_back(SERVICE_DEPOSITS);
    services.push_back(SERVICE_QUOTES);
    client.configure_services(&anchor, &services);

    // Set anchor metadata so routing can score it.
    client.set_anchor_metadata(&anchor, &8000u32, &300u64, &8000u32, &9900u32, &1_000_000u64);

    // Submit a quote valid for 1 hour.
    let quote_id = client.submit_quote(
        &anchor,
        &String::from_str(&env, "USD"),
        &String::from_str(&env, "USDC"),
        &10_000u64,  // rate
        &25u32,      // fee_percentage (0.25%)
        &100u64,     // minimum_amount
        &100_000u64, // maximum_amount
        &1_003_600u64, // valid_until (t + 3600)
    );
    assert_eq!(quote_id, 1, "first quote should have ID 1");

    // Route a transaction using LowestFee strategy.
    let mut strategy = Vec::new(&env);
    strategy.push_back(Symbol::new(&env, "LowestFee"));
    let options = RoutingOptions {
        request: RoutingRequest {
            base_asset: String::from_str(&env, "USD"),
            quote_asset: String::from_str(&env, "USDC"),
            amount: 5_000u64,
            operation_type: 1u32,
        },
        strategy,
        min_reputation: 0u32,
        max_anchors: 5u32,
        require_kyc: false,
        require_compliance: false,
        subject: Address::generate(&env),
    };

    let best = client.route_transaction(&options);
    assert_eq!(best.anchor, anchor);
    assert_eq!(best.fee_percentage, 25);
    assert_eq!(best.base_asset, String::from_str(&env, "USD"));
    assert_eq!(best.quote_asset, String::from_str(&env, "USDC"));
}

// ---------------------------------------------------------------------------
// STEP 7 — Attestor revocation and cleanup
// ---------------------------------------------------------------------------

/// Revokes an attestor and verifies it is no longer recognized.
#[test]
fn harness_step7_revoke_attestor() {
    let env = Env::default();
    env.mock_all_auths();
    setup_ledger(&env, 1_000_000);

    let (client, _admin) = deploy_and_initialize(&env);
    let attestor = Address::generate(&env);
    let key = SigningKey::generate(&mut OsRng);
    register_attestor(&env, &client, &attestor, &attestor, &key);

    assert!(client.is_attestor(&attestor));
    client.revoke_attestor(&attestor);
    assert!(!client.is_attestor(&attestor), "attestor should be revoked");
}

/// Verifies that a revoked attestor cannot submit new attestations.
#[test]
#[should_panic]
fn harness_step7_revoked_attestor_cannot_attest() {
    let env = Env::default();
    env.mock_all_auths();
    setup_ledger(&env, 1_000_000);

    let (client, _admin) = deploy_and_initialize(&env);
    let attestor = Address::generate(&env);
    let subject = Address::generate(&env);
    let key = SigningKey::generate(&mut OsRng);
    register_attestor(&env, &client, &attestor, &attestor, &key);
    client.revoke_attestor(&attestor);

    let mut payload_bytes = Bytes::new(&env);
    for b in b"revoked-attestor-test-hash-32byt" {
        payload_bytes.push_back(*b);
    }
    let signature = sign_payload(&env, &key, &payload_bytes);
    // Must panic with AttestorNotRegistered.
    client.submit_attestation(&attestor, &subject, &1_000_001u64, &payload_bytes, &signature);
}

// ---------------------------------------------------------------------------
// STEP 8 — Full end-to-end workflow (deploy → register → attest → verify)
// ---------------------------------------------------------------------------

/// Complete end-to-end harness: deploys the contract, initializes admin,
/// registers an attestor, submits an attestation, verifies state, and cleans up.
/// This is the canonical "does the whole pipeline work?" test.
#[test]
fn harness_e2e_full_workflow() {
    let env = Env::default();
    env.mock_all_auths();
    setup_ledger(&env, 1_000_000);

    // ── Phase 1: Deploy and initialize ──────────────────────────────────────
    let (client, admin) = deploy_and_initialize(&env);
    assert_eq!(client.get_admin(), admin);

    // ── Phase 2: Register attestor ──────────────────────────────────────────
    let attestor = Address::generate(&env);
    let subject = Address::generate(&env);
    let key = SigningKey::generate(&mut OsRng);
    register_attestor(&env, &client, &attestor, &attestor, &key);
    assert!(client.is_attestor(&attestor));

    // ── Phase 3: Configure services ─────────────────────────────────────────
    let mut services = Vec::new(&env);
    services.push_back(SERVICE_DEPOSITS);
    services.push_back(SERVICE_WITHDRAWALS);
    services.push_back(SERVICE_QUOTES);
    client.configure_services(&attestor, &services);
    assert!(client.supports_service(&attestor, &SERVICE_DEPOSITS));

    // ── Phase 4: Submit attestation ─────────────────────────────────────────
    let mut payload_bytes = Bytes::new(&env);
    for b in b"e2e-full-workflow-test-hash-32by" {
        payload_bytes.push_back(*b);
    }
    let signature = sign_payload(&env, &key, &payload_bytes);
    let attest_id = client.submit_attestation(
        &attestor,
        &subject,
        &1_000_001u64,
        &payload_bytes,
        &signature,
    );

    // ── Phase 5: Verify on-chain state ──────────────────────────────────────
    let attestation = client.get_attestation(&attest_id);
    assert_eq!(attestation.issuer, attestor);
    assert_eq!(attestation.subject, subject);
    assert_eq!(attestation.schema_version, 1);

    // ── Phase 6: Cleanup — revoke attestor ──────────────────────────────────
    client.revoke_attestor(&attestor);
    assert!(!client.is_attestor(&attestor));
}

// ---------------------------------------------------------------------------
// STEP 9 — KYC workflow
// ---------------------------------------------------------------------------

/// Submits KYC data, approves it, and verifies the status transitions.
#[test]
fn harness_step9_kyc_workflow() {
    use anchorkit::contract::KycStatus;

    let env = Env::default();
    env.mock_all_auths();
    setup_ledger(&env, 1_000_000);

    let (client, _admin) = deploy_and_initialize(&env);
    let attestor = Address::generate(&env);
    let subject = Address::generate(&env);
    let key = SigningKey::generate(&mut OsRng);
    register_attestor(&env, &client, &attestor, &attestor, &key);

    // Before submission: NotSubmitted.
    assert_eq!(client.get_kyc_status(&subject), KycStatus::NotSubmitted);

    // Submit KYC data.
    let mut data_hash = Bytes::new(&env);
    for b in b"kyc-data-hash-for-subject-32byte" {
        data_hash.push_back(*b);
    }
    client.submit_kyc(&subject, &data_hash, &attestor);
    assert_eq!(client.get_kyc_status(&subject), KycStatus::Pending);

    // Admin approves.
    client.approve_kyc(&subject);
    assert_eq!(client.get_kyc_status(&subject), KycStatus::Approved);
}

// ---------------------------------------------------------------------------
// STEP 10 — CLI binary smoke tests (subprocess-based, skipped without binary)
// ---------------------------------------------------------------------------

/// Runs `anchorkit doctor` as a subprocess and checks it exits cleanly.
/// Skipped when the binary is not built or the environment is not set up.
#[test]
fn harness_cli_doctor_smoke() {
    // Only run when the binary exists in the expected release path.
    let binary = std::path::Path::new("target/release/anchorkit");
    if !binary.exists() {
        eprintln!("SKIP harness_cli_doctor_smoke: binary not found at {:?}", binary);
        return;
    }

    let output = std::process::Command::new(binary)
        .arg("doctor")
        .output();

    match output {
        Ok(out) => {
            let stdout = StdString::from_utf8_lossy(&out.stdout);
            let stderr = StdString::from_utf8_lossy(&out.stderr);
            // Doctor may fail checks but must not crash (exit code 0 or 1 are both valid).
            assert!(
                out.status.code().is_some(),
                "doctor command must exit with a code, not a signal"
            );
            // Must print the environment check header.
            assert!(
                stdout.contains("SorobanAnchor Environment Check")
                    || stderr.contains("SorobanAnchor Environment Check"),
                "doctor output missing expected header"
            );
        }
        Err(e) => {
            eprintln!("SKIP harness_cli_doctor_smoke: could not run binary: {}", e);
        }
    }
}

/// Runs `anchorkit deploy --dry-run` and verifies it exits without deploying.
#[test]
fn harness_cli_deploy_dry_run() {
    let binary = std::path::Path::new("target/release/anchorkit");
    if !binary.exists() {
        eprintln!("SKIP harness_cli_deploy_dry_run: binary not found");
        return;
    }

    let output = std::process::Command::new(binary)
        .args(["deploy", "--network", "testnet", "--dry-run"])
        .output();

    match output {
        Ok(out) => {
            let stdout = StdString::from_utf8_lossy(&out.stdout);
            // Dry-run must print the skip message and not attempt a real deploy.
            assert!(
                stdout.contains("dry-run") || stdout.contains("skipping"),
                "dry-run output missing expected message; got: {}", stdout
            );
        }
        Err(e) => {
            eprintln!("SKIP harness_cli_deploy_dry_run: could not run binary: {}", e);
        }
    }
}

// ---------------------------------------------------------------------------
// STEP 11 — Live testnet integration (opt-in via env var)
// ---------------------------------------------------------------------------

/// Live testnet smoke test. Only runs when `SOROBAN_ANCHOR_INTEGRATION=testnet`
/// and `ANCHOR_CONTRACT_ID` + `ANCHOR_ADMIN_SECRET` are set.
///
/// Calls `get_admin` on the deployed contract to verify it is reachable.
#[test]
fn harness_live_testnet_get_admin() {
    if std::env::var("SOROBAN_ANCHOR_INTEGRATION").as_deref() != Ok("testnet") {
        eprintln!("SKIP harness_live_testnet_get_admin: set SOROBAN_ANCHOR_INTEGRATION=testnet to enable");
        return;
    }

    let contract_id = match std::env::var("ANCHOR_CONTRACT_ID") {
        Ok(id) if !id.is_empty() => id,
        _ => {
            eprintln!("SKIP harness_live_testnet_get_admin: ANCHOR_CONTRACT_ID not set");
            return;
        }
    };
    let source = match std::env::var("ANCHOR_ADMIN_SECRET") {
        Ok(s) if !s.is_empty() => s,
        _ => {
            eprintln!("SKIP harness_live_testnet_get_admin: ANCHOR_ADMIN_SECRET not set");
            return;
        }
    };

    let output = std::process::Command::new("stellar")
        .args([
            "contract", "invoke",
            "--id", &contract_id,
            "--source", &source,
            "--rpc-url", "https://soroban-testnet.stellar.org",
            "--network-passphrase", "Test SDF Network ; September 2015",
            "--", "get_admin",
        ])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let admin = StdString::from_utf8_lossy(&out.stdout).trim().to_string();
            assert!(!admin.is_empty(), "get_admin returned empty result");
            eprintln!("Live testnet admin: {}", admin);
        }
        Ok(out) => {
            let stderr = StdString::from_utf8_lossy(&out.stderr);
            panic!("live testnet get_admin failed: {}", stderr);
        }
        Err(e) => {
            eprintln!("SKIP harness_live_testnet_get_admin: stellar CLI not available: {}", e);
        }
    }
}
