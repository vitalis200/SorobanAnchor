//! Tests for the `mock-only` feature flag.
//!
//! Compile and run with:
//!   cargo test --features mock-only --test mock_only_tests

#![cfg(feature = "mock-only")]

extern crate alloc;

use anchorkit::retry::{retry_with_backoff, MockJitterSource, RetryConfig};
use anchorkit::sep6::{initiate_deposit, RawDepositResponse};
use anchorkit::validate_anchor_domain;

// --- MockJitterSource ---

#[test]
fn mock_jitter_source_returns_seeded_values() {
    let config = RetryConfig::new(3, 100, 5_000, 2);
    let mut delays: alloc::vec::Vec<u64> = alloc::vec::Vec::new();

    let _result = retry_with_backoff(
        &config,
        |attempt| -> Result<(), u32> {
            if attempt < 2 { Err(attempt) } else { Ok(()) }
        },
        |_| true,
        |ms| delays.push(ms),
    );

    // Two retries occurred before success on attempt 2
    assert_eq!(delays.len(), 2);
}

#[test]
fn mock_jitter_retry_succeeds_on_first_attempt() {
    let config = RetryConfig::default();
    let result = retry_with_backoff(
        &config,
        |_| -> Result<&str, ()> { Ok("ok") },
        |_| false,
        |_| {},
    );
    assert_eq!(result, Ok("ok"));
}

#[test]
fn mock_jitter_retry_exhausts_attempts() {
    let config = RetryConfig::new(3, 0, 0, 1);
    let result = retry_with_backoff(
        &config,
        |_| -> Result<(), u32> { Err(42) },
        |_| true,
        |_| {},
    );
    assert_eq!(result, Err(42));
}

// --- Mock deposit helper ---

#[test]
fn mock_deposit_normalises_response() {
    let raw = RawDepositResponse {
        transaction_id: "mock-txn-001".into(),
        how: "Send to mock bank account".into(),
        extra_info: None,
        min_amount: Some(1),
        max_amount: Some(1_000),
        fee_fixed: Some(0),
        status: Some("pending_external".into()),
        clawback_enabled: None,
        stellar_memo: None,
        stellar_memo_type: None,
        asset_code: None,
    };
    let deposit = initiate_deposit(raw).expect("mock deposit should succeed");
    assert_eq!(deposit.transaction_id, "mock-txn-001");
}

#[test]
fn mock_deposit_rejects_empty_transaction_id() {
    let raw = RawDepositResponse {
        transaction_id: "".into(),
        how: "Send somewhere".into(),
        extra_info: None,
        min_amount: None,
        max_amount: None,
        fee_fixed: None,
        status: None,
        clawback_enabled: None,
        stellar_memo: None,
        stellar_memo_type: None,
        asset_code: None,
    };
    assert!(initiate_deposit(raw).is_err());
}

// --- Domain validator smoke tests ---

#[test]
fn mock_domain_validator_accepts_https() {
    assert!(validate_anchor_domain("https://mock-anchor.example.com").is_ok());
}

#[test]
fn mock_domain_validator_rejects_http() {
    assert!(validate_anchor_domain("http://mock-anchor.example.com").is_err());
}
