//! SEP-38 Anchor RFQ Service Layer
//!
//! Provides normalized service functions for fetching prices and requesting firm quotes
//! across different anchors.

extern crate alloc;
use alloc::string::String;

use crate::errors::Error;
use crate::errors::normalize_asset_code;

// ── Normalized response types ────────────────────────────────────────────────

/// Normalized price information from SEP-38 `/prices` endpoint.
///
/// # Examples
///
/// ```rust
/// use anchorkit::sep38::{fetch_prices, RawPrice};
///
/// let raw = RawPrice {
///     buy_asset: "USDC".into(),
///     sell_asset: "XLM".into(),
///     price: "0.15".into(),
/// };
/// let price = fetch_prices(raw).unwrap();
/// assert_eq!(price.buy_asset, "USDC");
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Price {
    pub buy_asset: String,
    pub sell_asset: String,
    pub price: String,
}

/// Normalized firm quote from SEP-38 `/quote` endpoint.
///
/// A firm quote is a binding commitment from the anchor to exchange assets at
/// the stated `price` until `expires_at`.
///
/// # Examples
///
/// ```rust
/// use anchorkit::sep38::{request_firm_quote, RawFirmQuote};
///
/// let raw = RawFirmQuote {
///     id: "quote-123".into(),
///     expires_at: "1700000000".into(),
///     price: "0.15".into(),
///     sell_amount: "1000".into(),
///     buy_amount: "150".into(),
///     sell_asset: "xlm".into(),
///     buy_asset: "usdc".into(),
/// };
/// let quote = request_firm_quote(raw, 0).unwrap();
/// assert_eq!(quote.id, "quote-123");
/// assert_eq!(quote.sell_asset, "XLM");
/// assert_eq!(quote.buy_asset, "USDC");
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FirmQuote {
    pub id: String,
    /// Unix timestamp (seconds) when this quote expires.
    pub expires_at: u64,
    pub price: String,
    pub sell_amount: String,
    pub buy_amount: String,
    /// Normalized (uppercase) asset code being sold.
    pub sell_asset: String,
    /// Normalized (uppercase) asset code being bought.
    pub buy_asset: String,
}

// ── Raw response types (from anchor APIs) ────────────────────────────────────

/// Raw price response from anchor /prices endpoint.
#[derive(Clone, Debug)]
pub struct RawPrice {
    pub buy_asset: String,
    pub sell_asset: String,
    pub price: String,
}

/// Raw quote response from anchor /quote endpoint.
#[derive(Clone, Debug)]
pub struct RawFirmQuote {
    pub id: String,
    /// Unix timestamp as a string (e.g. "1700000000").
    pub expires_at: String,
    pub price: String,
    pub sell_amount: String,
    pub buy_amount: String,
    /// Asset code being sold (e.g. `"XLM"`). Normalized to uppercase.
    pub sell_asset: String,
    /// Asset code being bought (e.g. `"USDC"`). Normalized to uppercase.
    pub buy_asset: String,
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Returns `true` if `price_str` is a non-empty, positive decimal string.
fn is_valid_positive_decimal(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    // Allow optional leading digits, optional single '.', trailing digits
    let mut has_digit = false;
    let mut dot_count = 0u32;
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            has_digit = true;
        } else if ch == '.' {
            dot_count += 1;
            if dot_count > 1 {
                return false;
            }
        } else {
            return false;
        }
    }
    if !has_digit {
        return false;
    }
    // Must be > 0: reject "0", "0.0", "0.00", etc.
    let v: f64 = s.parse().unwrap_or(0.0);
    v > 0.0
}

/// Validates all fields of a raw firm quote.
///
/// Returns `Err(Error::invalid_quote())` if any field is invalid.
/// Returns `Err(Error::stale_quote())` if `expires_at` is not in the future.
fn validate_quote_fields(raw: &RawFirmQuote, current_timestamp: u64) -> Result<u64, Error> {
    if raw.id.is_empty() {
        return Err(Error::invalid_quote());
    }
    let expires_at: u64 = raw.expires_at.parse().map_err(|_| Error::invalid_quote())?;
    if expires_at <= current_timestamp {
        return Err(Error::stale_quote());
    }
    if !is_valid_positive_decimal(&raw.price) {
        return Err(Error::invalid_quote());
    }
    if !is_valid_positive_decimal(&raw.sell_amount) {
        return Err(Error::invalid_quote());
    }
    if !is_valid_positive_decimal(&raw.buy_amount) {
        return Err(Error::invalid_quote());
    }
    Ok(expires_at)
}

// ── Service functions ────────────────────────────────────────────────────────

/// Normalizes a raw `/prices` response from an anchor.
///
/// Extracts and passes through `buy_asset`, `sell_asset`, and `price` fields.
/// Currently performs no field-level validation; all fields are accepted as-is.
///
/// # Arguments
///
/// * `raw` - A [`RawPrice`] populated from the anchor's `/prices` endpoint.
///
/// # Returns
///
/// A normalised [`Price`] on success.
///
/// # Errors
///
/// Currently always returns `Ok(...)`. Future versions may validate that
/// `price` is a valid decimal string.
///
/// # Examples
///
/// ```rust
/// use anchorkit::sep38::{fetch_prices, RawPrice};
///
/// Returns `Err(Error::invalid_quote())` if `price` is not a positive decimal string.
pub fn fetch_prices(raw: RawPrice) -> Result<Price, Error> {
    if !is_valid_positive_decimal(&raw.price) {
        return Err(Error::invalid_quote());
    }
    Ok(Price {
        buy_asset: normalize_asset_code(&raw.buy_asset)?,
        sell_asset: normalize_asset_code(&raw.sell_asset)?,
        price: raw.price,
    })
}

/// Normalizes a raw `/quote` response from an anchor.
///
/// Validates all fields and checks expiry against `current_timestamp`.
/// Returns `Err(Error::stale_quote())` if the quote has already expired.
/// Returns `Err(Error::invalid_quote())` if any field is malformed or zero.
pub fn request_firm_quote(raw: RawFirmQuote, current_timestamp: u64) -> Result<FirmQuote, Error> {
    let expires_at = validate_quote_fields(&raw, current_timestamp)?;
    Ok(FirmQuote {
        id: raw.id,
        expires_at,
        price: raw.price,
        sell_amount: raw.sell_amount,
        buy_amount: raw.buy_amount,
        sell_asset: normalize_asset_code(&raw.sell_asset)?,
        buy_asset: normalize_asset_code(&raw.buy_asset)?,
    })
}

/// Checks if a quote has expired based on the provided timestamp.
///
/// Returns `true` if `expires_at <= current_timestamp`.
pub fn is_quote_expired(quote: &FirmQuote, current_timestamp: u64) -> bool {
    quote.expires_at <= current_timestamp
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    fn valid_raw(expires_at: &str) -> RawFirmQuote {
        RawFirmQuote {
            id: "quote-123".to_string(),
            expires_at: expires_at.to_string(),
            price: "0.15".to_string(),
            sell_amount: "1000".to_string(),
            buy_amount: "150".to_string(),
            sell_asset: "XLM".to_string(),
            buy_asset: "USDC".to_string(),
        }
    }

    // ── fetch_prices ─────────────────────────────────────────────────────────

    #[test]
    fn test_fetch_prices_valid() {
        let raw = RawPrice {
            buy_asset: "USDC".to_string(),
            sell_asset: "XLM".to_string(),
            price: "0.15".to_string(),
        };
        let result = fetch_prices(raw).unwrap();
        assert_eq!(result.buy_asset, "USDC");
        assert_eq!(result.sell_asset, "XLM");
        assert_eq!(result.price, "0.15");
    }

    #[test]
    fn test_fetch_prices_empty_price_rejected() {
        let raw = RawPrice {
            buy_asset: "USDC".to_string(),
            sell_asset: "XLM".to_string(),
            price: "".to_string(),
        };
        assert!(fetch_prices(raw).is_err());
    }

    #[test]
    fn test_fetch_prices_zero_price_rejected() {
        let raw = RawPrice {
            buy_asset: "USDC".to_string(),
            sell_asset: "XLM".to_string(),
            price: "0.0".to_string(),
        };
        assert!(fetch_prices(raw).is_err());
    }

    #[test]
    fn test_fetch_prices_malformed_price_rejected() {
        let raw = RawPrice {
            buy_asset: "USDC".to_string(),
            sell_asset: "XLM".to_string(),
            price: "abc".to_string(),
        };
        assert!(fetch_prices(raw).is_err());
    }

    // ── request_firm_quote ───────────────────────────────────────────────────

    #[test]
    fn test_request_firm_quote_valid() {
        let raw = valid_raw("2000");
        let result = request_firm_quote(raw, 1000).unwrap();
        assert_eq!(result.id, "quote-123");
        assert_eq!(result.expires_at, 2000u64);
        assert_eq!(result.price, "0.15");
    }

    #[test]
    fn test_expired_quote_rejected() {
        // expires_at=1000, now=2000 → stale
        let raw = valid_raw("1000");
        let err = request_firm_quote(raw, 2000).unwrap_err();
        assert_eq!(err.code, crate::errors::ErrorCode::StaleQuote);
    }

    #[test]
    fn test_quote_at_exact_expiry_rejected() {
        // expires_at == now → stale
        let raw = valid_raw("1500");
        let err = request_firm_quote(raw, 1500).unwrap_err();
        assert_eq!(err.code, crate::errors::ErrorCode::StaleQuote);
    }

    #[test]
    fn test_empty_id_rejected() {
        let mut raw = valid_raw("2000");
        raw.id = "".to_string();
        assert!(request_firm_quote(raw, 1000).is_err());
    }

    #[test]
    fn test_malformed_price_rejected() {
        let mut raw = valid_raw("2000");
        raw.price = "not-a-number".to_string();
        let err = request_firm_quote(raw, 1000).unwrap_err();
        assert_eq!(err.code, crate::errors::ErrorCode::InvalidQuote);
    }

    #[test]
    fn test_zero_sell_amount_rejected() {
        let mut raw = valid_raw("2000");
        raw.sell_amount = "0".to_string();
        let err = request_firm_quote(raw, 1000).unwrap_err();
        assert_eq!(err.code, crate::errors::ErrorCode::InvalidQuote);
    }

    #[test]
    fn test_zero_buy_amount_rejected() {
        let mut raw = valid_raw("2000");
        raw.buy_amount = "0".to_string();
        let err = request_firm_quote(raw, 1000).unwrap_err();
        assert_eq!(err.code, crate::errors::ErrorCode::InvalidQuote);
    }

    #[test]
    fn test_malformed_expires_at_rejected() {
        let mut raw = valid_raw("not-a-timestamp");
        raw.expires_at = "not-a-timestamp".to_string();
        let err = request_firm_quote(raw, 1000).unwrap_err();
        assert_eq!(err.code, crate::errors::ErrorCode::InvalidQuote);
    }

    // ── is_quote_expired ─────────────────────────────────────────────────────

    #[test]
    fn test_is_quote_expired_true() {
        let quote = FirmQuote {
            id: "q".to_string(),
            expires_at: 1000,
            price: "0.15".to_string(),
            sell_amount: "1000".to_string(),
            buy_amount: "150".to_string(),
            sell_asset: "XLM".to_string(),
            buy_asset: "USDC".to_string(),
        };
        assert!(is_quote_expired(&quote, 2000));
    }

    #[test]
    fn test_is_quote_expired_false() {
        let quote = FirmQuote {
            id: "q".to_string(),
            expires_at: 2000,
            price: "0.15".to_string(),
            sell_amount: "1000".to_string(),
            buy_amount: "150".to_string(),
            sell_asset: "XLM".to_string(),
            buy_asset: "USDC".to_string(),
        };
        assert!(!is_quote_expired(&quote, 1000));
    }

    #[test]
    fn test_is_quote_expired_at_boundary() {
        let quote = FirmQuote {
            id: "q".to_string(),
            expires_at: 1500,
            price: "0.15".to_string(),
            sell_amount: "1000".to_string(),
            buy_amount: "150".to_string(),
            sell_asset: "XLM".to_string(),
            buy_asset: "USDC".to_string(),
        };
        assert!(is_quote_expired(&quote, 1500));
    }

    // ── asset code normalization ──────────────────────────────────────────────

    #[test]
    fn test_fetch_prices_normalizes_lowercase_codes() {
        let raw = RawPrice {
            buy_asset: "usdc".to_string(),
            sell_asset: "xlm".to_string(),
            price: "0.15".to_string(),
        };
        let result = fetch_prices(raw).unwrap();
        assert_eq!(result.buy_asset, "USDC");
        assert_eq!(result.sell_asset, "XLM");
    }

    #[test]
    fn test_fetch_prices_invalid_buy_asset_rejected() {
        let raw = RawPrice {
            buy_asset: "BAD CODE".to_string(),
            sell_asset: "XLM".to_string(),
            price: "0.15".to_string(),
        };
        let err = fetch_prices(raw).unwrap_err();
        assert_eq!(err.code, crate::errors::ErrorCode::InvalidAssetCode);
    }

    #[test]
    fn test_request_firm_quote_normalizes_asset_codes() {
        let mut raw = valid_raw("2000");
        raw.sell_asset = "xlm".to_string();
        raw.buy_asset = "usdc".to_string();
        let result = request_firm_quote(raw, 1000).unwrap();
        assert_eq!(result.sell_asset, "XLM");
        assert_eq!(result.buy_asset, "USDC");
    }

    #[test]
    fn test_request_firm_quote_invalid_sell_asset_rejected() {
        let mut raw = valid_raw("2000");
        raw.sell_asset = "TOOLONGCODE13".to_string();
        let err = request_firm_quote(raw, 1000).unwrap_err();
        assert_eq!(err.code, crate::errors::ErrorCode::InvalidAssetCode);
    }
}
