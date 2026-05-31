#![cfg(test)]

mod sep10_test_util;

mod request_id_tests {
    use soroban_sdk::{
        testutils::{Address as _, Ledger, LedgerInfo},
        Address, Bytes, Env, String,
    };

    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    use crate::contract::{AnchorKitContract, AnchorKitContractClient};
    use crate::sep10_test_util::{register_attestor_with_sep10, sign_payload};

    fn make_env() -> Env {
        let env = Env::default();
        env.mock_all_auths();
        env
    }

    fn payload(env: &Env, byte: u8) -> Bytes {
        let mut b = Bytes::new(env);
        for _ in 0..32 {
            b.push_back(byte);
        }
        b
    }

    #[test]
    fn test_generate_request_id() {
        let env = make_env();
        env.ledger().set(LedgerInfo {
            timestamp: 1000,
            protocol_version: 21,
            sequence_number: 0,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let req_id = client.generate_request_id();
        assert_eq!(req_id.created_at, 1000);
        assert_eq!(req_id.id.len(), 16);
    }

    #[test]
    fn test_unique_request_ids() {
        let env = make_env();
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        env.ledger().set(LedgerInfo {
            timestamp: 0,
            protocol_version: 21,
            sequence_number: 0,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });
        let id1 = client.generate_request_id();

        env.ledger().set(LedgerInfo {
            timestamp: 0,
            protocol_version: 21,
            sequence_number: 1,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });
        let id2 = client.generate_request_id();

        assert_ne!(id1.id, id2.id);
    }

    #[test]
    fn test_submit_attestation_with_request_id() {
        let env = make_env();
        env.ledger().set(LedgerInfo {
            timestamp: 1000,
            protocol_version: 21,
            sequence_number: 0,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);

        client.initialize(&admin);
        let signing_key = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &attestor, &attestor, &signing_key);

        let req_id = client.generate_request_id();
        let ph = payload(&env, 0x01);
        let real_sig = sign_payload(&env, &signing_key, &ph);
        let attest_id = client.submit_with_request_id(
            &req_id,
            &attestor,
            &subject,
            &1000u64,
            &ph,
            &real_sig,
        );

        assert_eq!(attest_id, 0);

        let span = client.get_tracing_span(&req_id.id).unwrap();
        assert_eq!(span.operation, String::from_str(&env, "submit_attestation"));
        assert_eq!(span.status, String::from_str(&env, "success"));
        assert_eq!(span.actor, attestor);

        // Verify RequestContext: root_request_id is preserved and operation_chain is populated
        let ctx = client.get_request_context(&req_id.id).unwrap();
        assert_eq!(ctx.root_request_id.id, req_id.id,
            "root_request_id must be preserved across sub-operations");
        assert_eq!(ctx.operation_chain.len(), 1,
            "operation_chain must have exactly one entry");
        assert_eq!(ctx.operation_chain.get(0).unwrap(),
            String::from_str(&env, "submit_attestation"),
            "operation_chain[0] must be 'submit_attestation'");
    }

    #[test]
    fn test_tracing_span_timing() {
        let env = make_env();
        env.ledger().set(LedgerInfo {
            timestamp: 1000,
            protocol_version: 21,
            sequence_number: 0,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);

        client.initialize(&admin);
        let signing_key = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &attestor, &attestor, &signing_key);

        let req_id = client.generate_request_id();
        let ph = payload(&env, 0x01);
        let real_sig = sign_payload(&env, &signing_key, &ph);
        client.submit_with_request_id(
            &req_id,
            &attestor,
            &subject,
            &1000u64,
            &ph,
            &real_sig,
        );

        let span = client.get_tracing_span(&req_id.id).unwrap();
        assert_eq!(span.started_at, 1000);
        assert_eq!(span.completed_at, 1000);
    }

    #[test]
    fn test_tracing_span_records_failure() {
        let env = make_env();
        env.ledger().set(LedgerInfo {
            timestamp: 1000,
            protocol_version: 21,
            sequence_number: 0,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let unregistered = Address::generate(&env);
        let subject = Address::generate(&env);

        client.initialize(&admin);

        let req_id = client.generate_request_id();

        let result = client.try_submit_with_request_id(
            &req_id,
            &unregistered,
            &subject,
            &1000u64,
            &payload(&env, 0x01),
            &Bytes::new(&env),
        );
        assert!(result.is_err());

        let span = client.get_tracing_span(&req_id.id);
        assert!(span.is_none());
    }

    #[test]
    fn test_submit_quote_with_request_id() {
        let env = make_env();
        env.ledger().set(LedgerInfo {
            timestamp: 1000,
            protocol_version: 21,
            sequence_number: 0,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let anchor = Address::generate(&env);

        client.initialize(&admin);
        let signing_key = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &anchor, &anchor, &signing_key);

        let mut services = soroban_sdk::Vec::new(&env);
        services.push_back(3u32);
        client.configure_services(&anchor, &services);

        let req_id = client.generate_request_id();
        client.quote_with_request_id(
            &req_id,
            &anchor,
            &String::from_str(&env, "USD"),
            &String::from_str(&env, "USDC"),
            &10000u64,
            &100u32,
            &100u64,
            &10000u64,
            &4600u64,
        );

        let span = client.get_tracing_span(&req_id.id).unwrap();
        assert_eq!(span.operation, String::from_str(&env, "submit_quote"));
        assert_eq!(span.status, String::from_str(&env, "success"));

        // Verify RequestContext: root_request_id is preserved and operation_chain is populated
        let ctx = client.get_request_context(&req_id.id).unwrap();
        assert_eq!(ctx.root_request_id.id, req_id.id,
            "root_request_id must be preserved across sub-operations");
        assert_eq!(ctx.operation_chain.len(), 1,
            "operation_chain must have exactly one entry");
        assert_eq!(ctx.operation_chain.get(0).unwrap(),
            String::from_str(&env, "submit_quote"),
            "operation_chain[0] must be 'submit_quote'");
    }

    // -----------------------------------------------------------------------
    // New tests: end-to-end RequestContext propagation
    // -----------------------------------------------------------------------

    #[test]
    fn test_root_request_id_preserved_across_sub_operations() {
        let env = make_env();
        env.ledger().set(LedgerInfo {
            timestamp: 2000,
            protocol_version: 21,
            sequence_number: 0,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);

        client.initialize(&admin);
        let signing_key = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &attestor, &attestor, &signing_key);

        // Create root context
        let root_id = client.generate_request_id();
        client.create_request_context(&root_id);

        // Simulate sub-operations appending to the chain
        client.append_operation(&root_id.id, &String::from_str(&env, "sep10_auth"));
        client.append_operation(&root_id.id, &String::from_str(&env, "sep6_deposit"));

        let ctx = client.get_request_context(&root_id.id).unwrap();

        // Root request ID must be preserved
        assert_eq!(ctx.root_request_id.id, root_id.id,
            "root_request_id must be preserved across sub-operations");
        assert_eq!(ctx.root_request_id.created_at, root_id.created_at);

        // Operation chain must be populated in order
        assert_eq!(ctx.operation_chain.len(), 2);
        assert_eq!(ctx.operation_chain.get(0).unwrap(),
            String::from_str(&env, "sep10_auth"));
        assert_eq!(ctx.operation_chain.get(1).unwrap(),
            String::from_str(&env, "sep6_deposit"));
    }

    #[test]
    fn test_operation_chain_populated_in_order() {
        let env = make_env();
        env.ledger().set(LedgerInfo {
            timestamp: 3000,
            protocol_version: 21,
            sequence_number: 0,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        client.initialize(&admin);

        let root_id = client.generate_request_id();
        client.create_request_context(&root_id);

        let ops = ["step_one", "step_two", "step_three"];
        for op in &ops {
            client.append_operation(&root_id.id, &String::from_str(&env, op));
        }

        let ctx = client.get_request_context(&root_id.id).unwrap();
        assert_eq!(ctx.operation_chain.len(), 3);
        for (i, op) in ops.iter().enumerate() {
            assert_eq!(
                ctx.operation_chain.get(i as u32).unwrap(),
                String::from_str(&env, op),
                "operation_chain[{}] must be '{}'", i, op
            );
        }
    }

    #[test]
    fn test_get_request_context_returns_full_chain() {
        let env = make_env();
        env.ledger().set(LedgerInfo {
            timestamp: 4000,
            protocol_version: 21,
            sequence_number: 0,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);

        client.initialize(&admin);
        let signing_key = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &attestor, &attestor, &signing_key);

        // Simulate a full deposit flow: SEP-10 auth → attestation → status poll
        let root_id = client.generate_request_id();
        client.create_request_context(&root_id);
        client.append_operation(&root_id.id, &String::from_str(&env, "sep10_auth"));

        let ph = payload(&env, 0xAA);
        let real_sig = sign_payload(&env, &signing_key, &ph);
        client.submit_with_request_id(
            &root_id,
            &attestor,
            &subject,
            &4000u64,
            &ph,
            &real_sig,
        );

        client.append_operation(&root_id.id, &String::from_str(&env, "transaction_status_poll"));

        // get_request_context must return the full chain for the root request ID
        let ctx = client.get_request_context(&root_id.id).unwrap();
        assert_eq!(ctx.root_request_id.id, root_id.id);
        // Chain: sep10_auth, submit_attestation (auto-appended), transaction_status_poll
        assert_eq!(ctx.operation_chain.len(), 3);
        assert_eq!(ctx.operation_chain.get(0).unwrap(),
            String::from_str(&env, "sep10_auth"));
        assert_eq!(ctx.operation_chain.get(1).unwrap(),
            String::from_str(&env, "submit_attestation"));
        assert_eq!(ctx.operation_chain.get(2).unwrap(),
            String::from_str(&env, "transaction_status_poll"));
    }

    #[test]
    fn test_get_request_context_returns_none_for_unknown_id() {
        let env = make_env();
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        client.initialize(&admin);

        // A request ID that was never used
        let unknown_id = payload(&env, 0xFF);
        let result = client.get_request_context(&unknown_id);
        assert!(result.is_none(), "get_request_context must return None for unknown IDs");
    }

    // -----------------------------------------------------------------------
    // #241 — Deterministic request ID hashing
    // -----------------------------------------------------------------------

    #[test]
    fn test_generate_request_id_is_deterministic_for_same_inputs() {
        // Two calls with identical ledger state (same timestamp + sequence) must
        // produce the same ID bytes.
        let env = make_env();
        env.ledger().set(LedgerInfo {
            timestamp: 5000,
            protocol_version: 21,
            sequence_number: 42,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let id1 = client.generate_request_id();
        let id2 = client.generate_request_id();
        // Same ledger state → same hash
        assert_eq!(id1.id, id2.id);
        assert_eq!(id1.created_at, 5000);
    }

    #[test]
    fn test_generate_child_request_id_differs_from_root() {
        let env = make_env();
        env.ledger().set(LedgerInfo {
            timestamp: 6000,
            protocol_version: 21,
            sequence_number: 1,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let root = client.generate_request_id();
        let child = client.generate_child_request_id(&root.id, &1u64);
        assert_ne!(root.id, child.id, "child ID must differ from root");
        assert_eq!(child.id.len(), 16);
    }

    #[test]
    fn test_generate_child_request_id_different_nonces_produce_different_ids() {
        let env = make_env();
        env.ledger().set(LedgerInfo {
            timestamp: 7000,
            protocol_version: 21,
            sequence_number: 5,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let root = client.generate_request_id();
        let child_a = client.generate_child_request_id(&root.id, &1u64);
        let child_b = client.generate_child_request_id(&root.id, &2u64);
        assert_ne!(child_a.id, child_b.id, "different nonces must produce different child IDs");
    }

    // -----------------------------------------------------------------------
    // #242 — Error code assertions
    // -----------------------------------------------------------------------

    #[test]
    fn test_new_error_code_discriminants() {
        use crate::errors::ErrorCode;
        assert_eq!(ErrorCode::AttestorProfileNotFound as u32, 50);
        assert_eq!(ErrorCode::InvalidRequestContext   as u32, 51);
        assert_eq!(ErrorCode::InvalidSessionMetadata  as u32, 52);
    }

    #[test]
    fn test_new_error_code_messages_non_empty() {
        use crate::errors::ErrorCode;
        assert!(!ErrorCode::AttestorProfileNotFound.default_message().is_empty());
        assert!(!ErrorCode::InvalidRequestContext.default_message().is_empty());
        assert!(!ErrorCode::InvalidSessionMetadata.default_message().is_empty());
    }
}
