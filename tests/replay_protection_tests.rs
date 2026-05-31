#![cfg(test)]

mod sep10_test_util;

mod replay_protection_tests {
    use soroban_sdk::{
        testutils::{Address as _, Ledger, LedgerInfo},
        Address, Bytes, Env,
    };

    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    use crate::contract::{AnchorKitContract, AnchorKitContractClient};
    use crate::sep10_test_util::register_attestor_with_sep10;

    fn make_env() -> Env {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set(LedgerInfo {
            timestamp: 1_000_000,
            protocol_version: 21,
            sequence_number: 0,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });
        env
    }

    fn payload(env: &Env, byte: u8) -> Bytes {
        let mut b = Bytes::new(env);
        for _ in 0..32 {
            b.push_back(byte);
        }
        b
    }

    fn sig(env: &Env) -> Bytes {
        let mut b = Bytes::new(env);
        b.push_back(0xaa);
        b.push_back(0xbb);
        b
    }

    fn setup(env: &Env) -> (AnchorKitContractClient, Address, Address, Address) {
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(env, &contract_id);
        let admin = Address::generate(env);
        let issuer = Address::generate(env);
        let subject = Address::generate(env);
        client.initialize(&admin);
        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(env, &client, &issuer, &issuer, &sk);
        (client, admin, issuer, subject)
    }

    // -----------------------------------------------------------------------
    // Identical (issuer, payload_hash) rejected on second submission
    // -----------------------------------------------------------------------

    #[test]
    #[should_panic]
    fn test_duplicate_attestation_rejected() {
        let env = make_env();
        let (client, _, issuer, subject) = setup(&env);

        let hash = payload(&env, 0x01);
        client.submit_attestation(&issuer, &subject, &1_000_001u64, &hash, &sig(&env));
        // Second submission with same issuer + hash must panic with ReplayAttack
        client.submit_attestation(&issuer, &subject, &1_000_002u64, &hash, &sig(&env));
    }

    // -----------------------------------------------------------------------
    // Different payload hash from same issuer is accepted
    // -----------------------------------------------------------------------

    #[test]
    fn test_different_hash_same_issuer_accepted() {
        let env = make_env();
        let (client, _, issuer, subject) = setup(&env);

        let id0 = client.submit_attestation(&issuer, &subject, &1_000_001u64, &payload(&env, 0x01), &sig(&env));
        let id1 = client.submit_attestation(&issuer, &subject, &1_000_002u64, &payload(&env, 0x02), &sig(&env));
        assert_eq!(id0, 0);
        assert_eq!(id1, 1);
    }

    // -----------------------------------------------------------------------
    // Same payload hash from different issuer is accepted
    // -----------------------------------------------------------------------

    #[test]
    fn test_same_hash_different_issuer_accepted() {
        let env = make_env();
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let issuer_a = Address::generate(&env);
        let issuer_b = Address::generate(&env);
        let subject = Address::generate(&env);
        client.initialize(&admin);

        let sk_a = SigningKey::generate(&mut OsRng);
        let sk_b = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &issuer_a, &issuer_a, &sk_a);
        register_attestor_with_sep10(&env, &client, &issuer_b, &issuer_b, &sk_b);

        let hash = payload(&env, 0x42);
        let id0 = client.submit_attestation(&issuer_a, &subject, &1_000_001u64, &hash, &sig(&env));
        let id1 = client.submit_attestation(&issuer_b, &subject, &1_000_002u64, &hash, &sig(&env));
        assert_eq!(id0, 0);
        assert_eq!(id1, 1);
    }

    // -----------------------------------------------------------------------
    // get_attestation_by_hash returns original timestamp
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_attestation_by_hash_returns_timestamp() {
        let env = make_env();
        let (client, _, issuer, subject) = setup(&env);

        let hash = payload(&env, 0x10);
        let ts = 1_000_001u64;
        client.submit_attestation(&issuer, &subject, &ts, &hash, &sig(&env));

        let stored_ts = client.get_attestation_by_hash(&issuer, &hash);
        assert_eq!(stored_ts, ts);
    }

    #[test]
    #[should_panic]
    fn test_get_attestation_by_hash_not_found_panics() {
        let env = make_env();
        let (client, _, issuer, _) = setup(&env);
        // Never submitted — must panic with AttestationNotFound
        client.get_attestation_by_hash(&issuer, &payload(&env, 0xff));
    }

    // -----------------------------------------------------------------------
    // Session nonce incremented after each successful attestation
    // -----------------------------------------------------------------------

    #[test]
    fn test_session_nonce_incremented_after_attestation() {
        let env = make_env();
        let (client, _, issuer, subject) = setup(&env);

        let session_id = client.create_session(&issuer);
        let before = client.get_session(&session_id).nonce;

        client.submit_attestation_with_session(
            &session_id, &issuer, &subject, &1_000_001u64, &payload(&env, 0x01), &sig(&env),
        );
        let after = client.get_session(&session_id).nonce;
        assert_eq!(after, before + 1);

        client.submit_attestation_with_session(
            &session_id, &issuer, &subject, &1_000_002u64, &payload(&env, 0x02), &sig(&env),
        );
        let after2 = client.get_session(&session_id).nonce;
        assert_eq!(after2, before + 2);
    }

    // -----------------------------------------------------------------------
    // Replay via submit_attestation_with_session also rejected
    // -----------------------------------------------------------------------

    #[test]
    #[should_panic]
    fn test_session_replay_rejected() {
        let env = make_env();
        let (client, _, issuer, subject) = setup(&env);

        let session_id = client.create_session(&issuer);
        let hash = payload(&env, 0x20);

        client.submit_attestation_with_session(
            &session_id, &issuer, &subject, &1_000_001u64, &hash, &sig(&env),
        );
        // Same issuer + hash in same session must be rejected
        client.submit_attestation_with_session(
            &session_id, &issuer, &subject, &1_000_002u64, &hash, &sig(&env),
        );
    }
}
