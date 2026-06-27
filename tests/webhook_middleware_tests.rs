#![cfg(test)]

mod webhook_middleware_tests {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    use anchorkit::{
        errors::ErrorCode,
        retry::RetryConfig,
        webhook::{deliver_webhook, get_dead_letter_webhooks, DlqEntry, WebhookDeliveryConfig},
    };

    fn config(max_retries: u32) -> WebhookDeliveryConfig {
        WebhookDeliveryConfig {
            endpoint_url: "https://example.com/hook".into(),
            max_retries,
            retry_delay_ms: 0,
            timeout_ms: 1000,
            retry_config: RetryConfig::new(max_retries, 0, 0, 1),
            dead_letter_storage_key: "test_dlq".into(),
        }
    }

    // -----------------------------------------------------------------------
    // 1. Immediate success — no retries triggered
    // -----------------------------------------------------------------------
    #[test]
    fn test_immediate_success_no_retries() {
        let call_count = Arc::new(Mutex::new(0u32));
        let cc = call_count.clone();

        let mut dlq: BTreeMap<String, Vec<DlqEntry>> = BTreeMap::new();
        let result = deliver_webhook(
            &config(3),
            r#"{"event":"deposit"}"#,
            &mut dlq,
            move |_url, _body| {
                *cc.lock().unwrap() += 1;
                Ok(200)
            },
            |_| {},
            || 1_000_000u64,
        );

        assert!(result.is_ok());
        assert_eq!(*call_count.lock().unwrap(), 1, "should call HTTP exactly once");
        assert!(dlq.is_empty(), "DLQ must be empty on success");
    }

    // -----------------------------------------------------------------------
    // 2. Two 503s then 200 — succeeds on 3rd attempt
    // -----------------------------------------------------------------------
    #[test]
    fn test_success_after_two_failures() {
        let call_count = Arc::new(Mutex::new(0u32));
        let cc = call_count.clone();

        let mut dlq: BTreeMap<String, Vec<DlqEntry>> = BTreeMap::new();
        let result = deliver_webhook(
            &config(3),
            r#"{"event":"withdrawal"}"#,
            &mut dlq,
            move |_url, _body| {
                let mut n = cc.lock().unwrap();
                *n += 1;
                if *n < 3 { Ok(503) } else { Ok(200) }
            },
            |_| {},
            || 1_000_000u64,
        );

        assert!(result.is_ok(), "expected success on 3rd attempt, got {:?}", result);
        assert_eq!(*call_count.lock().unwrap(), 3);
        assert!(
            dlq.is_empty(),
            "DLQ must be empty when delivery eventually succeeds"
        );
    }

    // -----------------------------------------------------------------------
    // 3. All retries exhausted — payload lands in DLQ
    // -----------------------------------------------------------------------
    #[test]
    fn test_exhausted_retries_writes_to_dlq() {
        let payload = r#"{"event":"kyc_failed"}"#;
        let mut dlq: BTreeMap<String, Vec<DlqEntry>> = BTreeMap::new();

        let result = deliver_webhook(
            &config(3),
            payload,
            &mut dlq,
            |_url, _body| Ok(503u16),
            |_| {},
            || 1_000_000u64,
        );

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, ErrorCode::WebhookDeliveryFailed);
        // context must record attempts_made
        let ctx = err.context.expect("context must be set");
        assert!(ctx.contains("attempts_made=3"), "context: {ctx}");

        // Payload must be in the DLQ
        let entries = get_dead_letter_webhooks(&dlq, "test_dlq");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].payload, payload);
        assert_eq!(entries[0].attempts_made, 3);
        assert_eq!(entries[0].last_status_code, 503);
    }

    // -----------------------------------------------------------------------
    // 4. Admin inspection — get_dead_letter_webhooks returns all failed payloads
    // -----------------------------------------------------------------------
    #[test]
    fn test_admin_can_inspect_dlq() {
        let mut dlq: BTreeMap<String, Vec<DlqEntry>> = BTreeMap::new();
        let payloads = [r#"{"event":"a"}"#, r#"{"event":"b"}"#];

        for p in &payloads {
            let _ = deliver_webhook(
                &config(1), // 1 attempt → immediate DLQ on failure
                p,
                &mut dlq,
                |_url, _body| Ok(500u16),
                |_| {},
                || 1_000_000u64,
            );
        }

        let entries = get_dead_letter_webhooks(&dlq, "test_dlq");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].payload, payloads[0]);
        assert_eq!(entries[1].payload, payloads[1]);

        // Unknown key returns empty slice
        assert!(get_dead_letter_webhooks(&dlq, "no_such_key").is_empty());
    }
}
