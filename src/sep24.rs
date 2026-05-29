//! SEP-24 Interactive Deposit & Withdrawal Service Layer
//!
//! Provides normalized service functions for initiating interactive deposits,
//! interactive withdrawals, and fetching transaction status for SEP-24 flows.

extern crate alloc;
use alloc::string::String;

use crate::domain_validator::validate_anchor_domain;
use crate::errors::{AnchorKitError, ErrorCode};
use crate::errors::normalize_asset_code;
use crate::sep6::TransactionStatus;

/// Raw response from anchor's `/transactions/deposit/interactive` endpoint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawInteractiveDepositResponse {
    pub url: String,
    pub id: String,
}

/// Raw response from anchor's `/transactions/withdraw/interactive` endpoint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawInteractiveWithdrawalResponse {
    pub url: String,
    pub id: String,
}

/// Raw response from anchor's `/transaction` endpoint for SEP-24.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawSep24TransactionResponse {
    pub id: String,
    pub status: String,
    pub more_info_url: Option<String>,
    pub stellar_transaction_id: Option<String>,
    /// Asset code for this transaction (e.g. `"USDC"`). Normalized to uppercase.
    pub asset_code: Option<String>,
}

/// Normalized response for interactive deposit initiation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InteractiveDepositResponse {
    /// URL to redirect user to for interactive flow.
    pub url: String,
    /// Unique transaction ID assigned by the anchor.
    pub id: String,
}

/// Normalized response for interactive withdrawal initiation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InteractiveWithdrawalResponse {
    /// URL to redirect user to for interactive flow.
    pub url: String,
    /// Unique transaction ID assigned by the anchor.
    pub id: String,
}

/// Normalized response for SEP-24 transaction status.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Sep24TransactionStatusResponse {
    /// Unique transaction ID.
    pub id: String,
    /// Current status of the transaction.
    pub status: TransactionStatus,
    /// URL with more information about the transaction (SEP-24 specific).
    pub more_info_url: Option<String>,
    /// Stellar transaction ID if available (SEP-24 specific).
    pub stellar_transaction_id: Option<String>,
    /// Normalized (uppercase) asset code, if provided.
    pub asset_code: Option<String>,
}

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

/// Validates that a SEP-24 interactive flow URL is a well-formed HTTPS URL.
///
/// In addition to the base `validate_anchor_domain` checks, this function also:
/// - Rejects URLs with embedded userinfo (`https://user:pass@host/...`)
/// - Rejects IP literals in brackets (`https://[::1]/...`)
/// - Rejects excessively long individual URL components (host > 253 chars, path > 2048 chars)
pub fn validate_interactive_url(url: &str) -> Result<(), AnchorKitError> {
    if url.is_empty() {
        return Err(AnchorKitError::invalid_endpoint_format());
    }

    // Must start with https://
    if !url.starts_with("https://") {
        return Err(AnchorKitError::invalid_endpoint_format());
    }

    let after_scheme = &url[8..]; // skip "https://"

    // Reject IP literals: https://[...]
    if after_scheme.starts_with('[') {
        return Err(AnchorKitError::invalid_endpoint_format());
    }

    // Reject userinfo: presence of '@' before the first '/' indicates user:pass@host
    let authority_end = after_scheme.find('/').unwrap_or(after_scheme.len());
    let authority = &after_scheme[..authority_end];
    if authority.contains('@') {
        return Err(AnchorKitError::invalid_endpoint_format());
    }

    // Reject excessively long host component (RFC 1035: max 253 chars)
    let host = authority.split(':').next().unwrap_or(authority);
    if host.len() > 253 {
        return Err(AnchorKitError::invalid_endpoint_format());
    }

    // Reject excessively long path component
    let path = &after_scheme[authority_end..];
    if path.len() > 2048 {
        return Err(AnchorKitError::invalid_endpoint_format());
    }

    validate_anchor_domain(url).map_err(|_| AnchorKitError::invalid_endpoint_format())
}

/// Validates that a transaction ID is non-empty and contains only
/// alphanumeric characters, hyphens, and underscores.
pub fn validate_transaction_id(id: &str) -> Result<(), AnchorKitError> {
    if id.is_empty() {
        return Err(AnchorKitError::new(
            ErrorCode::ValidationError,
            "Transaction ID must not be empty",
        ));
    }
    for c in id.chars() {
        if !c.is_alphanumeric() && c != '-' && c != '_' {
            return Err(AnchorKitError::new(
                ErrorCode::ValidationError,
                "Transaction ID contains invalid characters",
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Service functions
// ---------------------------------------------------------------------------

/// Normalizes the anchor's `/transactions/deposit/interactive` response.
///
/// Validates that both `url` and `id` are non-empty before returning the
/// normalised struct.
///
/// # Arguments
///
/// * `raw` - A [`RawInteractiveDepositResponse`] from the anchor's endpoint.
///
/// # Returns
///
/// A normalised [`InteractiveDepositResponse`] on success.
///
/// # Errors
///
/// Returns [`AnchorKitError`] with code [`ErrorCode::ValidationError`] if
/// `url` or `id` is empty.
///
/// # Examples
///
/// ```rust
/// use anchorkit::sep24::{initiate_interactive_deposit, RawInteractiveDepositResponse};
///
/// let raw = RawInteractiveDepositResponse {
///     url: "https://anchor.example.com/deposit".into(),
///     id: "tx-123".into(),
/// };
/// let resp = initiate_interactive_deposit(raw).unwrap();
/// assert_eq!(resp.url, "https://anchor.example.com/deposit");
/// assert_eq!(resp.id, "tx-123");
/// ```
pub fn initiate_interactive_deposit(
    raw: RawInteractiveDepositResponse,
) -> Result<InteractiveDepositResponse, AnchorKitError> {
    validate_interactive_url(&raw.url)?;
    validate_transaction_id(&raw.id)?;
    Ok(InteractiveDepositResponse {
        url: raw.url,
        id: raw.id,
    })
}

/// Normalizes the anchor's `/transactions/withdraw/interactive` response.
///
/// Validates that both `url` and `id` are non-empty before returning the
/// normalised struct.
///
/// # Arguments
///
/// * `raw` - A [`RawInteractiveWithdrawalResponse`] from the anchor's endpoint.
///
/// # Returns
///
/// A normalised [`InteractiveWithdrawalResponse`] on success.
///
/// # Errors
///
/// Returns [`AnchorKitError`] with code [`ErrorCode::ValidationError`] if
/// `url` or `id` is empty.
///
/// # Examples
///
/// ```rust
/// use anchorkit::sep24::{initiate_interactive_withdrawal, RawInteractiveWithdrawalResponse};
///
/// let raw = RawInteractiveWithdrawalResponse {
///     url: "https://anchor.example.com/withdraw".into(),
///     id: "tx-456".into(),
/// };
/// let resp = initiate_interactive_withdrawal(raw).unwrap();
/// assert_eq!(resp.id, "tx-456");
/// ```
pub fn initiate_interactive_withdrawal(
    raw: RawInteractiveWithdrawalResponse,
) -> Result<InteractiveWithdrawalResponse, AnchorKitError> {
    validate_interactive_url(&raw.url)?;
    validate_transaction_id(&raw.id)?;
    Ok(InteractiveWithdrawalResponse {
        url: raw.url,
        id: raw.id,
    })
}

/// Normalizes the anchor's `/transaction` response for SEP-24 flows.
///
/// Maps SEP-24 specific fields (`more_info_url`, `stellar_transaction_id`) and
/// normalises the status string via [`TransactionStatus::from_str`].
///
/// # Arguments
///
/// * `raw` - A [`RawSep24TransactionResponse`] from the anchor's `/transaction` endpoint.
///
/// # Returns
///
/// A normalised [`Sep24TransactionStatusResponse`] on success.
///
/// # Errors
///
/// Returns [`AnchorKitError`] with code [`ErrorCode::ValidationError`] if
/// `id` or `status` is empty.
///
/// # Examples
///
/// ```rust
/// use anchorkit::sep24::{fetch_sep24_transaction_status, RawSep24TransactionResponse};
/// use anchorkit::TransactionStatus;
///
/// let raw = RawSep24TransactionResponse {
///     id: "tx-789".into(),
///     status: "completed".into(),
///     more_info_url: Some("https://anchor.example.com/tx/tx-789".into()),
///     stellar_transaction_id: Some("stellar-tx-123".into()),
///     asset_code: None,
/// };
/// let resp = fetch_sep24_transaction_status(raw).unwrap();
/// assert_eq!(resp.status, TransactionStatus::Completed);
/// assert!(resp.more_info_url.is_some());
/// ```
pub fn fetch_sep24_transaction_status(
    raw: RawSep24TransactionResponse,
) -> Result<Sep24TransactionStatusResponse, AnchorKitError> {
    if raw.id.is_empty() {
        return Err(AnchorKitError::new(
            ErrorCode::ValidationError,
            "Missing id field in SEP-24 transaction response",
        ));
    }
    if raw.status.is_empty() {
        return Err(AnchorKitError::new(
            ErrorCode::ValidationError,
            "Missing status field in SEP-24 transaction response",
        ));
    }
    if let Some(ref url) = raw.more_info_url {
        validate_interactive_url(url)?;
    }
    let asset_code = raw.asset_code.as_deref()
        .map(normalize_asset_code)
        .transpose()?;

    Ok(Sep24TransactionStatusResponse {
        id: raw.id,
        status: TransactionStatus::from_str(&raw.status),
        more_info_url: raw.more_info_url,
        stellar_transaction_id: raw.stellar_transaction_id,
        asset_code,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    // -----------------------------------------------------------------------

    #[test]
    fn test_validate_interactive_url_accepts_https() {
        assert!(validate_interactive_url("https://anchor.example.com/deposit").is_ok());
    }

    #[test]
    fn test_validate_interactive_url_rejects_http() {
        assert!(validate_interactive_url("http://anchor.example.com/deposit").is_err());
    }

    #[test]
    fn test_validate_interactive_url_rejects_relative() {
        assert!(validate_interactive_url("/deposit/interactive").is_err());
        assert!(validate_interactive_url("deposit/interactive").is_err());
    }

    #[test]
    fn test_validate_interactive_url_rejects_data_uri() {
        assert!(validate_interactive_url("data:text/html,<h1>phish</h1>").is_err());
    }

    #[test]
    fn test_validate_interactive_url_rejects_empty() {
        assert!(validate_interactive_url("").is_err());
    }

    #[test]
    fn test_validate_interactive_url_rejects_userinfo() {
        assert!(validate_interactive_url("https://user:pass@anchor.example.com/deposit").is_err());
        assert!(validate_interactive_url("https://user@anchor.example.com/deposit").is_err());
    }

    #[test]
    fn test_validate_interactive_url_rejects_ip_literal() {
        assert!(validate_interactive_url("https://[::1]/deposit").is_err());
        assert!(validate_interactive_url("https://[2001:db8::1]/deposit").is_err());
    }

    #[test]
    fn test_validate_interactive_url_rejects_long_host() {
        // 254-char host (exceeds RFC 1035 limit of 253)
        let long_host = alloc::format!("https://{}.com/path", "a".repeat(250));
        assert!(validate_interactive_url(&long_host).is_err());
    }

    #[test]
    fn test_validate_interactive_url_rejects_long_path() {
        let long_path = alloc::format!("https://anchor.example.com/{}", "a".repeat(2049));
        assert!(validate_interactive_url(&long_path).is_err());
    }

    #[test]
    fn test_validate_interactive_url_accepts_valid_with_path_and_query() {
        assert!(validate_interactive_url("https://anchor.example.com/sep24/deposit?asset=USDC").is_ok());
    }

    // -----------------------------------------------------------------------
    // validate_transaction_id
    // -----------------------------------------------------------------------

    #[test]
    fn test_validate_transaction_id_accepts_valid() {
        assert!(validate_transaction_id("tx-123").is_ok());
        assert!(validate_transaction_id("tx_abc_456").is_ok());
        assert!(validate_transaction_id("ABC123").is_ok());
    }

    #[test]
    fn test_validate_transaction_id_rejects_empty() {
        assert!(validate_transaction_id("").is_err());
    }

    #[test]
    fn test_validate_transaction_id_rejects_invalid_chars() {
        assert!(validate_transaction_id("tx 123").is_err());
        assert!(validate_transaction_id("tx/123").is_err());
        assert!(validate_transaction_id("tx@123").is_err());
    }

    // -----------------------------------------------------------------------
    // initiate_interactive_deposit
    // -----------------------------------------------------------------------

    #[test]
    fn test_initiate_interactive_deposit_success() {
        let raw = RawInteractiveDepositResponse {
            url: "https://anchor.example.com/deposit".to_string(),
            id: "tx-123".to_string(),
        };
        let result = initiate_interactive_deposit(raw).unwrap();
        assert_eq!(result.url, "https://anchor.example.com/deposit");
        assert_eq!(result.id, "tx-123");
    }

    #[test]
    fn test_initiate_interactive_deposit_rejects_http_url() {
        let raw = RawInteractiveDepositResponse {
            url: "http://anchor.example.com/deposit".to_string(),
            id: "tx-123".to_string(),
        };
        assert!(initiate_interactive_deposit(raw).is_err());
    }

    #[test]
    fn test_initiate_interactive_deposit_rejects_relative_url() {
        let raw = RawInteractiveDepositResponse {
            url: "/deposit/interactive".to_string(),
            id: "tx-123".to_string(),
        };
        assert!(initiate_interactive_deposit(raw).is_err());
    }

    #[test]
    fn test_initiate_interactive_deposit_rejects_data_uri() {
        let raw = RawInteractiveDepositResponse {
            url: "data:text/html,<h1>phish</h1>".to_string(),
            id: "tx-123".to_string(),
        };
        assert!(initiate_interactive_deposit(raw).is_err());
    }

    #[test]
    fn test_initiate_interactive_deposit_missing_url() {
        let raw = RawInteractiveDepositResponse {
            url: "".to_string(),
            id: "tx-123".to_string(),
        };
        assert!(initiate_interactive_deposit(raw).is_err());
    }

    #[test]
    fn test_initiate_interactive_deposit_missing_id() {
        let raw = RawInteractiveDepositResponse {
            url: "https://anchor.example.com/deposit".to_string(),
            id: "".to_string(),
        };
        assert!(initiate_interactive_deposit(raw).is_err());
    }

    // -----------------------------------------------------------------------
    // initiate_interactive_withdrawal
    // -----------------------------------------------------------------------

    #[test]
    fn test_initiate_interactive_withdrawal_success() {
        let raw = RawInteractiveWithdrawalResponse {
            url: "https://anchor.example.com/withdraw".to_string(),
            id: "tx-456".to_string(),
        };
        let result = initiate_interactive_withdrawal(raw).unwrap();
        assert_eq!(result.url, "https://anchor.example.com/withdraw");
        assert_eq!(result.id, "tx-456");
    }

    #[test]
    fn test_initiate_interactive_withdrawal_rejects_http_url() {
        let raw = RawInteractiveWithdrawalResponse {
            url: "http://anchor.example.com/withdraw".to_string(),
            id: "tx-456".to_string(),
        };
        assert!(initiate_interactive_withdrawal(raw).is_err());
    }

    #[test]
    fn test_initiate_interactive_withdrawal_rejects_relative_url() {
        let raw = RawInteractiveWithdrawalResponse {
            url: "/withdraw/interactive".to_string(),
            id: "tx-456".to_string(),
        };
        assert!(initiate_interactive_withdrawal(raw).is_err());
    }

    #[test]
    fn test_initiate_interactive_withdrawal_rejects_data_uri() {
        let raw = RawInteractiveWithdrawalResponse {
            url: "data:text/html,<h1>phish</h1>".to_string(),
            id: "tx-456".to_string(),
        };
        assert!(initiate_interactive_withdrawal(raw).is_err());
    }

    #[test]
    fn test_initiate_interactive_withdrawal_missing_url() {
        let raw = RawInteractiveWithdrawalResponse {
            url: "".to_string(),
            id: "tx-456".to_string(),
        };
        assert!(initiate_interactive_withdrawal(raw).is_err());
    }

    #[test]
    fn test_initiate_interactive_withdrawal_missing_id() {
        let raw = RawInteractiveWithdrawalResponse {
            url: "https://anchor.example.com/withdraw".to_string(),
            id: "".to_string(),
        };
        assert!(initiate_interactive_withdrawal(raw).is_err());
    }

    // -----------------------------------------------------------------------
    // fetch_sep24_transaction_status
    // -----------------------------------------------------------------------

    #[test]
    fn test_fetch_sep24_transaction_status_success() {
        let raw = RawSep24TransactionResponse {
            id: "tx-789".to_string(),
            status: "completed".to_string(),
            more_info_url: Some("https://anchor.example.com/tx/tx-789".to_string()),
            stellar_transaction_id: Some("stellar-tx-123".to_string()),
            asset_code: None,
        };
        let result = fetch_sep24_transaction_status(raw).unwrap();
        assert_eq!(result.id, "tx-789");
        assert_eq!(result.status, TransactionStatus::Completed);
        assert_eq!(
            result.more_info_url,
            Some("https://anchor.example.com/tx/tx-789".to_string())
        );
        assert_eq!(
            result.stellar_transaction_id,
            Some("stellar-tx-123".to_string())
        );
    }

    #[test]
    fn test_fetch_sep24_transaction_status_rejects_http_more_info_url() {
        let raw = RawSep24TransactionResponse {
            id: "tx-789".to_string(),
            status: "completed".to_string(),
            more_info_url: Some("http://anchor.example.com/tx/tx-789".to_string()),
            stellar_transaction_id: None,
            asset_code: None,
        };
        assert!(fetch_sep24_transaction_status(raw).is_err());
    }

    #[test]
    fn test_fetch_sep24_transaction_status_rejects_relative_more_info_url() {
        let raw = RawSep24TransactionResponse {
            id: "tx-789".to_string(),
            status: "completed".to_string(),
            more_info_url: Some("/tx/tx-789".to_string()),
            stellar_transaction_id: None,
            asset_code: None,
        };
        assert!(fetch_sep24_transaction_status(raw).is_err());
    }

    #[test]
    fn test_fetch_sep24_transaction_status_none_more_info_url_ok() {
        let raw = RawSep24TransactionResponse {
            id: "tx-789".to_string(),
            status: "completed".to_string(),
            more_info_url: None,
            stellar_transaction_id: None,
            asset_code: None,
        };
        assert!(fetch_sep24_transaction_status(raw).is_ok());
    }

    #[test]
    fn test_fetch_sep24_transaction_status_missing_id() {
        let raw = RawSep24TransactionResponse {
            id: "".to_string(),
            status: "completed".to_string(),
            more_info_url: None,
            stellar_transaction_id: None,
            asset_code: None,
        };
        assert!(fetch_sep24_transaction_status(raw).is_err());
    }

    #[test]
    fn test_fetch_sep24_transaction_status_missing_status() {
        let raw = RawSep24TransactionResponse {
            id: "tx-789".to_string(),
            status: "".to_string(),
            more_info_url: None,
            stellar_transaction_id: None,
            asset_code: None,
        };
        assert!(fetch_sep24_transaction_status(raw).is_err());
    }

    #[test]
    fn test_fetch_sep24_transaction_status_pending() {
        let raw = RawSep24TransactionResponse {
            id: "tx-pending".to_string(),
            status: "pending_user".to_string(),
            more_info_url: None,
            stellar_transaction_id: None,
            asset_code: None,
        };
        let result = fetch_sep24_transaction_status(raw).unwrap();
        assert_eq!(result.status, TransactionStatus::PendingUser);
    }
}
