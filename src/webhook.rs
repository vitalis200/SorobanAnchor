//! Webhook delivery with exponential backoff and a Dead Letter Queue (DLQ).
//!
//! `deliver_webhook` wraps the HTTP POST in `retry_with_backoff`.  On total
//! exhaustion a structured [`DlqEntry`] is written into the caller-supplied DLQ
//! map under `dead_letter_storage_key`.  `get_dead_letter_webhooks` and
//! `query_dlq` let admins inspect those failed entries.

#[cfg(feature = "std")]
extern crate std;

extern crate alloc;

use alloc::{
    collections::BTreeMap,
    format,
    string::{String, ToString},
};
use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefCell;

use crate::{
    errors::{AnchorKitError, ErrorCode},
    retry::{retry_with_backoff, RetryConfig},
};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for a single webhook endpoint.
#[derive(Clone, Debug)]
pub struct WebhookDeliveryConfig {
    /// Target URL for the HTTP POST.
    pub endpoint_url: String,
    /// Maximum delivery attempts (passed straight to `RetryConfig::max_attempts`).
    pub max_retries: u32,
    /// Base delay between retries in milliseconds.
    pub retry_delay_ms: u64,
    /// Per-attempt timeout in milliseconds (informational; enforced by `http_post`).
    pub timeout_ms: u64,
    /// Backoff parameters (base delay, multiplier, cap).
    pub retry_config: RetryConfig,
    /// Key under which failed entries are stored in the DLQ map.
    pub dead_letter_storage_key: String,
}

// ---------------------------------------------------------------------------
// DLQ entry
// ---------------------------------------------------------------------------

/// Structured record stored in the DLQ when all delivery attempts are exhausted.
#[derive(Clone, Debug, PartialEq)]
pub struct DlqEntry {
    /// The payload that failed to deliver.
    pub payload: String,
    /// Unix timestamp (seconds) when the entry was written to the DLQ.
    pub failed_at_timestamp: u64,
    /// Last HTTP status code received, or 0 if the transport failed entirely.
    pub last_status_code: u16,
    /// Number of delivery attempts made before giving up.
    pub attempts_made: u32,
    /// Human-readable description of the last error.
    pub last_error: String,
}

// ---------------------------------------------------------------------------
// Delivery
// ---------------------------------------------------------------------------

/// Attempt to POST `payload` to `config.endpoint_url` with exponential backoff.
///
/// `http_post` is an injectable transport function `(url, body) -> Result<u16, String>`
/// that returns the HTTP status code on success or an error string on failure.
///
/// `sleep_fn` is called with the computed delay (ms) between retries.
///
/// `now_fn` returns the current Unix timestamp in seconds (used to timestamp DLQ entries).
///
/// On total failure a [`DlqEntry`] is appended to `dlq` under
/// `config.dead_letter_storage_key` and an `AnchorKitError` is returned.
pub fn deliver_webhook<H, S, T>(
    config: &WebhookDeliveryConfig,
    payload: &str,
    dlq: &mut BTreeMap<String, Vec<DlqEntry>>,
    http_post: H,
    mut sleep_fn: S,
    now_fn: T,
) -> Result<(), AnchorKitError>
where
    H: Fn(&str, &str) -> Result<u16, String>,
    S: FnMut(u64),
    T: Fn() -> u64,
{
    let mut retry_cfg = config.retry_config.clone();
    retry_cfg.max_attempts = config.max_retries;

    let last_error_msg: RefCell<String> = RefCell::new(String::new());
    let last_status: RefCell<u16> = RefCell::new(0);

    let mut jitter_source = crate::retry::LedgerJitterSource::new(0, now_fn());
    let result = retry_with_backoff(
        &retry_cfg,
        |attempt| {
            let (status, msg) = match http_post(&config.endpoint_url, payload) {
                Ok(s) if s < 400 => return Ok(()),
                Ok(s) => (s, format!("HTTP {s}")),
                Err(e) => (0, e),
            };
            #[cfg(feature = "std")]
            std::eprintln!(
                "[webhook] attempt={} status={} error=\"{}\"",
                attempt + 1,
                status,
                msg
            );
            *last_error_msg.borrow_mut() = msg.clone();
            *last_status.borrow_mut() = status;
            Err(msg)
        },
        |_e: &String| true,
        &mut sleep_fn,
        &mut jitter_source,
    );

    match result {
        Ok(()) => Ok(()),
        Err(e) => {
            let last = last_error_msg.into_inner();
            let status = last_status.into_inner();
            let entry = DlqEntry {
                payload: payload.to_string(),
                failed_at_timestamp: now_fn(),
                last_status_code: status,
                attempts_made: config.max_retries,
                last_error: last.clone(),
            };
            dlq.entry(config.dead_letter_storage_key.clone())
                .or_default()
                .push(entry);

            Err(AnchorKitError::with_context(
                ErrorCode::WebhookDeliveryFailed,
                &format!(
                    "Webhook delivery failed after {} attempt(s): {}",
                    config.max_retries, e
                ),
                &format!("attempts_made={} last_status={} last_error={}", config.max_retries, status, last),
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// DLQ inspection
// ---------------------------------------------------------------------------

/// Return all [`DlqEntry`] records stored under `key` in the DLQ, or an empty slice.
pub fn get_dead_letter_webhooks<'a>(
    dlq: &'a BTreeMap<String, Vec<DlqEntry>>,
    key: &str,
) -> &'a [DlqEntry] {
    dlq.get(key).map(Vec::as_slice).unwrap_or(&[])
}

/// Query DLQ entries filtered by minimum HTTP status code and time range.
///
/// Returns entries where `last_status_code >= min_status` (use 0 to match all)
/// and `failed_at_timestamp` is within `[from_ts, to_ts]` (inclusive).
/// Pass `to_ts = u64::MAX` to match all entries up to the present.
pub fn query_dlq<'a>(
    dlq: &'a BTreeMap<String, Vec<DlqEntry>>,
    key: &str,
    min_status: u16,
    from_ts: u64,
    to_ts: u64,
) -> Vec<&'a DlqEntry> {
    dlq.get(key)
        .map(|entries| {
            entries
                .iter()
                .filter(|e| {
                    e.last_status_code >= min_status
                        && e.failed_at_timestamp >= from_ts
                        && e.failed_at_timestamp <= to_ts
                })
                .collect()
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use alloc::collections::BTreeMap;

    fn make_config(max_retries: u32) -> WebhookDeliveryConfig {
        WebhookDeliveryConfig {
            endpoint_url: "https://example.com/hook".to_string(),
            max_retries,
            retry_delay_ms: 10,
            timeout_ms: 1000,
            retry_config: RetryConfig {
                max_attempts: max_retries,
                base_delay_ms: 1,
                backoff_multiplier: 1,
                max_delay_ms: 10,
            },

            dead_letter_storage_key: "test-key".to_string(),
        }
    }

    #[test]
    fn deliver_succeeds_on_first_attempt() {
        let mut dlq: BTreeMap<String, Vec<DlqEntry>> = BTreeMap::new();
        let result = deliver_webhook(
            &make_config(3),
            "payload",
            &mut dlq,
            |_, _| Ok(200),
            |_| {},
            || 1000,
        );
        assert!(result.is_ok());
        assert!(dlq.is_empty());
    }

    #[test]
    fn deliver_stores_dlq_entry_after_exhaustion() {
        let mut dlq: BTreeMap<String, Vec<DlqEntry>> = BTreeMap::new();
        let result = deliver_webhook(
            &make_config(2),
            "my-payload",
            &mut dlq,
            |_, _| Ok(503),
            |_| {},
            || 9999,
        );
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, ErrorCode::WebhookDeliveryFailed);

        let entries = get_dead_letter_webhooks(&dlq, "test-key");
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.payload, "my-payload");
        assert_eq!(entry.last_status_code, 503);
        assert_eq!(entry.attempts_made, 2);
        assert_eq!(entry.failed_at_timestamp, 9999);
        assert!(!entry.last_error.is_empty());
    }

    #[test]
    fn deliver_stores_dlq_entry_on_transport_error() {
        let mut dlq: BTreeMap<String, Vec<DlqEntry>> = BTreeMap::new();
        let result = deliver_webhook(
            &make_config(1),
            "payload",
            &mut dlq,
            |_, _| Err("connection refused".to_string()),
            |_| {},
            || 42,
        );
        assert!(result.is_err());
        let entries = get_dead_letter_webhooks(&dlq, "test-key");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].last_status_code, 0); // transport failure
        assert_eq!(entries[0].attempts_made, 1);
    }

    #[test]
    fn multiple_failures_accumulate_in_dlq() {
        let mut dlq: BTreeMap<String, Vec<DlqEntry>> = BTreeMap::new();
        let config = make_config(1);
        for i in 0..3u64 {
            let _ = deliver_webhook(
                &config,
                &alloc::format!("payload-{}", i),
                &mut dlq,
                |_, _| Ok(500),
                |_| {},
                move || i * 100,
            );
        }
        let entries = get_dead_letter_webhooks(&dlq, "test-key");
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn query_dlq_filters_by_status_and_time() {
        let mut dlq: BTreeMap<String, Vec<DlqEntry>> = BTreeMap::new();
        let key = "test-key";
        dlq.entry(key.to_string()).or_default().extend([
            DlqEntry { payload: "a".to_string(), failed_at_timestamp: 100, last_status_code: 500, attempts_made: 1, last_error: "e".to_string() },
            DlqEntry { payload: "b".to_string(), failed_at_timestamp: 200, last_status_code: 503, attempts_made: 1, last_error: "e".to_string() },
            DlqEntry { payload: "c".to_string(), failed_at_timestamp: 300, last_status_code: 0,   attempts_made: 1, last_error: "e".to_string() },
        ]);

        // All entries
        assert_eq!(query_dlq(&dlq, key, 0, 0, u64::MAX).len(), 3);
        // Only 5xx
        assert_eq!(query_dlq(&dlq, key, 500, 0, u64::MAX).len(), 2);
        // Time range
        assert_eq!(query_dlq(&dlq, key, 0, 150, 250).len(), 1);
        // No match
        assert_eq!(query_dlq(&dlq, key, 0, 400, 500).len(), 0);
    }
}
