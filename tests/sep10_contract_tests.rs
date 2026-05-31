#![cfg(test)]

mod sep10_test_util;

mod sep10_contract_tests {
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;
    use soroban_sdk::testutils::{Address as _, Ledger, LedgerInfo};
    use soroban_sdk::{Address, Bytes, Env, String};

    use crate::contract::{AnchorKitContract, AnchorKitContractClient};
    use crate::sep10_test_util::{build_sep10_jwt, build_sep10_jwt_with_iat, register_attestor_with_sep10};

    fn make_env() -> Env {
        let env = Env::default();
        env.mock_all_auths();
        env
    }

    fn ledger(env: &Env, ts: u64) {
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

    #[test]
    fn contract_verify_sep10_token_succeeds() {
        let env = make_env();
        ledger(&env, 1000);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let issuer = Address::generate(&env);
        client.initialize(&admin);

        let sk = SigningKey::generate(&mut OsRng);
        let pk = Bytes::from_slice(&env, sk.verifying_key().as_bytes());
        client.set_sep10_jwt_verifying_key(&issuer, &pk);

        let attestor = Address::generate(&env);
        let sub = attestor.to_string();
        let sub_std: std::string::String = sub.to_string();
        let jwt = build_sep10_jwt(&sk, sub_std.as_str(), 2000);
        let token = String::from_str(&env, jwt.as_str());
        client.verify_sep10_token(&token, &issuer);
    }

    #[test]
    fn contract_register_attestor_with_sep10_roundtrip() {
        let env = make_env();
        ledger(&env, 0);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let attestor = Address::generate(&env);
        let issuer = Address::generate(&env);
        client.initialize(&admin);

        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &attestor, &issuer, &sk);
        assert!(client.is_attestor(&attestor));
    }

    // --- Issue #159: clock skew tolerance and max lifetime cap ---

    fn setup(env: &Env, ts: u64) -> (AnchorKitContractClient, Address, SigningKey) {
        ledger(env, ts);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(env, &contract_id);
        let admin = Address::generate(env);
        client.initialize(&admin);
        let sk = SigningKey::generate(&mut OsRng);
        let pk = Bytes::from_slice(env, sk.verifying_key().as_bytes());
        let issuer = Address::generate(env);
        client.set_sep10_jwt_verifying_key(&issuer, &pk);
        (client, issuer, sk)
    }

    /// Token accepted when exp is within the default 60 s skew window.
    #[test]
    fn token_accepted_within_skew_window() {
        let env = make_env();
        // now=1000, exp=1050 — expired by clock but within 60 s skew
        let (client, issuer, sk) = setup(&env, 1100);
        let sub = Address::generate(&env).to_string();
        let sub_str: std::string::String = sub.to_string();
        // exp=1050, now=1100 → diff=50 < skew=60 → accepted
        let jwt = build_sep10_jwt(&sk, &sub_str, 1050);
        let token = String::from_str(&env, &jwt);
        client.verify_sep10_token(&token, &issuer);
    }

    /// Token rejected when exp is just outside the skew window.
    #[test]
    #[should_panic]
    fn token_rejected_just_outside_skew_window() {
        let env = make_env();
        // now=1200, exp=1000 → diff=200 > skew=60 → rejected
        let (client, issuer, sk) = setup(&env, 1200);
        let sub = Address::generate(&env).to_string();
        let sub_str: std::string::String = sub.to_string();
        let jwt = build_sep10_jwt(&sk, &sub_str, 1000);
        let token = String::from_str(&env, &jwt);
        client.verify_sep10_token(&token, &issuer);
    }

    /// Token rejected when lifetime (exp - iat) exceeds 24 hours.
    #[test]
    #[should_panic]
    fn token_rejected_when_lifetime_exceeds_cap() {
        let env = make_env();
        let now = 10_000u64;
        let (client, issuer, sk) = setup(&env, now);
        let sub = Address::generate(&env).to_string();
        let sub_str: std::string::String = sub.to_string();
        // iat=now, exp=now+86401 → lifetime=86401 > 86400 → rejected
        let jwt = build_sep10_jwt_with_iat(&sk, &sub_str, now, now + 86_401);
        let token = String::from_str(&env, &jwt);
        client.verify_sep10_token(&token, &issuer);
    }

    /// Token accepted at exact expiry boundary (exp == now, within default skew).
    #[test]
    fn token_accepted_at_exact_expiry_boundary() {
        let env = make_env();
        let now = 5_000u64;
        let (client, issuer, sk) = setup(&env, now);
        let sub = Address::generate(&env).to_string();
        let sub_str: std::string::String = sub.to_string();
        // exp == now → exp + skew(60) = 5060 > 5000 → accepted
        let jwt = build_sep10_jwt(&sk, &sub_str, now);
        let token = String::from_str(&env, &jwt);
        client.verify_sep10_token(&token, &issuer);
    }
}
