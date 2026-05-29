/// Transaction State Tracker Tests
/// This test file demonstrates and validates the Transaction State Tracker implementation

use crate::transaction_state_tracker::*;
use soroban_sdk::Env;

#[cfg(test)]
mod transaction_state_tracker_tests {
    use super::*;
    use soroban_sdk::testutils::Address;
    use soroban_sdk::String;

    #[test]
    fn test_transaction_state_to_string() {
        assert_eq!(TransactionState::Pending.as_str(), "pending");
        assert_eq!(TransactionState::InProgress.as_str(), "in_progress");
        assert_eq!(TransactionState::Completed.as_str(), "completed");
        assert_eq!(TransactionState::Failed.as_str(), "failed");
    }

    #[test]
    fn test_transaction_state_from_string() {
        assert_eq!(
            TransactionState::from_str("pending"),
            Some(TransactionState::Pending)
        );
        assert_eq!(
            TransactionState::from_str("in_progress"),
            Some(TransactionState::InProgress)
        );
        assert_eq!(
            TransactionState::from_str("completed"),
            Some(TransactionState::Completed)
        );
        assert_eq!(
            TransactionState::from_str("failed"),
            Some(TransactionState::Failed)
        );
        assert_eq!(TransactionState::from_str("unknown"), None);
    }

    #[test]
    fn test_full_transaction_lifecycle() {
        let env = Env::default();
        let mut tracker = TransactionStateTracker::new(true);
        let initiator = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);

        // Create transaction -> Pending
        let tx1_result = tracker.create_transaction(1, initiator.clone(), &env);
        assert!(tx1_result.is_ok());
        let tx1 = tx1_result.unwrap();
        assert_eq!(tx1.state, TransactionState::Pending);

        // Start transaction -> In-progress
        let tx2_result = tracker.start_transaction(1, &env);
        assert!(tx2_result.is_ok());
        let tx2 = tx2_result.unwrap();
        assert_eq!(tx2.state, TransactionState::InProgress);

        // Complete transaction -> Completed
        let tx3_result = tracker.complete_transaction(1, &env);
        assert!(tx3_result.is_ok());
        let tx3 = tx3_result.unwrap();
        assert_eq!(tx3.state, TransactionState::Completed);
    }

    #[test]
    fn test_transaction_failure_with_error_message() {
        let env = Env::default();
        let mut tracker = TransactionStateTracker::new(true);
        let initiator = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);

        tracker.create_transaction(1, initiator.clone(), &env).ok();
        tracker.start_transaction(1, &env).ok();

        let error_msg = String::from_str(&env, "Payment declined");
        let result = tracker.fail_transaction(1, error_msg.clone(), &env);

        assert!(result.is_ok());
        let record = result.unwrap();
        assert_eq!(record.state, TransactionState::Failed);
        assert_eq!(record.error_message, Some(error_msg));
    }

    #[test]
    fn test_query_transactions_by_state() {
        let env = Env::default();
        let mut tracker = TransactionStateTracker::new(true);
        let initiator = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);

        // Create 5 transactions
        for i in 1..=5 {
            tracker.create_transaction(i, initiator.clone(), &env).ok();
        }

        // Move some to in-progress
        tracker.start_transaction(1, &env).ok();
        tracker.start_transaction(2, &env).ok();

        // Complete one
        tracker.complete_transaction(2, &env).ok();

        // Query by state
        let pending_result = tracker.get_transactions_by_state(TransactionState::Pending);
        assert!(pending_result.is_ok());
        let pending = pending_result.unwrap();
        assert_eq!(pending.len(), 3); // 3, 4, 5

        let in_progress_result = tracker.get_transactions_by_state(TransactionState::InProgress);
        assert!(in_progress_result.is_ok());
        let in_progress = in_progress_result.unwrap();
        assert_eq!(in_progress.len(), 1); // 1

        let completed_result = tracker.get_transactions_by_state(TransactionState::Completed);
        assert!(completed_result.is_ok());
        let completed = completed_result.unwrap();
        assert_eq!(completed.len(), 1); // 2
    }

    #[test]
    fn test_production_mode_flag() {
        let env = Env::default();
        let mut prod_tracker = TransactionStateTracker::new(false);
        let initiator = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);

        // In production mode, cache should not be populated
        let result = prod_tracker.create_transaction(1, initiator.clone(), &env);
        assert!(result.is_ok());
        assert_eq!(prod_tracker.cache_size(), 0); // Should be 0 in production mode

        // In dev mode, cache should be populated
        let mut dev_tracker = TransactionStateTracker::new(true);
        let result = dev_tracker.create_transaction(1, initiator.clone(), &env);
        assert!(result.is_ok());
        assert_eq!(dev_tracker.cache_size(), 1); // Should be 1 in dev mode
    }

    #[test]
    fn test_transaction_not_found() {
        let env = Env::default();
        let tracker = TransactionStateTracker::new(true);

        let result = tracker.get_transaction_state(999, &env);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), None);
    }

    #[test]
    fn test_multiple_transactions_isolation() {
        let env = Env::default();
        let mut tracker = TransactionStateTracker::new(true);
        let initiator1 = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        let initiator2 = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);

        // Create transactions from different initiators
        tracker.create_transaction(1, initiator1.clone(), &env).ok();
        tracker.create_transaction(2, initiator2.clone(), &env).ok();

        // Update first one
        tracker.start_transaction(1, &env).ok();

        // Verify second one is still pending
        let tx2_state = tracker.get_transaction_state(2, &env);
        assert!(tx2_state.is_ok());
        let tx2 = tx2_state.unwrap().unwrap();
        assert_eq!(tx2.state, TransactionState::Pending);
        assert_eq!(tx2.initiator, initiator2);
    }

    #[test]
    fn test_clear_cache_dev_mode() {
        let env = Env::default();
        let mut tracker = TransactionStateTracker::new(true);
        let initiator = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);

        tracker.create_transaction(1, initiator.clone(), &env).ok();
        tracker.create_transaction(2, initiator.clone(), &env).ok();
        assert_eq!(tracker.cache_size(), 2);

        let clear_result = tracker.clear_cache(&env);
        assert!(clear_result.is_ok());
        assert_eq!(tracker.cache_size(), 0);
    }

    #[test]
    fn test_timestamp_tracking() {
        let env = Env::default();
        let mut tracker = TransactionStateTracker::new(true);
        let initiator = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);

        let create_result = tracker.create_transaction(1, initiator.clone(), &env);
        let record1 = create_result.unwrap();
        let initial_timestamp = record1.timestamp;

        let update_result = tracker.start_transaction(1, &env);
        let record2 = update_result.unwrap();

        // Timestamps should be set and last_updated should reflect the change
        assert_eq!(record2.timestamp, initial_timestamp);
        assert!(record2.last_updated >= initial_timestamp);
    }

    // -----------------------------------------------------------------------
    // Backward / same-state transition guard
    // -----------------------------------------------------------------------

    #[test]
    fn test_illegal_backward_transition_rejected() {
        let env = Env::default();
        let mut tracker = TransactionStateTracker::new(true);
        let initiator = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);

        tracker.create_transaction(1, initiator.clone(), &env).ok();
        tracker.start_transaction(1, &env).ok();
        tracker.complete_transaction(1, &env).ok();

        // Completed → Pending is illegal
        let r = tracker.advance_transaction_state(1, TransactionState::Pending, &env);
        assert!(r.is_err());

        // Completed → InProgress is illegal
        let r = tracker.advance_transaction_state(1, TransactionState::InProgress, &env);
        assert!(r.is_err());
    }

    #[test]
    fn test_illegal_same_state_transition_rejected() {
        let env = Env::default();
        let mut tracker = TransactionStateTracker::new(true);
        let initiator = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);

        tracker.create_transaction(1, initiator.clone(), &env).ok();

        // Pending → Pending is not a valid transition
        let r = tracker.advance_transaction_state(1, TransactionState::Pending, &env);
        assert!(r.is_err());
    }

    #[test]
    fn test_valid_forward_transition_accepted() {
        let env = Env::default();
        let mut tracker = TransactionStateTracker::new(true);
        let initiator = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);

        tracker.create_transaction(1, initiator.clone(), &env).ok();

        let r = tracker.advance_transaction_state(1, TransactionState::InProgress, &env);
        assert!(r.is_ok());
        assert_eq!(r.unwrap().state, TransactionState::InProgress);

        let r = tracker.advance_transaction_state(1, TransactionState::Completed, &env);
        assert!(r.is_ok());
        assert_eq!(r.unwrap().state, TransactionState::Completed);
    }

    // -----------------------------------------------------------------------
    // Audit log entries for success and failure
    // -----------------------------------------------------------------------

    #[test]
    fn test_audit_log_entry_created_on_successful_transition() {
        let env = Env::default();
        let mut tracker = TransactionStateTracker::new(true);
        let initiator = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);

        tracker.create_transaction(1, initiator.clone(), &env).ok();
        tracker.start_transaction(1, &env).ok();

        assert_eq!(tracker.audit_log.len(), 1);
        let entry = &tracker.audit_log[0];
        assert_eq!(entry.from_state, TransactionState::Pending);
        assert_eq!(entry.to_state, TransactionState::InProgress);
        assert!(entry.success);
    }

    #[test]
    fn test_audit_log_entry_created_on_failed_transition() {
        let env = Env::default();
        let mut tracker = TransactionStateTracker::new(true);
        let initiator = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);

        tracker.create_transaction(1, initiator.clone(), &env).ok();
        // Illegal: Pending → Completed
        let _ = tracker.advance_transaction_state(1, TransactionState::Completed, &env);

        assert_eq!(tracker.audit_log.len(), 1);
        let entry = &tracker.audit_log[0];
        assert_eq!(entry.from_state, TransactionState::Pending);
        assert_eq!(entry.to_state, TransactionState::Completed);
        assert!(!entry.success);
    }

    // -----------------------------------------------------------------------
    // Invalid transition rejection — exhaustive matrix
    // -----------------------------------------------------------------------

    #[test]
    fn test_completed_to_pending_rejected() {
        let env = Env::default();
        let mut tracker = TransactionStateTracker::new(true);
        let initiator = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        tracker.create_transaction(1, initiator, &env).ok();
        tracker.start_transaction(1, &env).ok();
        tracker.complete_transaction(1, &env).ok();

        let r = tracker.advance_transaction_state(1, TransactionState::Pending, &env);
        assert!(r.is_err(), "Completed → Pending must be rejected");
    }

    #[test]
    fn test_completed_to_in_progress_rejected() {
        let env = Env::default();
        let mut tracker = TransactionStateTracker::new(true);
        let initiator = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        tracker.create_transaction(1, initiator, &env).ok();
        tracker.start_transaction(1, &env).ok();
        tracker.complete_transaction(1, &env).ok();

        let r = tracker.advance_transaction_state(1, TransactionState::InProgress, &env);
        assert!(r.is_err(), "Completed → InProgress must be rejected");
    }

    #[test]
    fn test_completed_to_failed_rejected() {
        let env = Env::default();
        let mut tracker = TransactionStateTracker::new(true);
        let initiator = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        tracker.create_transaction(1, initiator, &env).ok();
        tracker.start_transaction(1, &env).ok();
        tracker.complete_transaction(1, &env).ok();

        let r = tracker.fail_transaction(1, String::from_str(&env, "late failure"), &env);
        assert!(r.is_err(), "Completed → Failed must be rejected");
    }

    #[test]
    fn test_failed_to_in_progress_rejected() {
        let env = Env::default();
        let mut tracker = TransactionStateTracker::new(true);
        let initiator = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        tracker.create_transaction(1, initiator, &env).ok();
        tracker.start_transaction(1, &env).ok();
        tracker.fail_transaction(1, String::from_str(&env, "err"), &env).ok();

        let r = tracker.advance_transaction_state(1, TransactionState::InProgress, &env);
        assert!(r.is_err(), "Failed → InProgress must be rejected");
    }

    #[test]
    fn test_failed_to_completed_rejected() {
        let env = Env::default();
        let mut tracker = TransactionStateTracker::new(true);
        let initiator = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        tracker.create_transaction(1, initiator, &env).ok();
        tracker.start_transaction(1, &env).ok();
        tracker.fail_transaction(1, String::from_str(&env, "err"), &env).ok();

        let r = tracker.complete_transaction(1, &env);
        assert!(r.is_err(), "Failed → Completed must be rejected");
    }

    #[test]
    fn test_failed_to_failed_rejected() {
        let env = Env::default();
        let mut tracker = TransactionStateTracker::new(true);
        let initiator = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        tracker.create_transaction(1, initiator, &env).ok();
        tracker.start_transaction(1, &env).ok();
        tracker.fail_transaction(1, String::from_str(&env, "first failure"), &env).ok();

        let r = tracker.fail_transaction(1, String::from_str(&env, "second failure"), &env);
        assert!(r.is_err(), "Failed → Failed must be rejected");
    }

    #[test]
    fn test_pending_to_completed_directly_rejected() {
        let env = Env::default();
        let mut tracker = TransactionStateTracker::new(true);
        let initiator = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        tracker.create_transaction(1, initiator, &env).ok();

        let r = tracker.complete_transaction(1, &env);
        assert!(r.is_err(), "Pending → Completed must be rejected");
    }

    #[test]
    fn test_pending_to_in_progress_to_failed_valid() {
        let env = Env::default();
        let mut tracker = TransactionStateTracker::new(true);
        let initiator = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        tracker.create_transaction(1, initiator, &env).ok();
        tracker.start_transaction(1, &env).ok();

        let r = tracker.fail_transaction(1, String::from_str(&env, "mid-flight error"), &env);
        assert!(r.is_ok(), "InProgress → Failed must be accepted");
        assert_eq!(r.unwrap().state, TransactionState::Failed);
    }

    #[test]
    fn test_pending_to_failed_directly_valid() {
        // Pending → Failed is a valid transition (immediate pre-processing failure)
        let env = Env::default();
        let mut tracker = TransactionStateTracker::new(true);
        let initiator = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        tracker.create_transaction(1, initiator, &env).ok();

        let r = tracker.fail_transaction(1, String::from_str(&env, "pre-processing failure"), &env);
        assert!(r.is_ok(), "Pending → Failed must be accepted");
        let record = r.unwrap();
        assert_eq!(record.state, TransactionState::Failed);
        assert!(record.recovery_metadata.is_some());
        let meta = record.recovery_metadata.unwrap();
        assert_eq!(meta.failed_from_state, TransactionState::Pending);
    }

    // -----------------------------------------------------------------------
    // Invalid transition error message format
    // -----------------------------------------------------------------------

    #[test]
    fn test_invalid_transition_error_carries_e24_prefix() {
        let env = Env::default();
        let mut tracker = TransactionStateTracker::new(true);
        let initiator = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        tracker.create_transaction(1, initiator, &env).ok();
        tracker.start_transaction(1, &env).ok();
        tracker.complete_transaction(1, &env).ok();

        let err = tracker
            .advance_transaction_state(1, TransactionState::Pending, &env)
            .unwrap_err();
        let err_str = err.to_string();
        assert!(
            err_str.contains("[E24]"),
            "error message must carry [E24] prefix, got: {err_str}"
        );
        assert!(
            err_str.contains("completed"),
            "error message must name the from-state, got: {err_str}"
        );
        assert!(
            err_str.contains("pending"),
            "error message must name the to-state, got: {err_str}"
        );
    }

    // -----------------------------------------------------------------------
    // Recovery metadata
    // -----------------------------------------------------------------------

    #[test]
    fn test_recovery_metadata_populated_on_failure() {
        let env = Env::default();
        let mut tracker = TransactionStateTracker::new(true);
        let initiator = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        tracker.create_transaction(1, initiator, &env).ok();
        tracker.start_transaction(1, &env).ok();

        let reason = String::from_str(&env, "network timeout");
        let record = tracker.fail_transaction(1, reason.clone(), &env).unwrap();

        assert!(record.recovery_metadata.is_some(), "recovery_metadata must be set on failure");
        let meta = record.recovery_metadata.unwrap();
        assert_eq!(meta.failure_reason, reason, "failure_reason must match the error message");
        assert_eq!(meta.failed_from_state, TransactionState::InProgress);
        assert_eq!(meta.retry_count, 0);
        assert!(meta.last_updated_ledger > 0, "last_updated_ledger must be non-zero");
    }

    #[test]
    fn test_recovery_metadata_absent_on_success() {
        let env = Env::default();
        let mut tracker = TransactionStateTracker::new(true);
        let initiator = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        tracker.create_transaction(1, initiator, &env).ok();
        tracker.start_transaction(1, &env).ok();
        let record = tracker.complete_transaction(1, &env).unwrap();

        assert!(record.recovery_metadata.is_none(), "recovery_metadata must be None for Completed");
    }

    #[test]
    fn test_get_recovery_metadata_returns_metadata() {
        let env = Env::default();
        let mut tracker = TransactionStateTracker::new(true);
        let initiator = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        tracker.create_transaction(1, initiator, &env).ok();
        tracker.start_transaction(1, &env).ok();
        tracker.fail_transaction(1, String::from_str(&env, "timeout"), &env).ok();

        let meta = tracker.get_recovery_metadata(1, &env).unwrap();
        assert!(meta.is_some());
        let meta = meta.unwrap();
        assert_eq!(meta.failed_from_state, TransactionState::InProgress);
        assert_eq!(meta.retry_count, 0);
    }

    #[test]
    fn test_get_recovery_metadata_none_for_non_failed() {
        let env = Env::default();
        let mut tracker = TransactionStateTracker::new(true);
        let initiator = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        tracker.create_transaction(1, initiator, &env).ok();

        let meta = tracker.get_recovery_metadata(1, &env).unwrap();
        assert!(meta.is_none(), "recovery_metadata must be None for Pending");
    }

    #[test]
    fn test_is_recoverable_true_for_failed() {
        let env = Env::default();
        let mut tracker = TransactionStateTracker::new(true);
        let initiator = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        tracker.create_transaction(1, initiator, &env).ok();
        tracker.start_transaction(1, &env).ok();
        tracker.fail_transaction(1, String::from_str(&env, "err"), &env).ok();

        assert!(tracker.is_recoverable(1, &env).unwrap());
    }

    #[test]
    fn test_is_recoverable_false_for_completed() {
        let env = Env::default();
        let mut tracker = TransactionStateTracker::new(true);
        let initiator = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        tracker.create_transaction(1, initiator, &env).ok();
        tracker.start_transaction(1, &env).ok();
        tracker.complete_transaction(1, &env).ok();

        assert!(!tracker.is_recoverable(1, &env).unwrap());
    }

    #[test]
    fn test_record_recovery_attempt_increments_retry_count() {
        let env = Env::default();
        let mut tracker = TransactionStateTracker::new(true);
        let initiator = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        tracker.create_transaction(1, initiator, &env).ok();
        tracker.start_transaction(1, &env).ok();
        tracker.fail_transaction(1, String::from_str(&env, "err"), &env).ok();

        tracker.record_recovery_attempt(1, &env).unwrap();
        tracker.record_recovery_attempt(1, &env).unwrap();

        let meta = tracker.get_recovery_metadata(1, &env).unwrap().unwrap();
        assert_eq!(meta.retry_count, 2);
    }

    #[test]
    fn test_record_recovery_attempt_on_non_failed_returns_error() {
        let env = Env::default();
        let mut tracker = TransactionStateTracker::new(true);
        let initiator = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        tracker.create_transaction(1, initiator, &env).ok();
        tracker.start_transaction(1, &env).ok();

        // Transaction is InProgress, not Failed — must return an error
        let r = tracker.record_recovery_attempt(1, &env);
        assert!(r.is_err(), "record_recovery_attempt on non-Failed tx must return error");
    }

    #[test]
    fn test_get_failed_transactions_returns_only_failed() {
        let env = Env::default();
        let mut tracker = TransactionStateTracker::new(true);
        let initiator = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);

        tracker.create_transaction(1, initiator.clone(), &env).ok();
        tracker.create_transaction(2, initiator.clone(), &env).ok();
        tracker.create_transaction(3, initiator.clone(), &env).ok();

        tracker.start_transaction(1, &env).ok();
        tracker.fail_transaction(1, String::from_str(&env, "err"), &env).ok();
        tracker.start_transaction(2, &env).ok();
        tracker.complete_transaction(2, &env).ok();
        // tx 3 stays Pending

        let failed = tracker.get_failed_transactions().unwrap();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].transaction_id, 1);
        assert!(failed[0].recovery_metadata.is_some());
    }

    // -----------------------------------------------------------------------
    // Audit log integrity after mixed transitions
    // -----------------------------------------------------------------------

    #[test]
    fn test_audit_log_mixed_success_and_failure() {
        let env = Env::default();
        let mut tracker = TransactionStateTracker::new(true);
        let initiator = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);

        tracker.create_transaction(1, initiator, &env).ok();
        tracker.start_transaction(1, &env).ok();                                    // success
        let _ = tracker.advance_transaction_state(1, TransactionState::Pending, &env); // failure
        tracker.complete_transaction(1, &env).ok();                                 // success

        assert_eq!(tracker.audit_log.len(), 3);
        assert!(tracker.audit_log[0].success);
        assert!(!tracker.audit_log[1].success);
        assert!(tracker.audit_log[2].success);
    }
}

#[cfg(test)]
mod snapshot_tests {
    use std::collections::HashMap;

    /// Minimal snapshot representation matching the JSON fixtures.
    #[derive(serde::Deserialize, PartialEq, Debug)]
    struct RecordSnapshot {
        transaction_id: u64,
        state: String,
        state_u32: u32,
        initiator: String,
        timestamp: u64,
        last_updated: u64,
        error_message: Option<String>,
    }

    fn load_snapshot(name: &str) -> RecordSnapshot {
        let path = format!(
            "{}/test_snapshots/transaction_state_tracker_tests/{}.json",
            env!("CARGO_MANIFEST_DIR"),
            name
        );
        let data = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("missing snapshot: {path}"));
        serde_json::from_str(&data).unwrap_or_else(|e| panic!("bad snapshot {name}: {e}"))
    }

    #[test]
    fn snapshot_state_discriminants() {
        let cases: HashMap<&str, (&str, u32)> = [
            ("record_pending",     ("Pending",    1)),
            ("record_in_progress", ("InProgress", 2)),
            ("record_completed",   ("Completed",  3)),
            ("record_failed",      ("Failed",     4)),
        ]
        .into();

        for (file, (expected_state, expected_u32)) in &cases {
            let snap = load_snapshot(file);
            assert_eq!(
                snap.state, *expected_state,
                "{file}: state name changed — on-chain encoding regression"
            );
            assert_eq!(
                snap.state_u32, *expected_u32,
                "{file}: state discriminant changed — on-chain encoding regression"
            );
        }
    }

    #[test]
    fn snapshot_failed_has_error_message() {
        let snap = load_snapshot("record_failed");
        assert!(
            snap.error_message.is_some(),
            "record_failed snapshot must have an error_message"
        );
    }

    #[test]
    fn snapshot_non_failed_no_error_message() {
        for name in &["record_pending", "record_in_progress", "record_completed"] {
            let snap = load_snapshot(name);
            assert!(
                snap.error_message.is_none(),
                "{name} snapshot must not have an error_message"
            );
        }
    }
}
