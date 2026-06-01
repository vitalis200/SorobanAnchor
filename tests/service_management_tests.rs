//! Tests for anchor service enable/disable toggles and service rollback handling.
//!
//! These tests verify that:
//! 1. Services can be enabled/disabled individually
//! 2. Service state is tracked correctly
//! 3. Service configuration snapshots can be created
//! 4. Rollback to previous configurations works
//! 5. Multiple services can be managed together

#![cfg(test)]

mod service_management_tests {
    use soroban_sdk::testutils::{Address as _, Ledger, LedgerInfo};
    use soroban_sdk::{Address, Env, Vec};

    use anchorkit::service_management::{ServiceManager, ServiceToggleState, ServiceConfigSnapshot};

    // Service codes
    const SERVICE_DEPOSITS: u32 = 1;
    const SERVICE_WITHDRAWALS: u32 = 2;
    const SERVICE_QUOTES: u32 = 3;
    const SERVICE_KYC: u32 = 4;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

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
            max_entry_ttl: 6_312_000,
        });
    }

    fn create_service_vec(env: &Env, services: &[u32]) -> Vec<u32> {
        let mut vec = Vec::new(env);
        for service in services {
            vec.push_back(*service);
        }
        vec
    }

    // -----------------------------------------------------------------------
    // Service Enable/Disable Tests
    // -----------------------------------------------------------------------

    /// Test that a service can be enabled
    #[test]
    fn service_can_be_enabled() {
        let env = make_env();
        set_ledger(&env, 1000);

        let anchor = Address::generate(&env);

        let result = ServiceManager::enable_service(&env, &anchor, SERVICE_DEPOSITS);
        assert!(result);

        let state = ServiceManager::get_service_state(&env, &anchor);
        assert_eq!(state.enabled_services.len(), 1);
        assert_eq!(state.enabled_services.get(0).unwrap(), SERVICE_DEPOSITS);
    }

    /// Test that a service can be disabled
    #[test]
    fn service_can_be_disabled() {
        let env = make_env();
        set_ledger(&env, 1000);

        let anchor = Address::generate(&env);

        // Enable a service first
        ServiceManager::enable_service(&env, &anchor, SERVICE_DEPOSITS);

        // Disable it
        let result = ServiceManager::disable_service(&env, &anchor, SERVICE_DEPOSITS);
        assert!(result);

        let state = ServiceManager::get_service_state(&env, &anchor);
        assert_eq!(state.enabled_services.len(), 0);
        assert_eq!(state.disabled_services.len(), 1);
    }

    /// Test that enabling an already enabled service returns false
    #[test]
    fn enabling_already_enabled_service_returns_false() {
        let env = make_env();
        set_ledger(&env, 1000);

        let anchor = Address::generate(&env);

        ServiceManager::enable_service(&env, &anchor, SERVICE_DEPOSITS);
        let result = ServiceManager::enable_service(&env, &anchor, SERVICE_DEPOSITS);

        assert!(!result);
    }

    /// Test that disabling an already disabled service returns false
    #[test]
    fn disabling_already_disabled_service_returns_false() {
        let env = make_env();
        set_ledger(&env, 1000);

        let anchor = Address::generate(&env);

        ServiceManager::enable_service(&env, &anchor, SERVICE_DEPOSITS);
        ServiceManager::disable_service(&env, &anchor, SERVICE_DEPOSITS);
        let result = ServiceManager::disable_service(&env, &anchor, SERVICE_DEPOSITS);

        assert!(!result);
    }

    /// Test that multiple services can be enabled
    #[test]
    fn multiple_services_can_be_enabled() {
        let env = make_env();
        set_ledger(&env, 1000);

        let anchor = Address::generate(&env);

        ServiceManager::enable_service(&env, &anchor, SERVICE_DEPOSITS);
        ServiceManager::enable_service(&env, &anchor, SERVICE_WITHDRAWALS);
        ServiceManager::enable_service(&env, &anchor, SERVICE_QUOTES);

        let state = ServiceManager::get_service_state(&env, &anchor);
        assert_eq!(state.enabled_services.len(), 3);
    }

    /// Test that services can be selectively disabled
    #[test]
    fn services_can_be_selectively_disabled() {
        let env = make_env();
        set_ledger(&env, 1000);

        let anchor = Address::generate(&env);

        ServiceManager::enable_service(&env, &anchor, SERVICE_DEPOSITS);
        ServiceManager::enable_service(&env, &anchor, SERVICE_WITHDRAWALS);
        ServiceManager::enable_service(&env, &anchor, SERVICE_QUOTES);

        ServiceManager::disable_service(&env, &anchor, SERVICE_WITHDRAWALS);

        let state = ServiceManager::get_service_state(&env, &anchor);
        assert_eq!(state.enabled_services.len(), 2);
        assert_eq!(state.disabled_services.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Service Status Query Tests
    // -----------------------------------------------------------------------

    /// Test that service enabled status can be queried
    #[test]
    fn service_enabled_status_can_be_queried() {
        let env = make_env();
        set_ledger(&env, 1000);

        let anchor = Address::generate(&env);

        ServiceManager::enable_service(&env, &anchor, SERVICE_DEPOSITS);

        assert!(ServiceManager::is_service_enabled(&env, &anchor, SERVICE_DEPOSITS));
        assert!(!ServiceManager::is_service_enabled(&env, &anchor, SERVICE_WITHDRAWALS));
    }

    /// Test that disabled service returns false
    #[test]
    fn disabled_service_returns_false() {
        let env = make_env();
        set_ledger(&env, 1000);

        let anchor = Address::generate(&env);

        ServiceManager::enable_service(&env, &anchor, SERVICE_DEPOSITS);
        ServiceManager::disable_service(&env, &anchor, SERVICE_DEPOSITS);

        assert!(!ServiceManager::is_service_enabled(&env, &anchor, SERVICE_DEPOSITS));
    }

    // -----------------------------------------------------------------------
    // Snapshot Tests
    // -----------------------------------------------------------------------

    /// Test that a service configuration snapshot can be created
    #[test]
    fn service_configuration_snapshot_can_be_created() {
        let env = make_env();
        set_ledger(&env, 1000);

        let anchor = Address::generate(&env);
        let services = create_service_vec(&env, &[SERVICE_DEPOSITS, SERVICE_WITHDRAWALS]);

        let snapshot_id = ServiceManager::create_snapshot(
            &env,
            &anchor,
            &services,
            "initial_config",
        );

        assert_eq!(snapshot_id, 0);

        let snapshot = ServiceManager::get_snapshot(&env, snapshot_id).unwrap();
        assert_eq!(snapshot.snapshot_id, 0);
        assert_eq!(snapshot.anchor, anchor);
        assert_eq!(snapshot.services.len(), 2);
    }

    /// Test that multiple snapshots can be created
    #[test]
    fn multiple_snapshots_can_be_created() {
        let env = make_env();
        set_ledger(&env, 1000);

        let anchor = Address::generate(&env);
        let services1 = create_service_vec(&env, &[SERVICE_DEPOSITS]);
        let services2 = create_service_vec(&env, &[SERVICE_DEPOSITS, SERVICE_WITHDRAWALS]);

        let snap1 = ServiceManager::create_snapshot(&env, &anchor, &services1, "config_v1");
        let snap2 = ServiceManager::create_snapshot(&env, &anchor, &services2, "config_v2");

        assert_eq!(snap1, 0);
        assert_eq!(snap2, 1);

        let snapshot1 = ServiceManager::get_snapshot(&env, snap1).unwrap();
        let snapshot2 = ServiceManager::get_snapshot(&env, snap2).unwrap();

        assert_eq!(snapshot1.services.len(), 1);
        assert_eq!(snapshot2.services.len(), 2);
    }

    /// Test that snapshot includes timestamp
    #[test]
    fn snapshot_includes_timestamp() {
        let env = make_env();
        set_ledger(&env, 5000);

        let anchor = Address::generate(&env);
        let services = create_service_vec(&env, &[SERVICE_DEPOSITS]);

        let snapshot_id = ServiceManager::create_snapshot(&env, &anchor, &services, "test");
        let snapshot = ServiceManager::get_snapshot(&env, snapshot_id).unwrap();

        assert_eq!(snapshot.created_at, 5000);
    }

    /// Test that snapshot includes description
    #[test]
    fn snapshot_includes_description() {
        let env = make_env();
        set_ledger(&env, 1000);

        let anchor = Address::generate(&env);
        let services = create_service_vec(&env, &[SERVICE_DEPOSITS]);

        let snapshot_id = ServiceManager::create_snapshot(
            &env,
            &anchor,
            &services,
            "before_maintenance",
        );
        let snapshot = ServiceManager::get_snapshot(&env, snapshot_id).unwrap();

        assert_eq!(snapshot.description, soroban_sdk::String::from_small_str(&env, "before_maintenance"));
    }

    /// Test that snapshot count is tracked
    #[test]
    fn snapshot_count_is_tracked() {
        let env = make_env();
        set_ledger(&env, 1000);

        let anchor = Address::generate(&env);
        let services = create_service_vec(&env, &[SERVICE_DEPOSITS]);

        assert_eq!(ServiceManager::get_snapshot_count(&env), 0);

        ServiceManager::create_snapshot(&env, &anchor, &services, "snap1");
        assert_eq!(ServiceManager::get_snapshot_count(&env), 1);

        ServiceManager::create_snapshot(&env, &anchor, &services, "snap2");
        assert_eq!(ServiceManager::get_snapshot_count(&env), 2);
    }

    // -----------------------------------------------------------------------
    // Rollback Tests
    // -----------------------------------------------------------------------

    /// Test that rollback to a snapshot works
    #[test]
    fn rollback_to_snapshot_works() {
        let env = make_env();
        set_ledger(&env, 1000);

        let anchor = Address::generate(&env);
        let services = create_service_vec(&env, &[SERVICE_DEPOSITS, SERVICE_WITHDRAWALS]);

        // Create snapshot
        let snapshot_id = ServiceManager::create_snapshot(&env, &anchor, &services, "initial");

        // Change services
        ServiceManager::enable_service(&env, &anchor, SERVICE_QUOTES);
        ServiceManager::disable_service(&env, &anchor, SERVICE_WITHDRAWALS);

        let state_before = ServiceManager::get_service_state(&env, &anchor);
        assert_eq!(state_before.enabled_services.len(), 2); // DEPOSITS, QUOTES

        // Rollback
        let result = ServiceManager::rollback_to_snapshot(&env, snapshot_id);
        assert!(result);

        let state_after = ServiceManager::get_service_state(&env, &anchor);
        assert_eq!(state_after.enabled_services.len(), 2); // DEPOSITS, WITHDRAWALS
    }

    /// Test that rollback to non-existent snapshot returns false
    #[test]
    fn rollback_to_non_existent_snapshot_returns_false() {
        let env = make_env();
        set_ledger(&env, 1000);

        let anchor = Address::generate(&env);

        let result = ServiceManager::rollback_to_snapshot(&env, 999);
        assert!(!result);
    }

    /// Test that multiple rollbacks can be performed
    #[test]
    fn multiple_rollbacks_can_be_performed() {
        let env = make_env();
        set_ledger(&env, 1000);

        let anchor = Address::generate(&env);
        let services1 = create_service_vec(&env, &[SERVICE_DEPOSITS]);
        let services2 = create_service_vec(&env, &[SERVICE_DEPOSITS, SERVICE_WITHDRAWALS]);

        let snap1 = ServiceManager::create_snapshot(&env, &anchor, &services1, "config1");
        let snap2 = ServiceManager::create_snapshot(&env, &anchor, &services2, "config2");

        // Rollback to snap2
        ServiceManager::rollback_to_snapshot(&env, snap2);
        let state = ServiceManager::get_service_state(&env, &anchor);
        assert_eq!(state.enabled_services.len(), 2);

        // Rollback to snap1
        ServiceManager::rollback_to_snapshot(&env, snap1);
        let state = ServiceManager::get_service_state(&env, &anchor);
        assert_eq!(state.enabled_services.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Bulk Operations Tests
    // -----------------------------------------------------------------------

    /// Test that all services can be enabled at once
    #[test]
    fn all_services_can_be_enabled_at_once() {
        let env = make_env();
        set_ledger(&env, 1000);

        let anchor = Address::generate(&env);
        let all_services = create_service_vec(&env, &[
            SERVICE_DEPOSITS,
            SERVICE_WITHDRAWALS,
            SERVICE_QUOTES,
            SERVICE_KYC,
        ]);

        ServiceManager::enable_all_services(&env, &anchor, &all_services);

        let state = ServiceManager::get_service_state(&env, &anchor);
        assert_eq!(state.enabled_services.len(), 4);
        assert_eq!(state.disabled_services.len(), 0);
    }

    /// Test that all services can be disabled at once
    #[test]
    fn all_services_can_be_disabled_at_once() {
        let env = make_env();
        set_ledger(&env, 1000);

        let anchor = Address::generate(&env);
        let all_services = create_service_vec(&env, &[
            SERVICE_DEPOSITS,
            SERVICE_WITHDRAWALS,
            SERVICE_QUOTES,
            SERVICE_KYC,
        ]);

        ServiceManager::enable_all_services(&env, &anchor, &all_services);
        ServiceManager::disable_all_services(&env, &anchor, &all_services);

        let state = ServiceManager::get_service_state(&env, &anchor);
        assert_eq!(state.enabled_services.len(), 0);
        assert_eq!(state.disabled_services.len(), 4);
    }

    // -----------------------------------------------------------------------
    // State Persistence Tests
    // -----------------------------------------------------------------------

    /// Test that service state persists across queries
    #[test]
    fn service_state_persists_across_queries() {
        let env = make_env();
        set_ledger(&env, 1000);

        let anchor = Address::generate(&env);

        ServiceManager::enable_service(&env, &anchor, SERVICE_DEPOSITS);

        let state1 = ServiceManager::get_service_state(&env, &anchor);
        let state2 = ServiceManager::get_service_state(&env, &anchor);

        assert_eq!(state1.enabled_services.len(), 1);
        assert_eq!(state2.enabled_services.len(), 1);
    }

    /// Test that different anchors have independent service states
    #[test]
    fn different_anchors_have_independent_states() {
        let env = make_env();
        set_ledger(&env, 1000);

        let anchor1 = Address::generate(&env);
        let anchor2 = Address::generate(&env);

        ServiceManager::enable_service(&env, &anchor1, SERVICE_DEPOSITS);
        ServiceManager::enable_service(&env, &anchor2, SERVICE_WITHDRAWALS);

        let state1 = ServiceManager::get_service_state(&env, &anchor1);
        let state2 = ServiceManager::get_service_state(&env, &anchor2);

        assert_eq!(state1.enabled_services.len(), 1);
        assert_eq!(state2.enabled_services.len(), 1);
        assert_eq!(state1.enabled_services.get(0).unwrap(), SERVICE_DEPOSITS);
        assert_eq!(state2.enabled_services.get(0).unwrap(), SERVICE_WITHDRAWALS);
    }

    /// Test that service state includes update timestamp
    #[test]
    fn service_state_includes_update_timestamp() {
        let env = make_env();
        set_ledger(&env, 5000);

        let anchor = Address::generate(&env);

        ServiceManager::enable_service(&env, &anchor, SERVICE_DEPOSITS);

        let state = ServiceManager::get_service_state(&env, &anchor);
        assert_eq!(state.updated_at, 5000);
    }
}
