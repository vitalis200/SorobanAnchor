#![cfg(test)]

mod sep10_test_util;

mod admin_permission_tests {
    use soroban_sdk::{
        testutils::{Address as _, Ledger, LedgerInfo},
        Address, Env,
    };

    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    use crate::contract::{AdminRole, AnchorKitContract, AnchorKitContractClient};
    use crate::sep10_test_util::register_attestor_with_sep10;

    fn make_env() -> Env {
        let env = Env::default();
        env.mock_all_auths();
        env
    }

    fn setup_ledger(env: &Env) {
        env.ledger().set(LedgerInfo {
            timestamp: 1_000,
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
        setup_ledger(&env);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        client.initialize(&admin);
        (env, admin, client)
    }

    // -----------------------------------------------------------------------
    // Issue #344 — admin permission model enforcement
    // -----------------------------------------------------------------------

    /// The primary admin can approve a pending KYC record.
    #[test]
    fn test_admin_can_approve_kyc() {
        let (env, admin, client) = setup_contract();
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &attestor, &attestor, &sk);

        let data_hash = soroban_sdk::Bytes::from_slice(&env, b"kyc_data_hash_1234567890abcdefgh");
        client.submit_kyc(&subject, &data_hash, &attestor);
        client.approve_kyc(&admin, &subject);

        let status = client.get_kyc_status(&subject);
        assert_eq!(status, crate::contract::KycStatus::Approved);
    }

    /// The primary admin can reject a pending KYC record.
    #[test]
    fn test_admin_can_reject_kyc() {
        let (env, admin, client) = setup_contract();
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &attestor, &attestor, &sk);

        let data_hash = soroban_sdk::Bytes::from_slice(&env, b"kyc_data_hash_1234567890abcdefgh");
        client.submit_kyc(&subject, &data_hash, &attestor);
        let reason = soroban_sdk::Bytes::from_slice(&env, b"reason_hash_1234567890abcdefghij");
        client.reject_kyc(&admin, &subject, &reason);

        let status = client.get_kyc_status(&subject);
        assert_eq!(status, crate::contract::KycStatus::Rejected);
    }

    /// A non-admin address without a KycAdmin role is rejected.
    #[test]
    #[should_panic]
    fn test_non_admin_cannot_approve_kyc_without_role() {
        let env = Env::default(); // NO mock_all_auths — auth is enforced
        setup_ledger(&env);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let non_admin = Address::generate(&env);
        let subject = Address::generate(&env);

        env.mock_auths(&[soroban_sdk::testutils::MockAuth {
            address: &admin,
            invoke: &soroban_sdk::testutils::MockAuthInvoke {
                contract: &contract_id,
                fn_name: "initialize",
                args: soroban_sdk::vec![&env, admin.clone().into()],
                sub_invokes: &[],
            },
        }]);
        client.initialize(&admin);

        // Attempt approve_kyc as non_admin with no role — should panic.
        client.approve_kyc(&non_admin, &subject);
    }

    // -----------------------------------------------------------------------
    // Issue #345 — multi-admin / role-based access control
    // -----------------------------------------------------------------------

    /// `has_role` returns false for an address that was never granted any role.
    #[test]
    fn test_has_role_false_for_ungranted_address() {
        let (env, _admin, client) = setup_contract();
        let stranger = Address::generate(&env);
        assert!(!client.has_role(&stranger, &AdminRole::KycAdmin));
        assert!(!client.has_role(&stranger, &AdminRole::AttestorAdmin));
        assert!(!client.has_role(&stranger, &AdminRole::CacheAdmin));
    }

    /// After `grant_role`, `has_role` returns true for the grantee.
    #[test]
    fn test_grant_role_makes_has_role_true() {
        let (env, _admin, client) = setup_contract();
        let delegate = Address::generate(&env);

        assert!(!client.has_role(&delegate, &AdminRole::KycAdmin));
        client.grant_role(&delegate, &AdminRole::KycAdmin);
        assert!(client.has_role(&delegate, &AdminRole::KycAdmin));
    }

    /// Granting one role does not implicitly grant other roles.
    #[test]
    fn test_grant_role_is_role_specific() {
        let (env, _admin, client) = setup_contract();
        let delegate = Address::generate(&env);

        client.grant_role(&delegate, &AdminRole::KycAdmin);
        assert!(client.has_role(&delegate, &AdminRole::KycAdmin));
        assert!(!client.has_role(&delegate, &AdminRole::AttestorAdmin));
        assert!(!client.has_role(&delegate, &AdminRole::CacheAdmin));
    }

    /// After `revoke_role`, `has_role` returns false again.
    #[test]
    fn test_revoke_role_removes_grant() {
        let (env, _admin, client) = setup_contract();
        let delegate = Address::generate(&env);

        client.grant_role(&delegate, &AdminRole::KycAdmin);
        assert!(client.has_role(&delegate, &AdminRole::KycAdmin));

        client.revoke_role(&delegate, &AdminRole::KycAdmin);
        assert!(!client.has_role(&delegate, &AdminRole::KycAdmin));
    }

    /// Revoking a role that was never granted is a no-op (does not panic).
    #[test]
    fn test_revoke_role_idempotent() {
        let (env, _admin, client) = setup_contract();
        let delegate = Address::generate(&env);

        // Revoke without prior grant — should not panic.
        client.revoke_role(&delegate, &AdminRole::KycAdmin);
        assert!(!client.has_role(&delegate, &AdminRole::KycAdmin));
    }

    /// The primary admin implicitly passes `has_role` for every role.
    #[test]
    fn test_primary_admin_has_all_roles_implicitly() {
        let (env, admin, client) = setup_contract();

        assert!(client.has_role(&admin, &AdminRole::KycAdmin));
        assert!(client.has_role(&admin, &AdminRole::AttestorAdmin));
        assert!(client.has_role(&admin, &AdminRole::CacheAdmin));
    }

    /// A KycAdmin role-holder can approve KYC without being the primary admin.
    #[test]
    fn test_kyc_admin_role_allows_approve_kyc() {
        let (env, _admin, client) = setup_contract();
        let delegate = Address::generate(&env);
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);

        client.grant_role(&delegate, &AdminRole::KycAdmin);

        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &attestor, &attestor, &sk);

        let data_hash = soroban_sdk::Bytes::from_slice(&env, b"kyc_data_hash_1234567890abcdefgh");
        client.submit_kyc(&subject, &data_hash, &attestor);
        client.approve_kyc(&delegate, &subject);

        let status = client.get_kyc_status(&subject);
        assert_eq!(status, crate::contract::KycStatus::Approved);
    }

    /// A KycAdmin role-holder can reject KYC.
    #[test]
    fn test_kyc_admin_role_allows_reject_kyc() {
        let (env, _admin, client) = setup_contract();
        let delegate = Address::generate(&env);
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);

        client.grant_role(&delegate, &AdminRole::KycAdmin);

        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &attestor, &attestor, &sk);

        let data_hash = soroban_sdk::Bytes::from_slice(&env, b"kyc_data_hash_1234567890abcdefgh");
        client.submit_kyc(&subject, &data_hash, &attestor);
        let reason = soroban_sdk::Bytes::from_slice(&env, b"reason_hash_1234567890abcdefghij");
        client.reject_kyc(&delegate, &subject, &reason);

        let status = client.get_kyc_status(&subject);
        assert_eq!(status, crate::contract::KycStatus::Rejected);
    }

    /// An AttestorAdmin role-holder can register attestors in a session.
    #[test]
    fn test_attestor_admin_role_allows_register_with_session() {
        let (env, _admin, client) = setup_contract();
        let delegate = Address::generate(&env);
        let user = Address::generate(&env);
        let new_attestor = Address::generate(&env);

        client.grant_role(&delegate, &AdminRole::AttestorAdmin);

        let session_id = client.create_session(&user);
        let sk = SigningKey::generate(&mut OsRng);
        let pk = soroban_sdk::BytesN::from_array(&env, sk.verifying_key().as_bytes());
        client.register_attestor_with_session(&delegate, &session_id, &new_attestor, &pk);

        assert!(client.is_attestor(&new_attestor));
    }

    /// An AttestorAdmin role-holder can revoke attestors in a session.
    #[test]
    fn test_attestor_admin_role_allows_revoke_with_session() {
        let (env, admin, client) = setup_contract();
        let delegate = Address::generate(&env);
        let user = Address::generate(&env);
        let existing_attestor = Address::generate(&env);

        // Register the attestor first via primary admin.
        let sk = SigningKey::generate(&mut OsRng);
        let pk = soroban_sdk::BytesN::from_array(&env, sk.verifying_key().as_bytes());
        let session_id = client.create_session(&user);
        client.register_attestor_with_session(&admin, &session_id, &existing_attestor, &pk);
        assert!(client.is_attestor(&existing_attestor));

        // Grant AttestorAdmin to delegate and revoke via delegate.
        client.grant_role(&delegate, &AdminRole::AttestorAdmin);
        let session2 = client.create_session(&user);
        client.revoke_attestor_with_session(&delegate, &session2, &existing_attestor);
        assert!(!client.is_attestor(&existing_attestor));
    }

    /// Multiple addresses can hold the same role independently.
    #[test]
    fn test_multiple_addresses_can_hold_same_role() {
        let (env, _admin, client) = setup_contract();
        let delegate_a = Address::generate(&env);
        let delegate_b = Address::generate(&env);

        client.grant_role(&delegate_a, &AdminRole::KycAdmin);
        client.grant_role(&delegate_b, &AdminRole::KycAdmin);

        assert!(client.has_role(&delegate_a, &AdminRole::KycAdmin));
        assert!(client.has_role(&delegate_b, &AdminRole::KycAdmin));
    }
}
