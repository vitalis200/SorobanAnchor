//! Mock fixtures for testing without a live anchor.
//!
//! Enabled by the `mock-only` feature flag. Provides pre-built, valid instances
//! of every public response type so integration tests and CI pipelines can
//! exercise the full parsing and validation pipeline without network access.
//!
//! # Example
//!
//! ```rust
//! # #[cfg(feature = "mock-only")]
//! # {
//! use anchorkit::mock::{mock_deposit_response, mock_interactive_deposit_response};
//! use anchorkit::{initiate_deposit, sep24::initiate_interactive_deposit};
//!
//! // Parse a mock raw deposit response
//! let raw = mock_deposit_response();
//! let deposit = initiate_deposit(raw).expect("mock must be valid");
//! assert_eq!(deposit.transaction_id, "mock-txn-001");
//!
//! // Parse a mock interactive deposit response
//! let raw24 = mock_interactive_deposit_response();
//! let interactive = initiate_interactive_deposit(raw24).expect("mock must be valid");
//! assert!(interactive.url.starts_with("https://"));
//! # }
//! ```

extern crate alloc;
use alloc::string::{String, ToString};
use alloc::format;

use crate::sep6::{RawDepositResponse, RawWithdrawalResponse, RawTransactionResponse};
use crate::sep24::{RawInteractiveDepositResponse, RawInteractiveWithdrawalResponse, RawSep24TransactionResponse};
use crate::sep38::{RawPrice, RawFirmQuote};

// ── Sentinel values ───────────────────────────────────────────────────────────

/// Anchor base URL used in all mock responses.
pub const MOCK_ANCHOR_URL: &str = "https://mock-anchor.example.com";

/// Stellar asset code used in mock responses.
pub const MOCK_ASSET_CODE: &str = "USDC";

/// Transaction ID used in SEP-6 mock responses.
pub const MOCK_TXN_ID: &str = "mock-txn-001";

/// Transaction ID used in SEP-24 mock responses.
pub const MOCK_TXN_ID_24: &str = "mock-txn-24-001";

/// Epoch timestamp used in quote mock responses (2024-01-15 00:00:00 UTC).
pub const MOCK_EXPIRES_AT: u64 = 1_705_276_800;

// ── SEP-6 mocks ───────────────────────────────────────────────────────────────

/// Returns a valid [`RawDepositResponse`] suitable for passing to [`initiate_deposit`].
///
/// [`initiate_deposit`]: crate::initiate_deposit
pub fn mock_deposit_response() -> RawDepositResponse {
    RawDepositResponse {
        transaction_id: MOCK_TXN_ID.to_string(),
        how: "Send USDC to the mock anchor address".to_string(),
        extra_info: Some("Use memo: MOCK001".to_string()),
        min_amount: Some(10),
        max_amount: Some(10_000),
        fee_fixed: Some(1),
        status: Some("pending_external".to_string()),
        clawback_enabled: Some(false),
        stellar_memo: Some("MOCK001".to_string()),
        stellar_memo_type: Some("text".to_string()),
        asset_code: Some(MOCK_ASSET_CODE.to_string()),
    }
}

/// Returns a valid [`RawWithdrawalResponse`] suitable for passing to [`initiate_withdrawal`].
///
/// [`initiate_withdrawal`]: crate::initiate_withdrawal
pub fn mock_withdrawal_response() -> RawWithdrawalResponse {
    RawWithdrawalResponse {
        transaction_id: MOCK_TXN_ID.to_string(),
        account_id: "GXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX".to_string(),
        memo: Some("MOCK-WITHDRAW".to_string()),
        memo_type: Some("text".to_string()),
        min_amount: Some(5),
        max_amount: Some(5_000),
        fee_fixed: Some(2),
        status: Some("pending_anchor".to_string()),
        asset_code: Some(MOCK_ASSET_CODE.to_string()),
    }
}

/// Returns a valid pending [`RawTransactionResponse`] for SEP-6 status polling.
pub fn mock_transaction_response_pending() -> RawTransactionResponse {
    RawTransactionResponse {
        transaction_id: MOCK_TXN_ID.to_string(),
        kind: Some("deposit".to_string()),
        status: "pending_external".to_string(),
        amount_in: Some(100),
        amount_out: Some(99),
        amount_fee: Some(1),
        message: Some("Waiting for external transfer".to_string()),
    }
}

/// Returns a completed [`RawTransactionResponse`] for SEP-6 status polling.
pub fn mock_transaction_response_completed() -> RawTransactionResponse {
    RawTransactionResponse {
        transaction_id: MOCK_TXN_ID.to_string(),
        kind: Some("deposit".to_string()),
        status: "completed".to_string(),
        amount_in: Some(100),
        amount_out: Some(99),
        amount_fee: Some(1),
        message: Some("Deposit complete".to_string()),
    }
}

// ── SEP-24 mocks ──────────────────────────────────────────────────────────────

/// Returns a valid [`RawInteractiveDepositResponse`] suitable for passing to
/// [`initiate_interactive_deposit`].
///
/// [`initiate_interactive_deposit`]: crate::sep24::initiate_interactive_deposit
pub fn mock_interactive_deposit_response() -> RawInteractiveDepositResponse {
    RawInteractiveDepositResponse {
        url: format!("{MOCK_ANCHOR_URL}/sep24/transactions/deposit/interactive?token=mock_jwt"),
        id: MOCK_TXN_ID_24.to_string(),
    }
}

/// Returns a valid [`RawInteractiveWithdrawalResponse`] suitable for passing to
/// [`initiate_interactive_withdrawal`].
///
/// [`initiate_interactive_withdrawal`]: crate::sep24::initiate_interactive_withdrawal
pub fn mock_interactive_withdrawal_response() -> RawInteractiveWithdrawalResponse {
    RawInteractiveWithdrawalResponse {
        url: format!("{MOCK_ANCHOR_URL}/sep24/transactions/withdraw/interactive?token=mock_jwt"),
        id: "mock-txn-24-withdraw-001".to_string(),
    }
}

/// Returns a pending [`RawSep24TransactionResponse`] for SEP-24 status checking.
pub fn mock_sep24_transaction_pending() -> RawSep24TransactionResponse {
    RawSep24TransactionResponse {
        id: MOCK_TXN_ID_24.to_string(),
        status: "pending_user_transfer_start".to_string(),
        more_info_url: Some(format!("{MOCK_ANCHOR_URL}/sep24/transaction?id={MOCK_TXN_ID_24}")),
        stellar_transaction_id: None,
        asset_code: Some(MOCK_ASSET_CODE.to_string()),
    }
}

/// Returns a completed [`RawSep24TransactionResponse`] for SEP-24 status checking.
pub fn mock_sep24_transaction_completed() -> RawSep24TransactionResponse {
    RawSep24TransactionResponse {
        id: MOCK_TXN_ID_24.to_string(),
        status: "completed".to_string(),
        more_info_url: Some(format!("{MOCK_ANCHOR_URL}/sep24/transaction?id={MOCK_TXN_ID_24}")),
        stellar_transaction_id: Some("mock-stellar-sep24-txn".to_string()),
        asset_code: Some(MOCK_ASSET_CODE.to_string()),
    }
}

// ── SEP-38 mocks ──────────────────────────────────────────────────────────────

/// Returns a valid [`RawPrice`] for SEP-38 price queries.
///
/// Asset codes use the plain uppercase form expected by [`fetch_prices`].
///
/// [`fetch_prices`]: crate::sep38::fetch_prices
pub fn mock_price() -> RawPrice {
    RawPrice {
        buy_asset: MOCK_ASSET_CODE.to_string(),
        sell_asset: "XLM".to_string(),
        price: "1.02".to_string(),
    }
}

/// Returns a valid non-expired [`RawFirmQuote`] for SEP-38 quote requests.
///
/// The `expires_at` is set to a fixed future timestamp. Use
/// [`MOCK_EXPIRES_AT`] when checking expiry logic in tests.
/// Asset codes use the plain uppercase form expected by [`request_firm_quote`].
///
/// [`request_firm_quote`]: crate::sep38::request_firm_quote
pub fn mock_firm_quote() -> RawFirmQuote {
    RawFirmQuote {
        id: "mock-quote-001".to_string(),
        expires_at: MOCK_EXPIRES_AT.to_string(),
        price: "1.02".to_string(),
        sell_amount: "100".to_string(),
        buy_amount: "102".to_string(),
        sell_asset: "XLM".to_string(),
        buy_asset: MOCK_ASSET_CODE.to_string(),
    }
}

// ── Composite helpers ─────────────────────────────────────────────────────────

/// A summary of a mock anchor's supported capabilities, returned by
/// [`mock_anchor_capabilities`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockAnchorCapabilities {
    pub anchor_url: String,
    pub asset_code: String,
    pub supports_sep6: bool,
    pub supports_sep24: bool,
    pub supports_sep38: bool,
}

/// Returns a [`MockAnchorCapabilities`] describing the standard mock anchor.
pub fn mock_anchor_capabilities() -> MockAnchorCapabilities {
    MockAnchorCapabilities {
        anchor_url: MOCK_ANCHOR_URL.to_string(),
        asset_code: MOCK_ASSET_CODE.to_string(),
        supports_sep6: true,
        supports_sep24: true,
        supports_sep38: true,
    }
}
