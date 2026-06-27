#![cfg(test)]

#[path = "sep10_test_util.rs"]
mod sep10_test_util;

mod capability_detection_tests {
    use soroban_sdk::{testutils::Address as _, Address, Env, Vec};

    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    use anchorkit::contract::{
        AnchorKitContract, AnchorKitContractClient, ServiceRetirementInfo, ServiceType,
        SERVICE_DEPOSITS, SERVICE_WITHDRAWALS, SERVICE_QUOTES, SERVICE_KYC,
        SERVICE_CAPABILITY_VERSION,
    };
    use crate::sep10_test_util::register_attestor_with_sep10;

    fn make_env() -> Env {
        let env = Env::default();
        env.mock_all_auths();
        env
    }

    fn setup(env: &Env) -> (AnchorKitContractClient, Address) {
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(env, &contract_id);
        let admin = Address::generate(env);
        client.initialize(&admin);
        (client, admin)
    }

    fn services(env: &Env, vals: &[u32]) -> Vec<u32> {
        let mut v = Vec::new(env);
        for &s in vals {
            v.push_back(s);
        }
        v
    }

    // -----------------------------------------------------------------------
    // ServiceType enum
    // -----------------------------------------------------------------------

    #[test]
    fn test_service_type_values() {
        assert_eq!(ServiceType::Deposits.as_u32(), SERVICE_DEPOSITS);
        assert_eq!(ServiceType::Withdrawals.as_u32(), SERVICE_WITHDRAWALS);
        assert_eq!(ServiceType::Quotes.as_u32(), SERVICE_QUOTES);
        assert_eq!(ServiceType::KYC.as_u32(), SERVICE_KYC);
        assert_eq!(SERVICE_DEPOSITS, 1u32);
        assert_eq!(SERVICE_WITHDRAWALS, 2u32);
        assert_eq!(SERVICE_QUOTES, 3u32);
        assert_eq!(SERVICE_KYC, 4u32);
    }

    // -----------------------------------------------------------------------
    // configure_services / get_supported_services / supports_service
    // -----------------------------------------------------------------------

    #[test]
    fn test_detect_deposit_only_anchor() {
        let env = make_env();
        let (client, _) = setup(&env);
        let anchor = Address::generate(&env);
        { let sk = SigningKey::generate(&mut OsRng); register_attestor_with_sep10(&env, &client, &anchor, &anchor, &sk); }

        client.configure_services(&anchor, &services(&env, &[SERVICE_DEPOSITS]));

        let record = client.get_supported_services(&anchor);
        assert_eq!(record.services.len(), 1);
        assert!(record.services.contains(&SERVICE_DEPOSITS));

        assert!(client.supports_service(&anchor, &SERVICE_DEPOSITS));
        assert!(!client.supports_service(&anchor, &SERVICE_WITHDRAWALS));
        assert!(!client.supports_service(&anchor, &SERVICE_QUOTES));
        assert!(!client.supports_service(&anchor, &SERVICE_KYC));
    }

    #[test]
    fn test_detect_withdrawal_only_anchor() {
        let env = make_env();
        let (client, _) = setup(&env);
        let anchor = Address::generate(&env);
        { let sk = SigningKey::generate(&mut OsRng); register_attestor_with_sep10(&env, &client, &anchor, &anchor, &sk); }

        client.configure_services(&anchor, &services(&env, &[SERVICE_WITHDRAWALS]));

        assert!(!client.supports_service(&anchor, &SERVICE_DEPOSITS));
        assert!(client.supports_service(&anchor, &SERVICE_WITHDRAWALS));
        assert!(!client.supports_service(&anchor, &SERVICE_QUOTES));
        assert!(!client.supports_service(&anchor, &SERVICE_KYC));
    }

    #[test]
    fn test_detect_quote_provider_anchor() {
        let env = make_env();
        let (client, _) = setup(&env);
        let anchor = Address::generate(&env);
        { let sk = SigningKey::generate(&mut OsRng); register_attestor_with_sep10(&env, &client, &anchor, &anchor, &sk); }

        client.configure_services(&anchor, &services(&env, &[SERVICE_QUOTES]));

        assert!(!client.supports_service(&anchor, &SERVICE_DEPOSITS));
        assert!(!client.supports_service(&anchor, &SERVICE_WITHDRAWALS));
        assert!(client.supports_service(&anchor, &SERVICE_QUOTES));
        assert!(!client.supports_service(&anchor, &SERVICE_KYC));
    }

    #[test]
    fn test_detect_full_service_anchor() {
        let env = make_env();
        let (client, _) = setup(&env);
        let anchor = Address::generate(&env);
        { let sk = SigningKey::generate(&mut OsRng); register_attestor_with_sep10(&env, &client, &anchor, &anchor, &sk); }

        client.configure_services(
            &anchor,
            &services(&env, &[SERVICE_DEPOSITS, SERVICE_WITHDRAWALS, SERVICE_QUOTES, SERVICE_KYC]),
        );

        assert!(client.supports_service(&anchor, &SERVICE_DEPOSITS));
        assert!(client.supports_service(&anchor, &SERVICE_WITHDRAWALS));
        assert!(client.supports_service(&anchor, &SERVICE_QUOTES));
        assert!(client.supports_service(&anchor, &SERVICE_KYC));
    }

    #[test]
    fn test_update_anchor_capabilities() {
        let env = make_env();
        let (client, _) = setup(&env);
        let anchor = Address::generate(&env);
        { let sk = SigningKey::generate(&mut OsRng); register_attestor_with_sep10(&env, &client, &anchor, &anchor, &sk); }

        // Initial: deposits only
        client.configure_services(&anchor, &services(&env, &[SERVICE_DEPOSITS]));
        assert!(client.supports_service(&anchor, &SERVICE_DEPOSITS));
        assert!(!client.supports_service(&anchor, &SERVICE_WITHDRAWALS));

        // Update: deposits + withdrawals
        client.configure_services(&anchor, &services(&env, &[SERVICE_DEPOSITS, SERVICE_WITHDRAWALS]));
        assert!(client.supports_service(&anchor, &SERVICE_DEPOSITS));
        assert!(client.supports_service(&anchor, &SERVICE_WITHDRAWALS));
    }

    // -----------------------------------------------------------------------
    // Validation: empty list rejected
    // -----------------------------------------------------------------------

    #[test]
    fn test_reject_empty_services() {
        let env = make_env();
        let (client, _) = setup(&env);
        let anchor = Address::generate(&env);
        { let sk = SigningKey::generate(&mut OsRng); register_attestor_with_sep10(&env, &client, &anchor, &anchor, &sk); }

        let result = client.try_configure_services(&anchor, &services(&env, &[]));
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // Validation: duplicate services rejected
    // -----------------------------------------------------------------------

    #[test]
    fn test_reject_duplicate_services() {
        let env = make_env();
        let (client, _) = setup(&env);
        let anchor = Address::generate(&env);
        { let sk = SigningKey::generate(&mut OsRng); register_attestor_with_sep10(&env, &client, &anchor, &anchor, &sk); }

        let result = client.try_configure_services(
            &anchor,
            &services(&env, &[SERVICE_DEPOSITS, SERVICE_DEPOSITS]),
        );
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // Validation: unregistered anchor rejected
    // -----------------------------------------------------------------------

    #[test]
    fn test_reject_unregistered_anchor_services() {
        let env = make_env();
        let (client, _) = setup(&env);
        let anchor = Address::generate(&env);
        // NOT registered

        let result = client.try_configure_services(&anchor, &services(&env, &[SERVICE_DEPOSITS]));
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // get_supported_services for non-configured anchor
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_services_for_non_configured_anchor() {
        let env = make_env();
        let (client, _) = setup(&env);
        let anchor = Address::generate(&env);
        { let sk = SigningKey::generate(&mut OsRng); register_attestor_with_sep10(&env, &client, &anchor, &anchor, &sk); }
        // No configure_services call

        let result = client.try_get_supported_services(&anchor);
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // Service capability versioning (#239)
    // -----------------------------------------------------------------------

    #[test]
    fn test_services_stored_with_current_version() {
        let env = make_env();
        let (client, _) = setup(&env);
        let anchor = Address::generate(&env);
        { let sk = SigningKey::generate(&mut OsRng); register_attestor_with_sep10(&env, &client, &anchor, &anchor, &sk); }

        client.configure_services(&anchor, &services(&env, &[SERVICE_DEPOSITS, SERVICE_QUOTES]));

        let record = client.get_supported_services(&anchor);
        assert_eq!(record.service_capability_version, SERVICE_CAPABILITY_VERSION);
        assert_eq!(
            client.get_service_capability_version(&anchor),
            SERVICE_CAPABILITY_VERSION
        );
        assert_eq!(
            client.current_capability_version(),
            SERVICE_CAPABILITY_VERSION
        );
    }

    #[test]
    fn test_configure_versioned_accepts_current_version() {
        let env = make_env();
        let (client, _) = setup(&env);
        let anchor = Address::generate(&env);
        { let sk = SigningKey::generate(&mut OsRng); register_attestor_with_sep10(&env, &client, &anchor, &anchor, &sk); }

        let empty_retirements: Vec<ServiceRetirementInfo> = Vec::new(&env);
        client.configure_services_versioned(
            &anchor,
            &services(&env, &[SERVICE_KYC]),
            &empty_retirements,
            &SERVICE_CAPABILITY_VERSION,
        );
        assert!(client.supports_service(&anchor, &SERVICE_KYC));
        assert_eq!(
            client.get_service_capability_version(&anchor),
            SERVICE_CAPABILITY_VERSION
        );
    }

    #[test]
    fn test_reject_unsupported_version_too_new() {
        let env = make_env();
        let (client, _) = setup(&env);
        let anchor = Address::generate(&env);
        { let sk = SigningKey::generate(&mut OsRng); register_attestor_with_sep10(&env, &client, &anchor, &anchor, &sk); }

        let too_new = SERVICE_CAPABILITY_VERSION + 1;
        let empty_retirements: Vec<ServiceRetirementInfo> = Vec::new(&env);
        let result = client.try_configure_services_versioned(
            &anchor,
            &services(&env, &[SERVICE_DEPOSITS]),
            &empty_retirements,
            &too_new,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_reject_zero_version() {
        let env = make_env();
        let (client, _) = setup(&env);
        let anchor = Address::generate(&env);
        { let sk = SigningKey::generate(&mut OsRng); register_attestor_with_sep10(&env, &client, &anchor, &anchor, &sk); }

        let empty_retirements: Vec<ServiceRetirementInfo> = Vec::new(&env);
        let result = client.try_configure_services_versioned(
            &anchor,
            &services(&env, &[SERVICE_DEPOSITS]),
            &empty_retirements,
            &0u32,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_reject_unknown_service_code() {
        let env = make_env();
        let (client, _) = setup(&env);
        let anchor = Address::generate(&env);
        { let sk = SigningKey::generate(&mut OsRng); register_attestor_with_sep10(&env, &client, &anchor, &anchor, &sk); }

        // 99 is outside the recognised code range for the current version.
        let result = client.try_configure_services(&anchor, &services(&env, &[99u32]));
        assert!(result.is_err());

        // A mix of known + unknown is rejected wholesale.
        let result2 = client.try_configure_services(&anchor, &services(&env, &[SERVICE_DEPOSITS, 99u32]));
        assert!(result2.is_err());
    }

    /// All currently-defined service codes remain configurable and queryable
    /// under the current version (backwards compatibility of the code set).
    #[test]
    fn test_existing_codes_continue_working() {
        let env = make_env();
        let (client, _) = setup(&env);
        let anchor = Address::generate(&env);
        { let sk = SigningKey::generate(&mut OsRng); register_attestor_with_sep10(&env, &client, &anchor, &anchor, &sk); }

        client.configure_services(
            &anchor,
            &services(&env, &[SERVICE_DEPOSITS, SERVICE_WITHDRAWALS, SERVICE_QUOTES, SERVICE_KYC]),
        );
        assert!(client.supports_service(&anchor, &SERVICE_DEPOSITS));
        assert!(client.supports_service(&anchor, &SERVICE_WITHDRAWALS));
        assert!(client.supports_service(&anchor, &SERVICE_QUOTES));
        assert!(client.supports_service(&anchor, &SERVICE_KYC));
        assert_eq!(
            client.get_service_capability_version(&anchor),
            SERVICE_CAPABILITY_VERSION
        );
    }

    // -----------------------------------------------------------------------
    // Deterministic sorting (#258)
    // -----------------------------------------------------------------------

    #[test]
    fn test_services_sorted_deterministically() {
        let env = make_env();
        let (client, _) = setup(&env);
        let anchor = Address::generate(&env);
        { let sk = SigningKey::generate(&mut OsRng); register_attestor_with_sep10(&env, &client, &anchor, &anchor, &sk); }

        // Submit services in reverse order
        client.configure_services(
            &anchor,
            &services(&env, &[SERVICE_KYC, SERVICE_QUOTES, SERVICE_WITHDRAWALS, SERVICE_DEPOSITS]),
        );

        // Verify they are stored in sorted order
        let record = client.get_supported_services(&anchor);
        assert_eq!(record.services.len(), 4);
        assert_eq!(record.services.get(0).unwrap(), SERVICE_DEPOSITS);
        assert_eq!(record.services.get(1).unwrap(), SERVICE_WITHDRAWALS);
        assert_eq!(record.services.get(2).unwrap(), SERVICE_QUOTES);
        assert_eq!(record.services.get(3).unwrap(), SERVICE_KYC);
    }

    #[test]
    fn test_services_sorted_regardless_of_input_order() {
        let env = make_env();
        let (client, _) = setup(&env);
        let anchor1 = Address::generate(&env);
        let anchor2 = Address::generate(&env);
        { 
            let sk1 = SigningKey::generate(&mut OsRng); 
            register_attestor_with_sep10(&env, &client, &anchor1, &anchor1, &sk1);
            let sk2 = SigningKey::generate(&mut OsRng);
            register_attestor_with_sep10(&env, &client, &anchor2, &anchor2, &sk2);
        }

        // Configure anchor1 with services in one order
        client.configure_services(
            &anchor1,
            &services(&env, &[SERVICE_QUOTES, SERVICE_DEPOSITS, SERVICE_WITHDRAWALS]),
        );

        // Configure anchor2 with same services in different order
        client.configure_services(
            &anchor2,
            &services(&env, &[SERVICE_WITHDRAWALS, SERVICE_QUOTES, SERVICE_DEPOSITS]),
        );

        // Both should have identical sorted service lists
        let record1 = client.get_supported_services(&anchor1);
        let record2 = client.get_supported_services(&anchor2);
        
        assert_eq!(record1.services.len(), record2.services.len());
        for i in 0..record1.services.len() {
            assert_eq!(record1.services.get(i).unwrap(), record2.services.get(i).unwrap());
        }
        
        // Verify the sorted order
        assert_eq!(record1.services.get(0).unwrap(), SERVICE_DEPOSITS);
        assert_eq!(record1.services.get(1).unwrap(), SERVICE_WITHDRAWALS);
        assert_eq!(record1.services.get(2).unwrap(), SERVICE_QUOTES);
    }

    #[test]
    fn test_single_service_remains_unchanged() {
        let env = make_env();
        let (client, _) = setup(&env);
        let anchor = Address::generate(&env);
        { let sk = SigningKey::generate(&mut OsRng); register_attestor_with_sep10(&env, &client, &anchor, &anchor, &sk); }

        client.configure_services(&anchor, &services(&env, &[SERVICE_QUOTES]));

        let record = client.get_supported_services(&anchor);
        assert_eq!(record.services.len(), 1);
        assert_eq!(record.services.get(0).unwrap(), SERVICE_QUOTES);
    }

    #[test]
    fn test_two_services_sorted() {
        let env = make_env();
        let (client, _) = setup(&env);
        let anchor = Address::generate(&env);
        { let sk = SigningKey::generate(&mut OsRng); register_attestor_with_sep10(&env, &client, &anchor, &anchor, &sk); }

        // Submit in reverse order
        client.configure_services(&anchor, &services(&env, &[SERVICE_WITHDRAWALS, SERVICE_DEPOSITS]));

        let record = client.get_supported_services(&anchor);
        assert_eq!(record.services.len(), 2);
        assert_eq!(record.services.get(0).unwrap(), SERVICE_DEPOSITS);
        assert_eq!(record.services.get(1).unwrap(), SERVICE_WITHDRAWALS);
    }

    #[test]
    fn test_reconfigure_maintains_sorting() {
        let env = make_env();
        let (client, _) = setup(&env);
        let anchor = Address::generate(&env);
        { let sk = SigningKey::generate(&mut OsRng); register_attestor_with_sep10(&env, &client, &anchor, &anchor, &sk); }

        // Initial configuration in random order
        client.configure_services(&anchor, &services(&env, &[SERVICE_QUOTES, SERVICE_DEPOSITS]));
        
        let record1 = client.get_supported_services(&anchor);
        assert_eq!(record1.services.get(0).unwrap(), SERVICE_DEPOSITS);
        assert_eq!(record1.services.get(1).unwrap(), SERVICE_QUOTES);

        // Reconfigure with different services in random order
        client.configure_services(&anchor, &services(&env, &[SERVICE_KYC, SERVICE_WITHDRAWALS, SERVICE_DEPOSITS]));
        
        let record2 = client.get_supported_services(&anchor);
        assert_eq!(record2.services.len(), 3);
        assert_eq!(record2.services.get(0).unwrap(), SERVICE_DEPOSITS);
        assert_eq!(record2.services.get(1).unwrap(), SERVICE_WITHDRAWALS);
        assert_eq!(record2.services.get(2).unwrap(), SERVICE_KYC);
    }

    // -----------------------------------------------------------------------
    // configure_services_versioned — service-code validation (#ticket)
    //
    // These tests exercise the versioned entry-point directly so that any
    // future divergence in the validation path between configure_services and
    // configure_services_versioned is caught immediately.
    // -----------------------------------------------------------------------

    /// Service code 0 is reserved / invalid and must be rejected with
    /// `InvalidServiceType` regardless of which entry-point is used.
    #[test]
    fn test_configure_services_versioned_rejects_zero_code() {
        let env = make_env();
        let (client, _) = setup(&env);
        let anchor = Address::generate(&env);
        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &anchor, &anchor, &sk);

        let empty_retirements: Vec<ServiceRetirementInfo> = Vec::new(&env);
        let result = client.try_configure_services_versioned(
            &anchor,
            &services(&env, &[0u32]),
            &empty_retirements,
            &SERVICE_CAPABILITY_VERSION,
        );
        assert!(result.is_err(), "service code 0 must be rejected");
    }

    /// Any service code above MAX_KNOWN_SERVICE_CODE (currently 4) must be
    /// rejected with `InvalidServiceType`.
    #[test]
    fn test_configure_services_versioned_rejects_unknown_code() {
        let env = make_env();
        let (client, _) = setup(&env);
        let anchor = Address::generate(&env);
        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &anchor, &anchor, &sk);

        let empty_retirements: Vec<ServiceRetirementInfo> = Vec::new(&env);

        // Code 99 is well outside the known range.
        let result = client.try_configure_services_versioned(
            &anchor,
            &services(&env, &[99u32]),
            &empty_retirements,
            &SERVICE_CAPABILITY_VERSION,
        );
        assert!(result.is_err(), "service code 99 must be rejected");

        // A mix of a known code and an unknown code must also be rejected
        // wholesale — partial acceptance would silently drop the unknown entry.
        let result2 = client.try_configure_services_versioned(
            &anchor,
            &services(&env, &[SERVICE_DEPOSITS, 99u32]),
            &empty_retirements,
            &SERVICE_CAPABILITY_VERSION,
        );
        assert!(result2.is_err(), "mixed known/unknown codes must be rejected");
    }

    /// Duplicate service codes within a single call must be rejected so that
    /// the stored set is always a true set (no repeated entries).
    #[test]
    fn test_configure_services_versioned_rejects_duplicate_codes() {
        let env = make_env();
        let (client, _) = setup(&env);
        let anchor = Address::generate(&env);
        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &anchor, &anchor, &sk);

        let empty_retirements: Vec<ServiceRetirementInfo> = Vec::new(&env);
        let result = client.try_configure_services_versioned(
            &anchor,
            &services(&env, &[SERVICE_DEPOSITS, SERVICE_DEPOSITS]),
            &empty_retirements,
            &SERVICE_CAPABILITY_VERSION,
        );
        assert!(result.is_err(), "duplicate service codes must be rejected");
    }

    /// An empty service list must be rejected — configuring zero services is
    /// meaningless and likely a caller error.
    #[test]
    fn test_configure_services_versioned_rejects_empty_list() {
        let env = make_env();
        let (client, _) = setup(&env);
        let anchor = Address::generate(&env);
        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &anchor, &anchor, &sk);

        let empty_retirements: Vec<ServiceRetirementInfo> = Vec::new(&env);
        let result = client.try_configure_services_versioned(
            &anchor,
            &services(&env, &[]),
            &empty_retirements,
            &SERVICE_CAPABILITY_VERSION,
        );
        assert!(result.is_err(), "empty service list must be rejected");
    }
}

