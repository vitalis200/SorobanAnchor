#![cfg(test)]

mod sep10_test_util;

mod tracing_span_tests {
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
    fn test_span_propagates_across_operations() {
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
        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &attestor, &attestor, &sk);

        // Root span
        let root_id = client.generate_request_id();
        client.submit_with_request_id(
            &root_id,
            &attestor,
            &subject,
            &1000u64,
            &payload(&env, 0x01),
            &Bytes::new(&env),
        );

        // Child span
        let child_id = client.generate_request_id();
        client.propagate_span(
            &root_id,
            &child_id,
            &String::from_str(&env, "fetch_transaction_status"),
            &attestor,
        );

        // Verify child references parent
        let child_span = client.get_tracing_span(&child_id.id).unwrap();
        assert_eq!(child_span.parent_request_id_bytes, root_id.id);
        assert_eq!(child_span.span_index, 1);

        // Root span has no parent (empty bytes)
        let root_span = client.get_tracing_span(&root_id.id).unwrap();
        assert!(root_span.parent_request_id_bytes.is_empty());
        assert_eq!(root_span.span_index, 0);
    }

    #[test]
    fn test_root_span_has_no_parent() {
        let env = make_env();
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
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);

        client.initialize(&admin);
        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &attestor, &attestor, &sk);

        let req_id = client.generate_request_id();
        client.submit_with_request_id(
            &req_id,
            &attestor,
            &subject,
            &1000u64,
            &payload(&env, 0x02),
            &Bytes::new(&env),
        );

        let span = client.get_tracing_span(&req_id.id).unwrap();
        assert!(span.parent_request_id_bytes.is_empty(), "root span must have no parent");
        assert_eq!(span.span_index, 0);
    }

    #[test]
    fn test_sibling_spans_share_same_parent() {
        let env = make_env();
        env.ledger().set(LedgerInfo {
            timestamp: 500,
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
        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &attestor, &attestor, &sk);

        let root_id = client.generate_request_id();
        client.submit_with_request_id(
            &root_id,
            &attestor,
            &subject,
            &1000u64,
            &payload(&env, 0x03),
            &Bytes::new(&env),
        );

        let child_a = client.generate_request_id();
        let child_b = client.generate_request_id();

        client.propagate_span(
            &root_id,
            &child_a,
            &String::from_str(&env, "step_a"),
            &attestor,
        );
        client.propagate_span(
            &root_id,
            &child_b,
            &String::from_str(&env, "step_b"),
            &attestor,
        );

        let span_a = client.get_tracing_span(&child_a.id).unwrap();
        let span_b = client.get_tracing_span(&child_b.id).unwrap();

        // Both siblings reference the same parent
        assert_eq!(span_a.parent_request_id_bytes, root_id.id);
        assert_eq!(span_b.parent_request_id_bytes, root_id.id);
        // Siblings have different span indices
        assert_ne!(span_a.span_index, span_b.span_index);
    }

    #[test]
    fn test_get_trace_returns_all_spans_in_order() {
        let env = make_env();
        env.ledger().set(LedgerInfo {
            timestamp: 100,
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
        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &attestor, &attestor, &sk);

        let root_id = client.generate_request_id();
        client.submit_with_request_id(
            &root_id,
            &attestor,
            &subject,
            &1000u64,
            &payload(&env, 0x04),
            &Bytes::new(&env),
        );

        let child1 = client.generate_request_id();
        let child2 = client.generate_request_id();

        client.propagate_span(&root_id, &child1, &String::from_str(&env, "op1"), &attestor);
        client.propagate_span(&root_id, &child2, &String::from_str(&env, "op2"), &attestor);

        let trace = client.get_trace(&root_id.id);
        assert_eq!(trace.len(), 3);
        // First span is root (span_index 0)
        assert_eq!(trace.get(0).unwrap().span_index, 0);
        assert_eq!(trace.get(1).unwrap().span_index, 1);
        assert_eq!(trace.get(2).unwrap().span_index, 2);
    }

    #[test]
    fn test_structured_log_format_includes_parent_request_id() {
        let env = make_env();
        env.ledger().set(LedgerInfo {
            timestamp: 200,
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
        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &attestor, &attestor, &sk);

        let root_id = client.generate_request_id();
        client.submit_with_request_id(
            &root_id,
            &attestor,
            &subject,
            &1000u64,
            &payload(&env, 0x05),
            &Bytes::new(&env),
        );

        let child_id = client.generate_request_id();
        client.propagate_span(
            &root_id,
            &child_id,
            &String::from_str(&env, "sep6_deposit"),
            &attestor,
        );

        let child_span = client.get_tracing_span(&child_id.id).unwrap();
        // Structured log: parent_request_id_bytes is non-empty when span is a child
        assert!(!child_span.parent_request_id_bytes.is_empty());
        assert_eq!(
            child_span.parent_request_id_bytes,
            root_id.id,
            "structured log must include parent_request_id"
        );
        assert_eq!(child_span.operation, String::from_str(&env, "sep6_deposit"));
    }

    #[test]
    fn test_span_emits_request_id() {
        let env = make_env();
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
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);

        client.initialize(&admin);
        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &attestor, &attestor, &sk);

        let req_id = client.generate_request_id();
        let ph = payload(&env, 0x01);
        let real_sig = sign_payload(&env, &sk, &ph);
        client.submit_with_request_id(
            &req_id,
            &attestor,
            &subject,
            &1000u64,
            &ph,
            &real_sig,
        );

        let span = client.get_tracing_span(&req_id.id).unwrap();
        assert_eq!(span.request_id.id, req_id.id);
        assert_eq!(span.request_id.created_at, req_id.created_at);
    }

    #[test]
    fn test_span_emits_operation_metadata() {
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
        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &attestor, &attestor, &sk);

        let req_id = client.generate_request_id();
        let ph = payload(&env, 0x01);
        let real_sig = sign_payload(&env, &sk, &ph);
        client.submit_with_request_id(
            &req_id,
            &attestor,
            &subject,
            &1000u64,
            &ph,
            &real_sig,
        );

        let span = client.get_tracing_span(&req_id.id).unwrap();
        assert_eq!(span.operation, String::from_str(&env, "submit_attestation"));
        assert_eq!(span.actor, attestor);
        assert_eq!(span.started_at, 1000);
        assert_eq!(span.completed_at, 1000);
        assert_eq!(span.status, String::from_str(&env, "success"));
    }

    #[test]
    fn test_structured_log_format() {
        let env = make_env();
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
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);

        client.initialize(&admin);
        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &attestor, &attestor, &sk);

        let req_id = client.generate_request_id();
        let ph = payload(&env, 0x01);
        let real_sig = sign_payload(&env, &sk, &ph);
        client.submit_with_request_id(
            &req_id,
            &attestor,
            &subject,
            &1000u64,
            &ph,
            &real_sig,
        );

        let span = client.get_tracing_span(&req_id.id).unwrap();
        assert_eq!(span.request_id.id, req_id.id);
        assert_eq!(span.operation, String::from_str(&env, "submit_attestation"));
        assert_eq!(span.actor, attestor);
        assert_eq!(span.status, String::from_str(&env, "success"));
    }

    // -----------------------------------------------------------------------
    // #241 — Span lineage validation
    // -----------------------------------------------------------------------

    fn setup_with_root_span(env: &Env) -> (AnchorKitContractClient<'_>, Address, crate::contract::RequestId) {
        env.ledger().set(LedgerInfo {
            timestamp: 2000,
            protocol_version: 21,
            sequence_number: 10,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(env, &contract_id);
        let admin = Address::generate(env);
        let attestor = Address::generate(env);
        let subject = Address::generate(env);
        client.initialize(&admin);
        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(env, &client, &attestor, &attestor, &sk);
        let root_id = client.generate_request_id();
        let ph = payload(env, 0xAA);
        let sig = sign_payload(env, &sk, &ph);
        client.submit_with_request_id(&root_id, &attestor, &subject, &2000u64, &ph, &sig);
        (client, attestor, root_id)
    }

    #[test]
    fn test_propagate_span_increments_index() {
        let env = make_env();
        let (client, attestor, root_id) = setup_with_root_span(&env);

        env.ledger().set(LedgerInfo {
            timestamp: 2001,
            protocol_version: 21,
            sequence_number: 11,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });
        let child_id = client.generate_request_id();
        client.propagate_span(
            &root_id,
            &child_id,
            &String::from_str(&env, "child_op"),
            &attestor,
        );

        let child_span = client.get_tracing_span(&child_id.id).unwrap();
        assert_eq!(child_span.span_index, 1);
        assert_eq!(child_span.parent_request_id_bytes, root_id.id);
    }

    #[test]
    fn test_get_trace_returns_spans_in_order() {
        let env = make_env();
        let (client, attestor, root_id) = setup_with_root_span(&env);

        env.ledger().set(LedgerInfo {
            timestamp: 2001,
            protocol_version: 21,
            sequence_number: 11,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });
        let child_id = client.generate_request_id();
        client.propagate_span(
            &root_id,
            &child_id,
            &String::from_str(&env, "child_op"),
            &attestor,
        );

        let spans = client.get_trace(&root_id.id);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans.get(0).unwrap().span_index, 0);
        assert_eq!(spans.get(1).unwrap().span_index, 1);
    }

    #[test]
    #[should_panic]
    fn test_propagate_span_rejects_missing_root_context() {
        // Calling propagate_span with a parent that has no TracingContext
        // (i.e. was never created via submit_with_request_id) must panic.
        let env = make_env();
        env.ledger().set(LedgerInfo {
            timestamp: 3000,
            protocol_version: 21,
            sequence_number: 20,
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

        let fake_parent = client.generate_request_id();
        env.ledger().set(LedgerInfo {
            timestamp: 3001,
            protocol_version: 21,
            sequence_number: 21,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });
        let child_id = client.generate_request_id();
        let actor = Address::generate(&env);
        // Must panic: no TracingContext exists for fake_parent
        client.propagate_span(
            &fake_parent,
            &child_id,
            &String::from_str(&env, "op"),
            &actor,
        );
    }

    #[test]
    #[should_panic]
    fn test_propagate_span_rejects_identical_parent_child_ids() {
        let env = make_env();
        let (client, attestor, root_id) = setup_with_root_span(&env);
        // Passing the same ID for both parent and child must panic.
        client.propagate_span(
            &root_id,
            &root_id,
            &String::from_str(&env, "op"),
            &attestor,
        );
    }

    // -----------------------------------------------------------------------
    // #243 — RequestContext validation
    // -----------------------------------------------------------------------

    #[test]
    #[should_panic]
    fn test_append_operation_empty_name_rejected() {
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
        client.initialize(&admin);
        let req_id = client.generate_request_id();
        // Empty operation name must panic with ValidationError
        client.append_operation(&req_id.id, &String::from_str(&env, ""));
    }

    #[test]
    #[should_panic]
    fn test_create_request_context_zero_timestamp_rejected() {
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
        client.initialize(&admin);
        let mut req_id = client.generate_request_id();
        // Force zero timestamp — must panic with InvalidTimestamp
        req_id.created_at = 0;
        client.create_request_context(&req_id);
    }
}

