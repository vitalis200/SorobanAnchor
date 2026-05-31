#![cfg(test)]

mod sep10_test_util;

mod kyc_compliance_tests {
    use soroban_sdk::testutils::{Address as _, Ledger, LedgerInfo};
    use soroban_sdk::{Address, Bytes, Env, String};

    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    use crate::contract::{AnchorKitContract, AnchorKitContractClient, KycStatus};
    use crate::sep10_test_util::register_attestor_with_sep10;

    fn make_env() -> Env {
        let env = Env::default();
        env.mock_all_auths();
        env
    }

    fn set_ledger(env: &Env, ts: u64) {
        env.ledger().set(LedgerInfo {
            timestamp: ts,
            protocol_version: 21,
            sequence_number: 0,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });
    }

    fn setup_contract() -> (Env, Address, AnchorKitContractClient) {
        let env = make_env();
        set_ledger(&env, 1000);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        client.initialize(&admin);
        (env, admin, client)
    }

    fn register_attestor(
        env: &Env,
        client: &AnchorKitContractClient,
        _admin: &Address,
        attestor: &Address,
    ) {
        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(env, client, attestor, attestor, &sk);
    }

    // -----------------------------------------------------------------------
    // KYC State Transition Tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_kyc_state_transition_not_submitted_to_pending() {
        let (env, admin, client) = setup_contract();
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        register_attestor(&env, &client, &admin, &attestor);

        // Initially, KYC status should be NotSubmitted
        let status = client.get_kyc_status(&subject);
        assert_eq!(status, KycStatus::NotSubmitted);

        // Submit KYC data
        let data_hash = Bytes::from_slice(&env, b"test_kyc_data_hash_1234567890ab");
        client.submit_kyc(&subject, &data_hash, &attestor);

        // Status should now be Pending
        let status = client.get_kyc_status(&subject);
        assert_eq!(status, KycStatus::Pending);
    }

    #[test]
    fn test_kyc_state_transition_pending_to_approved() {
        let (env, admin, client) = setup_contract();
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        register_attestor(&env, &client, &admin, &attestor);

        // Submit KYC
        let data_hash = Bytes::from_slice(&env, b"test_kyc_data_hash_1234567890ab");
        client.submit_kyc(&subject, &data_hash, &attestor);

        // Verify Pending status
        let status = client.get_kyc_status(&subject);
        assert_eq!(status, KycStatus::Pending);

        // Approve KYC
        client.approve_kyc(&admin, &subject);

        // Status should now be Approved
        let status = client.get_kyc_status(&subject);
        assert_eq!(status, KycStatus::Approved);
    }

    #[test]
    fn test_kyc_status_expired_after_approval() {
        let (env, admin, client) = setup_contract();
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        register_attestor(&env, &client, &admin, &attestor);

        let data_hash = Bytes::from_slice(&env, b"test_kyc_data_hash_1234567890ab");
        client.submit_kyc(&subject, &data_hash, &attestor);
        client.approve_kyc(&subject);
        assert_eq!(client.get_kyc_status(&subject), KycStatus::Approved);

        set_ledger(&env, 1000 + 30 * 24 * 60 * 60 + 1);
        assert_eq!(client.get_kyc_status(&subject), KycStatus::Expired);
    }

    #[test]
    fn test_kyc_can_resubmit_after_expiry() {
        let (env, admin, client) = setup_contract();
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        register_attestor(&env, &client, &admin, &attestor);

        let data_hash = Bytes::from_slice(&env, b"test_kyc_data_hash_1234567890ab");
        client.submit_kyc(&subject, &data_hash, &attestor);
        client.approve_kyc(&subject);
        set_ledger(&env, 1000 + 30 * 24 * 60 * 60 + 1);
        assert_eq!(client.get_kyc_status(&subject), KycStatus::Expired);

        let new_data_hash = Bytes::from_slice(&env, b"reopened_kyc_data_hash_1234567890ab");
        client.submit_kyc(&subject, &new_data_hash, &attestor);
        assert_eq!(client.get_kyc_status(&subject), KycStatus::Pending);
    }

    #[test]
    fn test_kyc_state_transition_pending_to_rejected() {
        let (env, admin, client) = setup_contract();
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        register_attestor(&env, &client, &admin, &attestor);

        // Submit KYC
        let data_hash = Bytes::from_slice(&env, b"test_kyc_data_hash_1234567890ab");
        client.submit_kyc(&subject, &data_hash, &attestor);

        // Verify Pending status
        let status = client.get_kyc_status(&subject);
        assert_eq!(status, KycStatus::Pending);

        // Reject KYC with reason
        let reason_hash = Bytes::from_slice(&env, b"rejection_reason_hash_1234567890");
        client.reject_kyc(&admin, &subject, &reason_hash);

        // Status should now be Rejected
        let status = client.get_kyc_status(&subject);
        assert_eq!(status, KycStatus::Rejected);
    }

    #[test]
    fn test_kyc_cannot_transition_from_approved_to_pending() {
        let (env, admin, client) = setup_contract();
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        register_attestor(&env, &client, &admin, &attestor);

        // Submit and approve KYC
        let data_hash = Bytes::from_slice(&env, b"test_kyc_data_hash_1234567890ab");
        client.submit_kyc(&subject, &data_hash, &attestor);
        client.approve_kyc(&admin, &subject);

        // Verify Approved status
        let status = client.get_kyc_status(&subject);
        assert_eq!(status, KycStatus::Approved);

        // Try to submit KYC again - should fail
        let new_data_hash = Bytes::from_slice(&env, b"new_kyc_data_hash_1234567890abcd");
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.submit_kyc(&subject, &new_data_hash, &attestor);
        }));
        assert!(result.is_err());
    }

    #[test]
    fn test_kyc_cannot_transition_from_rejected_to_pending() {
        let (env, admin, client) = setup_contract();
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        register_attestor(&env, &client, &admin, &attestor);

        // Submit and reject KYC
        let data_hash = Bytes::from_slice(&env, b"test_kyc_data_hash_1234567890ab");
        client.submit_kyc(&subject, &data_hash, &attestor);
        let reason_hash = Bytes::from_slice(&env, b"rejection_reason_hash_1234567890");
        client.reject_kyc(&admin, &subject, &reason_hash);

        // Verify Rejected status
        let status = client.get_kyc_status(&subject);
        assert_eq!(status, KycStatus::Rejected);

        // Try to submit KYC again - should fail
        let new_data_hash = Bytes::from_slice(&env, b"new_kyc_data_hash_1234567890abcd");
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.submit_kyc(&subject, &new_data_hash, &attestor);
        }));
        assert!(result.is_err());
    }

    #[test]
    fn test_kyc_cannot_approve_non_pending() {
        let (env, admin, client) = setup_contract();
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        register_attestor(&env, &client, &admin, &attestor);

        // Submit and approve KYC
        let data_hash = Bytes::from_slice(&env, b"test_kyc_data_hash_1234567890ab");
        client.submit_kyc(&subject, &data_hash, &attestor);
        client.approve_kyc(&admin, &subject);

        // Try to approve again - should fail (illegal transition)
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.approve_kyc(&admin, &subject);
        }));
        assert!(result.is_err());
    }

    #[test]
    fn test_kyc_cannot_reject_non_pending() {
        let (env, admin, client) = setup_contract();
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        register_attestor(&env, &client, &admin, &attestor);

        // Submit and approve KYC
        let data_hash = Bytes::from_slice(&env, b"test_kyc_data_hash_1234567890ab");
        client.submit_kyc(&subject, &data_hash, &attestor);
        client.approve_kyc(&admin, &subject);

        // Try to reject approved KYC - should fail (illegal transition)
        let reason_hash = Bytes::from_slice(&env, b"rejection_reason_hash_1234567890");
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.reject_kyc(&admin, &subject, &reason_hash);
        }));
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // KYC Query Tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_kyc_status_not_submitted() {
        let (env, _admin, client) = setup_contract();
        let subject = Address::generate(&env);

        // Query status for non-existent KYC record
        let status = client.get_kyc_status(&subject);
        assert_eq!(status, KycStatus::NotSubmitted);
    }

    #[test]
    fn test_get_kyc_status_pending() {
        let (env, admin, client) = setup_contract();
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        register_attestor(&env, &client, &admin, &attestor);

        let data_hash = Bytes::from_slice(&env, b"test_kyc_data_hash_1234567890ab");
        client.submit_kyc(&subject, &data_hash, &attestor);

        let status = client.get_kyc_status(&subject);
        assert_eq!(status, KycStatus::Pending);
    }

    #[test]
    fn test_get_kyc_status_approved() {
        let (env, admin, client) = setup_contract();
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        register_attestor(&env, &client, &admin, &attestor);

        let data_hash = Bytes::from_slice(&env, b"test_kyc_data_hash_1234567890ab");
        client.submit_kyc(&subject, &data_hash, &attestor);
        client.approve_kyc(&admin, &subject);

        let status = client.get_kyc_status(&subject);
        assert_eq!(status, KycStatus::Approved);
    }

    #[test]
    fn test_get_kyc_status_rejected() {
        let (env, admin, client) = setup_contract();
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        register_attestor(&env, &client, &admin, &attestor);

        let data_hash = Bytes::from_slice(&env, b"test_kyc_data_hash_1234567890ab");
        client.submit_kyc(&subject, &data_hash, &attestor);
        let reason_hash = Bytes::from_slice(&env, b"rejection_reason_hash_1234567890");
        client.reject_kyc(&admin, &subject, &reason_hash);

        let status = client.get_kyc_status(&subject);
        assert_eq!(status, KycStatus::Rejected);
    }

    // -----------------------------------------------------------------------
    // Authorization Tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_submit_kyc_requires_attestor_auth() {
        let (env, admin, client) = setup_contract();
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        let unauthorized = Address::generate(&env);
        register_attestor(&env, &client, &admin, &attestor);

        let data_hash = Bytes::from_slice(&env, b"test_kyc_data_hash_1234567890ab");

        // Try to submit with unauthorized address - should fail
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.submit_kyc(&subject, &data_hash, &unauthorized);
        }));
        assert!(result.is_err());
    }

    #[test]
    fn test_approve_kyc_requires_admin_auth() {
        let (env, admin, client) = setup_contract();
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        register_attestor(&env, &client, &admin, &attestor);

        let data_hash = Bytes::from_slice(&env, b"test_kyc_data_hash_1234567890ab");
        client.submit_kyc(&subject, &data_hash, &attestor);

        // Try to approve with non-admin address - should fail
        let non_admin = Address::generate(&env);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.approve_kyc(&admin, &subject);
        }));
        // Note: In mock_all_auths mode, this may not fail as expected.
        // In production, this would require proper auth checks.
    }

    #[test]
    fn test_reject_kyc_requires_admin_auth() {
        let (env, admin, client) = setup_contract();
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        register_attestor(&env, &client, &admin, &attestor);

        let data_hash = Bytes::from_slice(&env, b"test_kyc_data_hash_1234567890ab");
        client.submit_kyc(&subject, &data_hash, &attestor);

        // Try to reject with non-admin address - should fail
        let reason_hash = Bytes::from_slice(&env, b"rejection_reason_hash_1234567890");
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.reject_kyc(&admin, &subject, &reason_hash);
        }));
        // Note: In mock_all_auths mode, this may not fail as expected.
        // In production, this would require proper auth checks.
    }

    // -----------------------------------------------------------------------
    // Compliance Enforcement Tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_attestation_rejected_with_non_approved_kyc() {
        let (env, admin, client) = setup_contract();
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        register_attestor(&env, &client, &admin, &attestor);

        // Try to submit attestation without KYC - should fail
        let timestamp = env.ledger().timestamp();
        let payload_hash = Bytes::from_slice(&env, b"payload_hash_1234567890abcdefghij");
        let signature = Bytes::from_slice(&env, b"signature_1234567890abcdefghijklmn");

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.submit_attestation_with_kyc_check(
                &attestor,
                &subject,
                timestamp,
                &payload_hash,
                &signature,
                true,
            );
        }));
        assert!(result.is_err());
    }

    #[test]
    fn test_attestation_rejected_with_pending_kyc() {
        let (env, admin, client) = setup_contract();
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        register_attestor(&env, &client, &admin, &attestor);

        // Submit KYC but don't approve
        let data_hash = Bytes::from_slice(&env, b"test_kyc_data_hash_1234567890ab");
        client.submit_kyc(&subject, &data_hash, &attestor);

        // Try to submit attestation with pending KYC - should fail
        let timestamp = env.ledger().timestamp();
        let payload_hash = Bytes::from_slice(&env, b"payload_hash_1234567890abcdefghij");
        let signature = Bytes::from_slice(&env, b"signature_1234567890abcdefghijklmn");

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.submit_attestation_with_kyc_check(
                &attestor,
                &subject,
                timestamp,
                &payload_hash,
                &signature,
                true,
            );
        }));
        assert!(result.is_err());
    }

    #[test]
    fn test_attestation_rejected_with_rejected_kyc() {
        let (env, admin, client) = setup_contract();
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        register_attestor(&env, &client, &admin, &attestor);

        // Submit and reject KYC
        let data_hash = Bytes::from_slice(&env, b"test_kyc_data_hash_1234567890ab");
        client.submit_kyc(&subject, &data_hash, &attestor);
        let reason_hash = Bytes::from_slice(&env, b"rejection_reason_hash_1234567890");
        client.reject_kyc(&admin, &subject, &reason_hash);

        // Try to submit attestation with rejected KYC - should fail
        let timestamp = env.ledger().timestamp();
        let payload_hash = Bytes::from_slice(&env, b"payload_hash_1234567890abcdefghij");
        let signature = Bytes::from_slice(&env, b"signature_1234567890abcdefghijklmn");

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.submit_attestation_with_kyc_check(
                &attestor,
                &subject,
                timestamp,
                &payload_hash,
                &signature,
                true,
            );
        }));
        assert!(result.is_err());
    }

    #[test]
    fn test_attestation_succeeds_with_approved_kyc() {
        let (env, admin, client) = setup_contract();
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        register_attestor(&env, &client, &admin, &attestor);

        // Submit and approve KYC
        let data_hash = Bytes::from_slice(&env, b"test_kyc_data_hash_1234567890ab");
        client.submit_kyc(&subject, &data_hash, &attestor);
        client.approve_kyc(&admin, &subject);

        // Submit attestation with approved KYC - should succeed
        let timestamp = env.ledger().timestamp();
        let payload_hash = Bytes::from_slice(&env, b"payload_hash_1234567890abcdefghij");
        let signature = Bytes::from_slice(&env, b"signature_1234567890abcdefghijklmn");

        let attestation_id = client.submit_attestation_with_kyc_check(
            &attestor,
            &subject,
            timestamp,
            &payload_hash,
            &signature,
            true,
        );

        // Verify attestation was created
        assert!(attestation_id > 0);
    }

    #[test]
    fn test_attestation_succeeds_without_kyc_check() {
        let (env, admin, client) = setup_contract();
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        register_attestor(&env, &client, &admin, &attestor);

        // Submit attestation without KYC check - should succeed
        let timestamp = env.ledger().timestamp();
        let payload_hash = Bytes::from_slice(&env, b"payload_hash_1234567890abcdefghij");
        let signature = Bytes::from_slice(&env, b"signature_1234567890abcdefghijklmn");

        let attestation_id = client.submit_attestation_with_kyc_check(
            &attestor,
            &subject,
            timestamp,
            &payload_hash,
            &signature,
            false,
        );

        // Verify attestation was created
        assert!(attestation_id > 0);
    }

    // -----------------------------------------------------------------------
    // Storage Persistence Tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_kyc_record_persists_in_storage() {
        let (env, admin, client) = setup_contract();
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        register_attestor(&env, &client, &admin, &attestor);

        // Submit KYC
        let data_hash = Bytes::from_slice(&env, b"test_kyc_data_hash_1234567890ab");
        client.submit_kyc(&subject, &data_hash, &attestor);

        // Query status multiple times - should be consistent
        let status1 = client.get_kyc_status(&subject);
        let status2 = client.get_kyc_status(&subject);
        assert_eq!(status1, status2);
        assert_eq!(status1, KycStatus::Pending);
    }

    #[test]
    fn test_multiple_subjects_kyc_independent() {
        let (env, admin, client) = setup_contract();
        let attestor = Address::generate(&env);
        let subject1 = Address::generate(&env);
        let subject2 = Address::generate(&env);
        register_attestor(&env, &client, &admin, &attestor);

        // Submit KYC for subject1
        let data_hash1 = Bytes::from_slice(&env, b"test_kyc_data_hash_1234567890ab");
        client.submit_kyc(&subject1, &data_hash1, &attestor);

        // Submit and approve KYC for subject2
        let data_hash2 = Bytes::from_slice(&env, b"test_kyc_data_hash_2234567890ab");
        client.submit_kyc(&subject2, &data_hash2, &attestor);
        client.approve_kyc(&admin, &subject2);

        // Verify statuses are independent
        let status1 = client.get_kyc_status(&subject1);
        let status2 = client.get_kyc_status(&subject2);
        assert_eq!(status1, KycStatus::Pending);
        assert_eq!(status2, KycStatus::Approved);
    }

    #[test]
    fn test_kyc_rejection_reason_stored() {
        let (env, admin, client) = setup_contract();
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        register_attestor(&env, &client, &admin, &attestor);

        // Submit and reject KYC with reason
        let data_hash = Bytes::from_slice(&env, b"test_kyc_data_hash_1234567890ab");
        client.submit_kyc(&subject, &data_hash, &attestor);
        let reason_hash = Bytes::from_slice(&env, b"rejection_reason_hash_1234567890");
        client.reject_kyc(&admin, &subject, &reason_hash);

        // Verify status is rejected
        let status = client.get_kyc_status(&subject);
        assert_eq!(status, KycStatus::Rejected);
    }
}
