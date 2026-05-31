//! Response schema validation for AnchorKit API responses.
//!
//! Validates that anchor API responses contain all required fields before
//! returning them to the SDK consumer. Throws [`Error::ValidationError`] on mismatch.

extern crate alloc;

use crate::errors::Error;

/// A validated deposit response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DepositResponse {
    pub transaction_id: alloc::string::String,
    pub status: alloc::string::String,
    pub deposit_address: alloc::string::String,
    pub expires_at: u64,
}

/// A validated withdraw response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WithdrawResponse {
    pub transaction_id: alloc::string::String,
    pub status: alloc::string::String,
    pub estimated_completion: u64,
}

/// A validated quote response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuoteResponse {
    pub id: alloc::string::String,
    pub status: alloc::string::String,
    pub amount: u64,
    pub asset: alloc::string::String,
    pub fee: u64,
}

/// A validated SEP-38 quote response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Sep38QuoteResponse {
    pub id: alloc::string::String,
    pub expires_at: alloc::string::String,
    pub price: alloc::string::String,
    pub sell_amount: alloc::string::String,
    pub buy_amount: alloc::string::String,
    pub fee: alloc::string::String,
}

/// A validated anchor info response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnchorInfoResponse {
    pub name: alloc::string::String,
    pub supported_assets: alloc::vec::Vec<alloc::string::String>,
}

/// A validated transaction status response.
#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransactionStatusResponse {
    pub transaction_id: alloc::string::String,
    pub status: alloc::string::String,
    pub kind: alloc::string::String,
}

/// Validates a raw deposit response map, returning a typed [`DepositResponse`]
/// or [`Error::validation_error`] if any required field is missing or empty.
///
/// # Arguments
///
/// * `transaction_id` - Unique transaction ID assigned by the anchor (must be non-empty).
/// * `status` - Current transaction status string (must be non-empty).
/// * `deposit_address` - Address or instructions for sending funds (must be non-empty).
/// * `expires_at` - Unix timestamp when the deposit window closes (`0` is accepted).
///
/// # Returns
///
/// A validated [`DepositResponse`] on success.
///
/// # Errors
///
/// Returns [`Error`] with code [`ErrorCode::ValidationError`] if any string
/// field is empty.
///
/// # Examples
///
/// ```rust
/// use anchorkit::validate_deposit_response;
///
/// let resp = validate_deposit_response("dep_123", "pending", "GDEPOSIT...", 2_000_000_000).unwrap();
/// assert_eq!(resp.transaction_id, "dep_123");
///
/// assert!(validate_deposit_response("", "pending", "GDEPOSIT...", 2_000_000_000).is_err());
/// ```
pub fn validate_deposit_response(
    transaction_id: &str,
    status: &str,
    deposit_address: &str,
    expires_at: u64,
) -> Result<DepositResponse, Error> {
    if transaction_id.is_empty() {
        return Err(Error::validation_error("transaction_id is empty"));
    }
    if status.is_empty() {
        return Err(Error::validation_error("status is empty"));
    }
    if !is_valid_sep6_status(status) {
        return Err(Error::validation_error("invalid status value"));
    }
    if deposit_address.is_empty() {
        return Err(Error::validation_error("deposit_address is empty"));
    }
    // expires_at must be 0 (no expiry) or a future Unix timestamp.
    // We use a compile-time lower bound of 1_700_000_000 (Nov 2023) as a
    // proxy for "past" when we cannot call the system clock in no_std.
    if expires_at != 0 && expires_at < 1_700_000_000 {
        return Err(Error::validation_error("expires_at is in the past"));
    }

    Ok(DepositResponse {
        transaction_id: alloc::string::String::from(transaction_id),
        status: alloc::string::String::from(status),
        deposit_address: alloc::string::String::from(deposit_address),
        expires_at,
    })
}

/// Validates a raw withdraw response, returning a typed [`WithdrawResponse`]
/// or [`Error::validation_error`] if any required field is missing or empty.
///
/// # Arguments
///
/// * `transaction_id` - Unique transaction ID assigned by the anchor (must be non-empty).
/// * `status` - Current transaction status string (must be non-empty).
/// * `estimated_completion` - Estimated Unix timestamp for completion (`0` is accepted).
///
/// # Returns
///
/// A validated [`WithdrawResponse`] on success.
///
/// # Errors
///
/// Returns [`Error`] with code [`ErrorCode::ValidationError`] if any string
/// field is empty.
///
/// # Examples
///
/// ```rust
/// use anchorkit::validate_withdraw_response;
///
/// let resp = validate_withdraw_response("wd_456", "processing", 2000).unwrap();
/// assert_eq!(resp.transaction_id, "wd_456");
///
/// assert!(validate_withdraw_response("", "processing", 2000).is_err());
/// ```
pub fn validate_withdraw_response(
    transaction_id: &str,
    status: &str,
    estimated_completion: u64,
) -> Result<WithdrawResponse, Error> {
    if transaction_id.is_empty() {
        return Err(Error::validation_error("transaction_id is empty"));
    }
    if status.is_empty() {
        return Err(Error::validation_error("status is empty"));
    }

    Ok(WithdrawResponse {
        transaction_id: alloc::string::String::from(transaction_id),
        status: alloc::string::String::from(status),
        estimated_completion,
    })
}

/// Validates a raw quote response, returning a typed [`QuoteResponse`]
/// or [`Error::validation_error`] if any required field is missing or empty.
///
/// # Arguments
///
/// * `id` - Unique quote ID (must be non-empty).
/// * `status` - Current quote status string (must be non-empty).
/// * `amount` - Quote amount in asset units (`0` is accepted).
/// * `asset` - Asset code (must be non-empty).
/// * `fee` - Fee in asset units (`0` is accepted).
///
/// # Returns
///
/// A validated [`QuoteResponse`] on success.
///
/// # Errors
///
/// Returns [`Error`] with code [`ErrorCode::ValidationError`] if `id`, `status`,
/// or `asset` is empty.
///
/// # Examples
///
/// ```rust
/// use anchorkit::validate_quote_response;
///
/// let resp = validate_quote_response("q1", "quoted", 100_000_000, "USDC", 500_000).unwrap();
/// assert_eq!(resp.asset, "USDC");
///
/// assert!(validate_quote_response("", "quoted", 0, "USDC", 0).is_err());
/// ```
pub fn validate_quote_response(
    id: &str,
    status: &str,
    amount: u64,
    asset: &str,
    fee: u64,
) -> Result<QuoteResponse, Error> {
    if id.is_empty() {
        return Err(Error::validation_error("id is empty"));
    }
    if status.is_empty() {
        return Err(Error::validation_error("status is empty"));
    }
    if !is_valid_quote_status(status) {
        return Err(Error::validation_error("invalid quote status"));
    }
    if amount == 0 {
        return Err(Error::validation_error("amount must be greater than zero"));
    }
    if asset.is_empty() {
        return Err(Error::validation_error("asset is empty"));
    }
    validate_stellar_asset(asset)?;

    Ok(QuoteResponse {
        id: alloc::string::String::from(id),
        status: alloc::string::String::from(status),
        amount,
        asset: alloc::string::String::from(asset),
        fee,
    })
}

/// Decode a base32-encoded string into bytes.
fn decode_base32(input: &[u8]) -> Option<alloc::vec::Vec<u8>> {
    let mut buffer: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    let mut bits = 0u32;
    let mut value = 0u32;
    for &ch in input {
        let val = decode_base32_value(ch)?;
        value = (value << 5) | (val as u32);
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            buffer.push(((value >> bits) & 0xFF) as u8);
        }
    }
    if bits != 0 {
        return None;
    }
    Some(buffer)
}

fn decode_base32_value(ch: u8) -> Option<u8> {
    match ch {
        b'A'..=b'Z' => Some(ch - b'A'),
        b'2'..=b'7' => Some(ch - b'2' + 26),
        _ => None,
    }
}

fn is_valid_stellar_account_char(c: char) -> bool {
    matches!(c, 'A'..='Z' | '2'..='7')
}

fn is_valid_stellar_strkey(account_id: &str) -> bool {
    const ACCOUNT_ID_VERSION_BYTE: u8 = 6 << 3;
    let decoded = match decode_base32(account_id.as_bytes()) {
        Some(bytes) => bytes,
        None => return false,
    };
    if decoded.len() != 35 {
        return false;
    }
    if decoded[0] != ACCOUNT_ID_VERSION_BYTE {
        return false;
    }
    let checksum = u16::from_le_bytes([decoded[33], decoded[34]]);
    crc16_xmodem(&decoded[..33]) == checksum
}

fn crc16_xmodem(input: &[u8]) -> u16 {
    let mut crc = 0u16;
    for &byte in input {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            crc = if (crc & 0x8000) != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// Validates a raw SEP-38 quote response, returning a typed [`Sep38QuoteResponse`]
/// or [`Error::validation_error`] if any required field is missing or empty.
///
/// # Arguments
///
/// * `id` - Unique quote ID (must be non-empty).
/// * `expires_at` - Expiration timestamp as string (must be non-empty).
/// * `price` - Exchange price (must be non-empty).
/// * `sell_amount` - Amount to sell (must be non-empty).
/// * `buy_amount` - Amount to buy (must be non-empty).
/// * `fee` - Fee amount (must be non-empty).
///
/// # Returns
///
/// A validated [`Sep38QuoteResponse`] on success.
///
/// # Errors
///
/// Returns [`Error`] with code [`ErrorCode::ValidationError`] if any string
/// field is empty.
pub fn validate_sep38_quote_response(
    id: &str,
    expires_at: &str,
    price: &str,
    sell_amount: &str,
    buy_amount: &str,
    fee: &str,
) -> Result<Sep38QuoteResponse, Error> {
    if id.is_empty() {
        return Err(Error::validation_error("id is empty"));
    }
    if expires_at.is_empty() {
        return Err(Error::validation_error("expires_at is empty"));
    }
    if price.is_empty() {
        return Err(Error::validation_error("price is empty"));
    }
    if sell_amount.is_empty() {
        return Err(Error::validation_error("sell_amount is empty"));
    }
    if buy_amount.is_empty() {
        return Err(Error::validation_error("buy_amount is empty"));
    }
    if fee.is_empty() {
        return Err(Error::validation_error("fee is empty"));
    }

    Ok(Sep38QuoteResponse {
        id: alloc::string::String::from(id),
        expires_at: alloc::string::String::from(expires_at),
        price: alloc::string::String::from(price),
        sell_amount: alloc::string::String::from(sell_amount),
        buy_amount: alloc::string::String::from(buy_amount),
        fee: alloc::string::String::from(fee),
    })
}

/// Validates a raw anchor info response, returning a typed [`AnchorInfoResponse`]
/// or [`Error::validation_error`] if any required field is missing or empty.
///
/// # Arguments
///
/// * `name` - Human-readable anchor name (must be non-empty).
/// * `supported_assets` - List of asset codes the anchor supports (must be non-empty).
///
/// # Returns
///
/// A validated [`AnchorInfoResponse`] on success.
///
/// # Errors
///
/// Returns [`Error`] with code [`ErrorCode::ValidationError`] if `name` is empty
/// or `supported_assets` is an empty list.
///
/// # Examples
///
/// ```rust
/// use anchorkit::validate_anchor_info_response;
///
/// let resp = validate_anchor_info_response(
///     "MyAnchor",
///     vec!["USDC".into(), "XLM".into()],
/// ).unwrap();
/// assert_eq!(resp.name, "MyAnchor");
/// assert_eq!(resp.supported_assets.len(), 2);
///
/// assert!(validate_anchor_info_response("", vec!["USDC".into()]).is_err());
/// assert!(validate_anchor_info_response("MyAnchor", vec![]).is_err());
/// ```
pub fn validate_anchor_info_response(
    name: &str,
    supported_assets: alloc::vec::Vec<alloc::string::String>,
) -> Result<AnchorInfoResponse, Error> {
    if name.is_empty() {
        return Err(Error::validation_error("name is empty"));
    }
    if name.len() > 100 {
        return Err(Error::validation_error("name must be 100 characters or fewer"));
    }
    if supported_assets.is_empty() {
        return Err(Error::validation_error("supported_assets is empty"));
    }
    for asset in &supported_assets {
        validate_stellar_asset(asset.as_str())?;
    }

    Ok(AnchorInfoResponse {
        name: alloc::string::String::from(name),
        supported_assets,
    })
}

/// Validates a raw transaction status response, returning a typed [`TransactionStatusResponse`]
/// or [`Error::validation_error`] if any required field is missing or empty.
///
/// # Arguments
///
/// * `transaction_id` - Unique transaction ID (must be non-empty).
/// * `status` - Current transaction status string (must be non-empty).
/// * `kind` - The type of transaction (e.g., "deposit", "withdrawal"; must be non-empty).
///
/// # Returns
///
/// A validated [`TransactionStatusResponse`] on success.
///
/// # Errors
///
/// Returns [`Error`] with code [`ErrorCode::ValidationError`] if any field is empty.
#[allow(dead_code)]
pub fn validate_transaction_status_response(
    transaction_id: &str,
    status: &str,
    kind: &str,
) -> Result<TransactionStatusResponse, Error> {
    if transaction_id.is_empty() {
        return Err(Error::validation_error("transaction_id is empty"));
    }
    if status.is_empty() {
        return Err(Error::validation_error("status is empty"));
    }
    if kind.is_empty() {
        return Err(Error::validation_error("kind is empty"));
    }

    Ok(TransactionStatusResponse {
        transaction_id: alloc::string::String::from(transaction_id),
        status: alloc::string::String::from(status),
        kind: alloc::string::String::from(kind),
    })
}

fn is_valid_sep6_status(status: &str) -> bool {
    match status {
        "pending_external"
        | "pending_anchor"
        | "pending_trust"
        | "pending_user"
        | "pending_user_transfer_start"
        | "completed"
        | "refunded"
        | "expired"
        | "incomplete"
        | "pending"
        | "no_market"
        | "too_small"
        | "too_large"
        | "error" => true,
        _ => false,
    }
}

/// Validate a Stellar asset identifier.
///
/// Accepts:
/// - `"native"` (XLM)
/// - `"CODE:ISSUER"` where CODE is 1–12 alphanumeric chars and ISSUER is a
///   56-character Stellar address starting with `G`.
///
/// # Examples
///
/// ```rust
/// use anchorkit::validate_stellar_asset;
///
/// assert!(validate_stellar_asset("native").is_ok());
/// assert!(validate_stellar_asset("USDC:GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5").is_ok());
/// assert!(validate_stellar_asset("INVALID").is_err());
/// assert!(validate_stellar_asset("").is_err());
/// ```
pub fn validate_stellar_asset(asset: &str) -> Result<(), Error> {
    if asset == "native" {
        return Ok(());
    }
    let parts: alloc::vec::Vec<&str> = asset.splitn(2, ':').collect();
    if parts.len() != 2 {
        return Err(Error::validation_error("asset must be 'native' or 'CODE:ISSUER'"));
    }
    let code = parts[0];
    let issuer = parts[1];
    if code.is_empty() || code.len() > 12 || !code.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(Error::validation_error("asset code must be 1-12 alphanumeric characters"));
    }
    if issuer.len() != 56 || !issuer.starts_with('G') || !issuer.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(Error::validation_error("asset issuer must be a 56-character Stellar address starting with G"));
    }
    Ok(())
}

/// Normalize and validate a Stellar account ID.
///
/// Accepts address strings with leading/trailing whitespace and lower-case
/// letters, normalizing them to the canonical upper-case Stellar public key
/// form before returning.
pub fn normalize_stellar_account_id(account_id: &str) -> Result<alloc::string::String, Error> {
    let trimmed = account_id.trim();
    if trimmed.is_empty() {
        return Err(Error::validation_error("account_id is empty"));
    }
    if trimmed.chars().any(|c| c.is_ascii_whitespace()) {
        return Err(Error::validation_error("account_id must not contain whitespace"));
    }
    let normalized = trimmed.to_ascii_uppercase();
    if normalized.len() != 56 {
        return Err(Error::validation_error("account_id must be 56 characters"));
    }
    if !normalized.starts_with('G') {
        return Err(Error::validation_error("account_id must start with G"));
    }
    if !normalized.chars().all(is_valid_stellar_account_char) {
        return Err(Error::validation_error("account_id contains invalid characters"));
    }
    if !is_valid_stellar_strkey(&normalized) {
        return Err(Error::validation_error("account_id checksum is invalid"));
    }
    Ok(alloc::string::String::from(normalized))
}

/// Validate a Stellar account ID string.
pub fn validate_stellar_account_id(account_id: &str) -> Result<(), Error> {
    normalize_stellar_account_id(account_id).map(|_| ())
}

fn is_valid_quote_status(status: &str) -> bool {
    matches!(status, "quoted" | "pending" | "expired" | "error")
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- validate_deposit_response ---

    #[test]
    fn test_valid_deposit_response() {
        let result = validate_deposit_response("dep_123", "pending", "GDEPOSIT...", 2_000_000_000);
        assert!(result.is_ok());
        let r = result.unwrap();
        assert_eq!(r.transaction_id, "dep_123");
        assert_eq!(r.status, "pending");
        assert_eq!(r.deposit_address, "GDEPOSIT...");
        assert_eq!(r.expires_at, 2_000_000_000);
    }

    #[test]
    fn test_deposit_missing_transaction_id() {
        let result = validate_deposit_response("", "pending", "GDEPOSIT...", 2_000_000_000);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, crate::errors::ErrorCode::ValidationError);
    }

    #[test]
    fn test_deposit_missing_status() {
        let result = validate_deposit_response("dep_123", "", "GDEPOSIT...", 2_000_000_000);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, crate::errors::ErrorCode::ValidationError);
    }

    #[test]
    fn test_deposit_invalid_status() {
        let result = validate_deposit_response("dep_123", "garbage_status", "GDEPOSIT...", 2_000_000_000);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, crate::errors::ErrorCode::ValidationError);
    }

    #[test]
    fn test_deposit_missing_deposit_address() {
        let result = validate_deposit_response("dep_123", "pending", "", 2_000_000_000);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, crate::errors::ErrorCode::ValidationError);
    }

    #[test]
    fn test_deposit_zero_expires_at_is_valid() {
        // expires_at = 0 is a valid u64; only string fields are required
        let result = validate_deposit_response("dep_123", "pending", "GDEPOSIT...", 0);
        assert!(result.is_ok());
    }

    // --- validate_withdraw_response ---

    #[test]
    fn test_valid_withdraw_response() {
        let result = validate_withdraw_response("wd_456", "processing", 2000);
        assert!(result.is_ok());
        let r = result.unwrap();
        assert_eq!(r.transaction_id, "wd_456");
        assert_eq!(r.status, "processing");
        assert_eq!(r.estimated_completion, 2000);
    }

    #[test]
    fn test_withdraw_missing_transaction_id() {
        let result = validate_withdraw_response("", "processing", 2000);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, crate::errors::ErrorCode::ValidationError);
    }

    #[test]
    fn test_withdraw_missing_status() {
        let result = validate_withdraw_response("wd_456", "", 2000);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, crate::errors::ErrorCode::ValidationError);
    }

    // --- validate_quote_response ---

    #[test]
    fn test_valid_quote_response() {
        let result = validate_quote_response(
            "quote_789", "quoted", 100_0000000,
            "USDC:GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5",
            500000,
        );
        assert!(result.is_ok());
        let r = result.unwrap();
        assert_eq!(r.id, "quote_789");
        assert_eq!(r.status, "quoted");
        assert_eq!(r.amount, 100_0000000);
        assert_eq!(r.fee, 500000);
    }

    #[test]
    fn test_quote_missing_id() {
        let result = validate_quote_response("", "quoted", 100_0000000, "native", 500000);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, crate::errors::ErrorCode::ValidationError);
    }

    #[test]
    fn test_quote_missing_status() {
        let result = validate_quote_response("quote_789", "", 100_0000000, "native", 500000);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, crate::errors::ErrorCode::ValidationError);
    }

    #[test]
    fn test_quote_missing_asset() {
        let result = validate_quote_response("quote_789", "quoted", 100_0000000, "", 500000);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, crate::errors::ErrorCode::ValidationError);
    }

    #[test]
    fn test_quote_zero_amount_is_valid() {
        // amount = 0 is now rejected per #189 requirements
        let result = validate_quote_response("quote_789", "quoted", 0, "native", 0);
        assert!(result.is_err());
    }

    // --- validate_sep38_quote_response ---

    #[test]
    fn test_valid_sep38_quote_response() {
        let result = validate_sep38_quote_response(
            "quote_123",
            "2023-11-01T00:00:00Z",
            "1.05",
            "100.00",
            "105.00",
            "1.00",
        );
        assert!(result.is_ok());
        let r = result.unwrap();
        assert_eq!(r.id, "quote_123");
        assert_eq!(r.expires_at, "2023-11-01T00:00:00Z");
        assert_eq!(r.price, "1.05");
        assert_eq!(r.sell_amount, "100.00");
        assert_eq!(r.buy_amount, "105.00");
        assert_eq!(r.fee, "1.00");
    }

    #[test]
    fn test_sep38_quote_missing_id() {
        let result = validate_sep38_quote_response(
            "", "2023-11-01T00:00:00Z", "1.05", "100.00", "105.00", "1.00",
        );
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, crate::errors::ErrorCode::ValidationError);
    }

    #[test]
    fn test_sep38_quote_missing_expires_at() {
        let result = validate_sep38_quote_response(
            "quote_123", "", "1.05", "100.00", "105.00", "1.00",
        );
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, crate::errors::ErrorCode::ValidationError);
    }

    #[test]
    fn test_sep38_quote_missing_price() {
        let result = validate_sep38_quote_response(
            "quote_123", "2023-11-01T00:00:00Z", "", "100.00", "105.00", "1.00",
        );
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, crate::errors::ErrorCode::ValidationError);
    }

    #[test]
    fn test_sep38_quote_missing_sell_amount() {
        let result = validate_sep38_quote_response(
            "quote_123", "2023-11-01T00:00:00Z", "1.05", "", "105.00", "1.00",
        );
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, crate::errors::ErrorCode::ValidationError);
    }

    #[test]
    fn test_sep38_quote_missing_buy_amount() {
        let result = validate_sep38_quote_response(
            "quote_123", "2023-11-01T00:00:00Z", "1.05", "100.00", "", "1.00",
        );
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, crate::errors::ErrorCode::ValidationError);
    }

    #[test]
    fn test_sep38_quote_missing_fee() {
        let result = validate_sep38_quote_response(
            "quote_123", "2023-11-01T00:00:00Z", "1.05", "100.00", "105.00", "",
        );
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, crate::errors::ErrorCode::ValidationError);
    }

    // --- validate_anchor_info_response ---

    #[test]
    fn test_valid_anchor_info_response() {
        let assets = alloc::vec![
            alloc::string::String::from("native"),
            alloc::string::String::from("USDC:GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5"),
        ];
        let result = validate_anchor_info_response("MyAnchor", assets);
        assert!(result.is_ok());
        let r = result.unwrap();
        assert_eq!(r.name, "MyAnchor");
        assert_eq!(r.supported_assets.len(), 2);
    }

    #[test]
    fn test_anchor_info_missing_name() {
        let assets = alloc::vec![alloc::string::String::from("native")];
        let result = validate_anchor_info_response("", assets);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, crate::errors::ErrorCode::ValidationError);
    }

    #[test]
    fn test_anchor_info_empty_assets() {
        let result = validate_anchor_info_response("MyAnchor", alloc::vec![]);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, crate::errors::ErrorCode::ValidationError);
    }

    // --- validate_transaction_status_response ---

    #[test]
    fn test_valid_transaction_status_response() {
        let result = validate_transaction_status_response("tx_123", "completed", "deposit");
        assert!(result.is_ok());
        let r = result.unwrap();
        assert_eq!(r.transaction_id, "tx_123");
        assert_eq!(r.status, "completed");
        assert_eq!(r.kind, "deposit");
    }

    #[test]
    fn test_transaction_status_missing_fields() {
        assert!(validate_transaction_status_response("", "completed", "deposit").is_err());
        assert!(validate_transaction_status_response("tx_123", "", "deposit").is_err());
        assert!(validate_transaction_status_response("tx_123", "completed", "").is_err());
    }

    // --- SDK does not crash on validation error ---

    #[test]
    fn test_validation_error_does_not_panic() {
        // Simulates SDK consumer handling the error gracefully
        let result = validate_deposit_response("", "", "", 0);
        match result {
            Err(e) if e.code == crate::errors::ErrorCode::ValidationError => { /* handled, no crash */ }
            _ => panic!("Expected ValidationError"),
        }
    }

    // ── #189 validate_stellar_asset ──────────────────────────────────────────

    #[test]
    fn test_stellar_asset_native() {
        assert!(validate_stellar_asset("native").is_ok());
    }

    #[test]
    fn test_stellar_asset_valid_issued() {
        assert!(validate_stellar_asset(
            "USDC:GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5"
        ).is_ok());
    }

    #[test]
    fn test_stellar_asset_empty() {
        assert!(validate_stellar_asset("").is_err());
    }

    #[test]
    fn test_stellar_asset_no_colon() {
        assert!(validate_stellar_asset("USDC").is_err());
    }

    #[test]
    fn test_stellar_asset_code_too_long() {
        assert!(validate_stellar_asset(
            "TOOLONGCODE123:GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5"
        ).is_err());
    }

    #[test]
    fn test_stellar_asset_issuer_wrong_length() {
        assert!(validate_stellar_asset("USDC:GSHORT").is_err());
    }

    #[test]
    fn test_stellar_asset_issuer_wrong_prefix() {
        // 56 chars but starts with 'A' not 'G'
        assert!(validate_stellar_asset(
            "USDC:ABBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5"
        ).is_err());
    }

    #[test]
    fn test_stellar_account_id_valid() {
        assert!(normalize_stellar_account_id(
            "GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5"
        ).is_ok());
    }

    #[test]
    fn test_stellar_account_id_normalizes_lowercase_and_whitespace() {
        let normalized = normalize_stellar_account_id(
            "  gbbd47if6lwk7p7mdevscwr7dpuwv3ny3dtqevfl4nat4aqh3zllfla5  "
        ).unwrap();
        assert_eq!(normalized, "GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5");
    }

    #[test]
    fn test_stellar_account_id_wrong_length() {
        assert!(validate_stellar_account_id("GBBD47IF6LWK7P7MDEVS").is_err());
    }

    #[test]
    fn test_stellar_account_id_wrong_prefix() {
        assert!(validate_stellar_account_id(
            "ABBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5"
        ).is_err());
    }

    #[test]
    fn test_stellar_account_id_invalid_checksum() {
        assert!(validate_stellar_account_id(
            "GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA6"
        ).is_err());
    }

    #[test]
    fn test_stellar_account_id_invalid_character() {
        assert!(validate_stellar_account_id(
            "GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFL!5"
        ).is_err());
    }

    // ── #189 validate_anchor_info_response extended ──────────────────────────

    #[test]
    fn test_anchor_info_invalid_asset_identifier() {
        let assets = alloc::vec![alloc::string::String::from("NOTVALID")];
        let result = validate_anchor_info_response("MyAnchor", assets);
        assert!(result.is_err());
    }

    #[test]
    fn test_anchor_info_valid_native_asset() {
        let assets = alloc::vec![alloc::string::String::from("native")];
        let result = validate_anchor_info_response("MyAnchor", assets);
        assert!(result.is_ok());
    }

    #[test]
    fn test_anchor_info_name_too_long() {
        let name = "A".repeat(101);
        let assets = alloc::vec![alloc::string::String::from("native")];
        let result = validate_anchor_info_response(&name, assets);
        assert!(result.is_err());
    }

    #[test]
    fn test_anchor_info_name_max_length_ok() {
        let name = "A".repeat(100);
        let assets = alloc::vec![alloc::string::String::from("native")];
        let result = validate_anchor_info_response(&name, assets);
        assert!(result.is_ok());
    }

    // ── #189 validate_quote_response extended ────────────────────────────────

    #[test]
    fn test_quote_zero_amount_rejected() {
        let result = validate_quote_response("q1", "quoted", 0, "native", 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_quote_invalid_asset_rejected() {
        let result = validate_quote_response("q1", "quoted", 100, "BADFORMAT", 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_quote_invalid_status_rejected() {
        let result = validate_quote_response("q1", "unknown_status", 100, "native", 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_quote_valid_native_asset() {
        let result = validate_quote_response("q1", "quoted", 100, "native", 0);
        assert!(result.is_ok());
    }

    // ── #189 validate_deposit_response expires_at ────────────────────────────

    #[test]
    fn test_deposit_past_expires_at_rejected() {
        // 1_000_000 is well before Nov 2023 — treated as past
        let result = validate_deposit_response("dep_1", "pending", "GADDR...", 1_000_000);
        assert!(result.is_err());
    }

    #[test]
    fn test_deposit_future_expires_at_accepted() {
        // 2_000_000_000 is year ~2033
        let result = validate_deposit_response("dep_1", "pending", "GADDR...", 2_000_000_000);
        assert!(result.is_ok());
    }

    #[test]
    fn test_deposit_zero_expires_at_accepted() {
        // 0 means "no expiry"
        let result = validate_deposit_response("dep_1", "pending", "GADDR...", 0);
        assert!(result.is_ok());
    }
}