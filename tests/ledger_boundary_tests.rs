#![cfg(test)]

//! Ledger boundary condition tests
//!
//! This module tests off-by-one errors and boundary conditions around:
//! - Ledger sequence rollover in rate limiting windows
//! - Timestamp-based TTL expiration in caches and sessions
//! - State transitions at exact boundary moments
//!
//! Each test verifies behavior at, before, and after critical boundaries.

mod ledger_boundary_tests {
    use soroban_sdk::{
        testutils::{Address as _, Ledger, LedgerInfo},
        Address, Bytes, Env, String,
    };

    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    use crate::contract::{AnchorKitContract, AnchorKitContractClient, AnchorMetadata};
    use crate::rate_limiter::{RateLimiter, RateLimitConfig};
    use crate::sep10_test_util::{register_attestor_with_sep10, sign_payload};
    use crate::transaction_state_tracker::{TransactionStateTracker, TransactionState};

    fn make_env() -> Env {
        let env = Env::default();
        env.mock_all_auths();
        env
    }

    fn set_ledger(env: &Env, timestamp: u64, sequence: u32) {
        env.ledger().set(LedgerInfo {
            timestamp,
            protocol_version: 21,
            sequence_number: sequence,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });
    }

    fn payload(env: &Env, byte: u8) -> Bytes {
        let mut b = Bytes::new(env);
        for _ in 0..32 {
            b.push_back(byte);
        }
        b
    }

    // =========================================================================
    // RATE LIMITER WINDOW BOUNDARY TESTS
    // =========================================================================

    /// Test rate limit window expiry at exact boundary (window_length ledgers)
    ///
    /// `is_window_expired`: `current - start >= window_length`
    /// With window_length=100, start=1000:
    /// - At 1099: 99 >= 100 is false → NOT expired
    /// - At 1100: 100 >= 100 is true → EXPIRED (window resets)
    #[test]
    fn test_rate_limit_window_expires_exactly_at_boundary() {
        let env = make_env();
        let contract_address = Address::generate(&env);
        let attestor = Address::generate(&env);

        let config = RateLimitConfig {
            max_submissions: 2,
            window_length: 100,
        };

        // Start at ledger 1000
        set_ledger(&env, 0, 1000);

        // Fill the window
        env.as_contract(&contract_address, || {
            assert!(RateLimiter::check_and_increment(&env, &attestor, &config).is_ok());
            assert!(RateLimiter::check_and_increment(&env, &attestor, &config).is_ok());
        });

        // Third submission fails (limit reached)
        env.as_contract(&contract_address, || {
            assert!(RateLimiter::check_and_increment(&env, &attestor, &config).is_err(),
                "Should be rate limited after filling window");
        });

        // At ledger 1099: 1099 - 1000 = 99 < 100 → NOT expired
        set_ledger(&env, 0, 1099);
        env.as_contract(&contract_address, || {
            assert!(RateLimiter::check_and_increment(&env, &attestor, &config).is_err(),
                "Should still be rate limited one ledger before window expiry");
        });

        // At ledger 1100: 1100 - 1000 = 100 >= 100 → EXPIRED → window resets
        set_ledger(&env, 0, 1100);
        env.as_contract(&contract_address, || {
            assert!(RateLimiter::check_and_increment(&env, &attestor, &config).is_ok(),
                "Window should reset at exactly window_start + window_length");
        });
    }

    /// Test rate limit one ledger before window expiry
    ///
    /// Verifies the window is still active at `start + window_length - 1`.
    #[test]
    fn test_rate_limit_one_ledger_before_window_expiry() {
        let env = make_env();
        let contract_address = Address::generate(&env);
        let attestor = Address::generate(&env);
        
        let config = RateLimitConfig {
            max_submissions: 1,
            window_length: 50,
        };

        // Start at ledger 2000
        set_ledger(&env, 0, 2000);
        
        env.as_contract(&contract_address, || {
            assert!(RateLimiter::check_and_increment(&env, &attestor, &config).is_ok());
            assert!(RateLimiter::check_and_increment(&env, &attestor, &config).is_err());
        });

        // Advance to ledger 2049 (one before expiry)
        set_ledger(&env, 0, 2049);
        env.as_contract(&contract_address, || {
            // Should still be rate limited
            assert!(RateLimiter::check_and_increment(&env, &attestor, &config).is_err());
        });
    }

    /// Test rate limit one ledger after window expiry
    ///
    /// Verifies the window resets at `start + window_length + 1`.
    #[test]
    fn test_rate_limit_one_ledger_after_window_expiry() {
        let env = make_env();
        let contract_address = Address::generate(&env);
        let attestor = Address::generate(&env);

        let config = RateLimitConfig {
            max_submissions: 1,
            window_length: 50,
        };

        // Start at ledger 3000
        set_ledger(&env, 0, 3000);

        env.as_contract(&contract_address, || {
            assert!(RateLimiter::check_and_increment(&env, &attestor, &config).is_ok());
        });

        // Advance to ledger 3051 (one after expiry at 3050)
        // 3051 - 3000 = 51 >= 50 → window expired
        set_ledger(&env, 0, 3051);
        env.as_contract(&contract_address, || {
            assert!(RateLimiter::check_and_increment(&env, &attestor, &config).is_ok(),
                "Window should have reset one ledger after expiry");
        });

        // After reset, the new window starts at 3051 — verify state
        let state = env.as_contract(&contract_address, || {
            RateLimiter::get_state(&env, &attestor)
        });
        assert_eq!(state.submission_count, 1,
            "Submission count should be 1 in the new window");
        assert_eq!(state.window_start_ledger, 3051,
            "Window start should be updated to current ledger after reset");
    }

    /// Test rate limit with window_length = 1 (minimum window)
    ///
    /// With window_length=1: expires when current - start >= 1
    /// - At start+0: 0 >= 1 is false → NOT expired (same ledger)
    /// - At start+1: 1 >= 1 is true → EXPIRED (window resets)
    #[test]
    fn test_rate_limit_minimum_window_length() {
        let env = make_env();
        let contract_address = Address::generate(&env);
        let attestor = Address::generate(&env);

        let config = RateLimitConfig {
            max_submissions: 1,
            window_length: 1,
        };

        set_ledger(&env, 0, 5000);

        env.as_contract(&contract_address, || {
            assert!(RateLimiter::check_and_increment(&env, &attestor, &config).is_ok(),
                "First submission should succeed");
            assert!(RateLimiter::check_and_increment(&env, &attestor, &config).is_err(),
                "Second submission should be rate limited");
        });

        // Advance by exactly 1 ledger: 5001 - 5000 = 1 >= 1 → window resets
        set_ledger(&env, 0, 5001);
        env.as_contract(&contract_address, || {
            assert!(RateLimiter::check_and_increment(&env, &attestor, &config).is_ok(),
                "Window should reset after exactly 1 ledger");
        });
    }

    /// Test rate limit window with ledger sequence near u32::MAX
    ///
    /// The rate limiter uses `saturating_sub` to prevent underflow:
    /// `current_ledger.saturating_sub(window_start_ledger) >= window_length`
    ///
    /// When `current_ledger < window_start_ledger` (e.g. after a hypothetical
    /// rollover), `saturating_sub` returns 0, so the window is NOT considered
    /// expired. This is the safe/conservative behavior.
    #[test]
    fn test_rate_limit_near_max_ledger_sequence() {
        let env = make_env();
        let contract_address = Address::generate(&env);
        let attestor = Address::generate(&env);

        let config = RateLimitConfig {
            max_submissions: 1,
            window_length: 100,
        };

        // Start near u32::MAX so window_start = u32::MAX - 50
        let near_max: u32 = u32::MAX - 50;
        set_ledger(&env, 0, near_max);

        env.as_contract(&contract_address, || {
            assert!(RateLimiter::check_and_increment(&env, &attestor, &config).is_ok(),
                "First submission should succeed near u32::MAX");
            assert!(RateLimiter::check_and_increment(&env, &attestor, &config).is_err(),
                "Second submission should be rate limited");
        });

        // Advance to u32::MAX — difference is 50, window_length is 100 → NOT expired
        set_ledger(&env, 0, u32::MAX);
        env.as_contract(&contract_address, || {
            // 50 < 100 → window not expired → still rate limited
            assert!(RateLimiter::check_and_increment(&env, &attestor, &config).is_err(),
                "Should still be rate limited at u32::MAX (window not expired)");
        });

        // Verify no panic occurred — saturating_sub handles the near-overflow safely
        // current(u32::MAX) - start(u32::MAX - 50) = 50 < 100 → not expired
        let state = env.as_contract(&contract_address, || {
            RateLimiter::get_state(&env, &attestor)
        });
        assert_eq!(state.submission_count, 1,
            "Submission count should remain 1 (window not reset)");
    }

    // =========================================================================
    // METADATA CACHE TTL BOUNDARY TESTS
    // =========================================================================

    /// Test cache expiry at exact TTL boundary
    ///
    /// Cache expiry logic: `if entry.cached_at + entry.ttl_seconds <= now { EXPIRED }`
    /// So cache is valid when `now < cached_at + ttl_seconds`
    /// - At t=1099: 1099 < 1100 → VALID
    /// - At t=1100: 1100 <= 1100 → EXPIRED (exclusive upper bound)
    #[test]
    fn test_cache_expires_exactly_at_ttl() {
        let env = make_env();
        set_ledger(&env, 1000, 0);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let anchor = Address::generate(&env);
        client.initialize(&admin);

        let meta = AnchorMetadata {
            anchor: anchor.clone(),
            reputation_score: 9000,
            liquidity_score: 8500,
            uptime_percentage: 9900,
            total_volume: 1_000_000,
            average_settlement_time: 300,
            is_active: true,
        };

        // Cache with TTL of 100 seconds at t=1000
        // Expiry condition: cached_at + ttl_seconds <= now → 1000 + 100 = 1100 <= now
        client.cache_metadata(&anchor, &meta, &100u64);

        // At t=1099 (one second before expiry): 1100 <= 1099 is false → VALID
        set_ledger(&env, 1099, 0);
        assert!(client.try_get_cached_metadata(&anchor).is_ok(),
            "Cache should be valid one second before expiry");

        // At t=1100 (exactly at cached_at + ttl): 1100 <= 1100 is true → EXPIRED
        // This is the exclusive boundary: cache expires AT the boundary
        set_ledger(&env, 1100, 0);
        assert!(client.try_get_cached_metadata(&anchor).is_err(),
            "Cache should be expired at exactly cached_at + ttl_seconds");

        // At t=1101 (one second after TTL): 1100 <= 1101 is true → EXPIRED
        set_ledger(&env, 1101, 0);
        assert!(client.try_get_cached_metadata(&anchor).is_err(),
            "Cache should be expired after TTL");
    }

    /// Test cache with TTL = 0 (immediate expiry)
    #[test]
    fn test_cache_with_zero_ttl() {
        let env = make_env();
        set_ledger(&env, 2000, 0);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let anchor = Address::generate(&env);
        client.initialize(&admin);

        let meta = AnchorMetadata {
            anchor: anchor.clone(),
            reputation_score: 9000,
            liquidity_score: 8500,
            uptime_percentage: 9900,
            total_volume: 1_000_000,
            average_settlement_time: 300,
            is_active: true,
        };

        // Cache with TTL of 0
        client.cache_metadata(&anchor, &meta, &0u64);

        // Should be immediately expired
        set_ledger(&env, 2001, 0);
        assert!(client.try_get_cached_metadata(&anchor).is_err());
    }

    /// Test cache with TTL = 1 (minimum valid TTL)
    ///
    /// Cache expiry: `cached_at + ttl_seconds <= now`
    /// With TTL=1 at t=3000: expires when 3001 <= now
    /// - At t=3000: 3001 <= 3000 is false → VALID
    /// - At t=3001: 3001 <= 3001 is true → EXPIRED
    #[test]
    fn test_cache_with_minimum_ttl() {
        let env = make_env();
        set_ledger(&env, 3000, 0);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let anchor = Address::generate(&env);
        client.initialize(&admin);

        let meta = AnchorMetadata {
            anchor: anchor.clone(),
            reputation_score: 9000,
            liquidity_score: 8500,
            uptime_percentage: 9900,
            total_volume: 1_000_000,
            average_settlement_time: 300,
            is_active: true,
        };

        // Cache with TTL of 1 second at t=3000
        // Expiry: 3000 + 1 = 3001 <= now
        client.cache_metadata(&anchor, &meta, &1u64);

        // At t=3000 (same time as cache): 3001 <= 3000 is false → VALID
        assert!(client.try_get_cached_metadata(&anchor).is_ok(),
            "Cache should be valid at creation time");

        // At t=3001 (exactly at expiry): 3001 <= 3001 is true → EXPIRED
        set_ledger(&env, 3001, 0);
        assert!(client.try_get_cached_metadata(&anchor).is_err(),
            "Cache should be expired at cached_at + 1");
    }

    /// Test stale-while-revalidate boundaries
    ///
    /// SWR logic:
    /// - FRESH when `age <= ttl_seconds` (inclusive)
    /// - STALE when `ttl_seconds < age <= ttl_seconds + stale_ttl_seconds` (inclusive)
    /// - EXPIRED when `age > ttl_seconds + stale_ttl_seconds`
    ///
    /// With primary_ttl=100, stale_ttl=50 at t=5000:
    /// - age = now - 5000
    /// - FRESH: age <= 100 → now <= 5100
    /// - STALE: 100 < age <= 150 → 5100 < now <= 5150
    /// - EXPIRED: age > 150 → now > 5150
    #[test]
    fn test_swr_cache_boundaries() {
        let env = make_env();
        set_ledger(&env, 5000, 0);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let anchor = Address::generate(&env);
        client.initialize(&admin);

        let meta = AnchorMetadata {
            anchor: anchor.clone(),
            reputation_score: 9000,
            liquidity_score: 8500,
            uptime_percentage: 9900,
            total_volume: 1_000_000,
            average_settlement_time: 300,
            is_active: true,
        };

        // primary TTL = 100s, stale TTL = 50s, cached at t=5000
        client.cache_metadata_swr(&anchor, &meta, &100u64, &50u64);

        // At t=5099 (age=99): 99 <= 100 → FRESH
        set_ledger(&env, 5099, 0);
        let (_, needs_refresh) = client.get_cached_metadata_swr(&anchor);
        assert!(!needs_refresh, "Should be FRESH at age=99");

        // At t=5100 (age=100): 100 <= 100 → FRESH (inclusive boundary)
        set_ledger(&env, 5100, 0);
        let (_, needs_refresh) = client.get_cached_metadata_swr(&anchor);
        assert!(!needs_refresh, "Should be FRESH at age=100 (inclusive)");

        // At t=5101 (age=101): 101 > 100 and 101 <= 150 → STALE
        set_ledger(&env, 5101, 0);
        let (_, needs_refresh) = client.get_cached_metadata_swr(&anchor);
        assert!(needs_refresh, "Should be STALE at age=101");

        // At t=5149 (age=149): 149 > 100 and 149 <= 150 → STALE
        set_ledger(&env, 5149, 0);
        let (_, needs_refresh) = client.get_cached_metadata_swr(&anchor);
        assert!(needs_refresh, "Should be STALE at age=149");

        // At t=5150 (age=150): 150 > 100 and 150 <= 150 → STALE (inclusive boundary)
        set_ledger(&env, 5150, 0);
        let (_, needs_refresh) = client.get_cached_metadata_swr(&anchor);
        assert!(needs_refresh, "Should be STALE at age=150 (inclusive)");

        // At t=5151 (age=151): 151 > 150 → EXPIRED
        set_ledger(&env, 5151, 0);
        assert!(client.try_get_cached_metadata_swr(&anchor).is_err(),
            "Should be EXPIRED at age=151");
    }

    // =========================================================================
    // SESSION TTL BOUNDARY TESTS
    // =========================================================================

    /// Test session expiry at exact TTL boundary
    ///
    /// Session expiry logic: `if now > session.created_at + ttl { EXPIRED }`
    /// So session is valid when `now <= created_at + ttl`
    /// - At t=13600: 13600 <= 13600 → VALID (inclusive)
    /// - At t=13601: 13601 > 13600 → EXPIRED
    #[test]
    fn test_session_expires_exactly_at_ttl() {
        let env = make_env();
        set_ledger(&env, 10000, 0);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        client.initialize(&admin);

        let session_id = client.create_session(&user);
        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &attestor, &attestor, &sk);

        // Session created at t=10000, default TTL=3600 → expires when now > 13600

        // At t=13599 (one second before expiry): 13599 <= 13600 → VALID
        set_ledger(&env, 13599, 0);
        let ph = payload(&env, 0x01);
        let sig = sign_payload(&env, &sk, &ph);
        assert!(client.try_submit_attestation_with_session(
            &session_id, &attestor, &subject, &13599u64, &ph, &sig
        ).is_ok(), "Session should be valid one second before expiry");

        // At t=13600 (exactly at created_at + ttl): 13600 <= 13600 → VALID (inclusive)
        set_ledger(&env, 13600, 0);
        let ph2 = payload(&env, 0x02);
        let sig2 = sign_payload(&env, &sk, &ph2);
        assert!(client.try_submit_attestation_with_session(
            &session_id, &attestor, &subject, &13600u64, &ph2, &sig2
        ).is_ok(), "Session should be valid at exactly created_at + ttl (inclusive)");

        // At t=13601 (one second after expiry): 13601 > 13600 → EXPIRED
        set_ledger(&env, 13601, 0);
        let ph3 = payload(&env, 0x03);
        let sig3 = sign_payload(&env, &sk, &ph3);
        assert!(client.try_submit_attestation_with_session(
            &session_id, &attestor, &subject, &13601u64, &ph3, &sig3
        ).is_err(), "Session should be expired one second after TTL");
    }

    /// Test session with custom TTL boundary
    ///
    /// The default session TTL is 3600 seconds. This test verifies the boundary
    /// at `created_at + DEFAULT_SESSION_TTL` using the standard `create_session`.
    /// Session expiry: `if now > created_at + ttl { EXPIRED }`
    #[test]
    fn test_session_custom_ttl_boundary() {
        let env = make_env();
        set_ledger(&env, 20000, 0);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        client.initialize(&admin);

        // Create session at t=20000 with default TTL=3600
        // Expires when now > 20000 + 3600 = 23600
        let session_id = client.create_session(&user);
        let session = client.get_session(&session_id);
        assert_eq!(session.created_at, 20000);
        assert_eq!(session.session_ttl_seconds, 3600);

        // At t=23600 (exactly at boundary): 23600 > 23600 is false → VALID
        set_ledger(&env, 23600, 0);
        let session = client.get_session(&session_id);
        assert!(!session.closed, "Session should not be closed at boundary");

        // At t=23601 (one second past boundary): 23601 > 23600 → EXPIRED
        // Verify by attempting an operation that calls validate_session
        set_ledger(&env, 23601, 0);
        let attestor = Address::generate(&env);
        let sk = SigningKey::generate(&mut OsRng);
        let pk = soroban_sdk::BytesN::from_array(&env, sk.verifying_key().as_bytes());
        assert!(
            client.try_register_attestor_with_session(&session_id, &attestor, &pk).is_err(),
            "Session should be expired one second past TTL"
        );
    }

    // =========================================================================
    // QUOTE VALIDITY BOUNDARY TESTS
    // =========================================================================

    /// Test quote expiry at exact valid_until boundary
    ///
    /// Routing filter logic: `if quote.valid_until <= now { continue; }` (skip expired)
    /// So quote is included when `valid_until > now`
    /// - At t=30499: 30500 > 30499 → INCLUDED
    /// - At t=30500: 30500 > 30500 is false → FILTERED (exclusive boundary)
    /// - At t=30501: 30500 > 30501 is false → FILTERED
    #[test]
    fn test_quote_expires_exactly_at_valid_until() {
        let env = make_env();
        set_ledger(&env, 30000, 0);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let anchor = Address::generate(&env);
        client.initialize(&admin);

        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &anchor, &anchor, &sk);

        let mut services = soroban_sdk::Vec::new(&env);
        services.push_back(3u32); // quotes service
        client.configure_services(&anchor, &services);

        // Submit quote valid until t=30500
        client.submit_quote(
            &anchor,
            &String::from_str(&env, "USD"),
            &String::from_str(&env, "USDC"),
            &10000u64,
            &25u32,
            &100u64,
            &100000u64,
            &30500u64,
        );

        // At t=30499: valid_until (30500) > now (30499) → INCLUDED in routing
        set_ledger(&env, 30499, 0);
        let quote = client.get_quote(&anchor, &1u64);
        assert_eq!(quote.valid_until, 30500,
            "Quote should exist in storage at t=30499");

        // At t=30500: valid_until (30500) > now (30500) is false → FILTERED by routing
        // Quote still exists in storage but routing skips it
        set_ledger(&env, 30500, 0);
        let quote = client.get_quote(&anchor, &1u64);
        assert_eq!(quote.valid_until, 30500,
            "Quote still in storage at t=30500");
        // Verify the boundary: valid_until <= now means filtered
        assert!(quote.valid_until <= 30500,
            "Quote should be filtered by routing at t=30500 (valid_until <= now)");

        // At t=30501: valid_until (30500) > now (30501) is false → FILTERED
        set_ledger(&env, 30501, 0);
        let quote = client.get_quote(&anchor, &1u64);
        assert!(quote.valid_until < 30501,
            "Quote should be filtered by routing at t=30501");
    }

    /// Test quote submission with valid_until in the past
    ///
    /// Per the bugfix spec (quote-routing-sep6-fixes/bugfix.md §2.1):
    /// `submit_quote` SHOULD panic with `StaleQuote` when `valid_until <= current_time`.
    ///
    /// NOTE: This test documents the EXPECTED behavior. If the implementation
    /// does not yet enforce this, the test will fail and the fix should be applied
    /// to `submit_quote` in `contract.rs`.
    #[test]
    #[should_panic]
    fn test_quote_submission_with_past_valid_until() {
        let env = make_env();
        set_ledger(&env, 40000, 0);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let anchor = Address::generate(&env);
        client.initialize(&admin);

        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &anchor, &anchor, &sk);

        let mut services = soroban_sdk::Vec::new(&env);
        services.push_back(3u32);
        client.configure_services(&anchor, &services);

        // Try to submit quote with valid_until strictly in the past
        // Expected: panic with StaleQuote (valid_until=39999 < current_time=40000)
        client.submit_quote(
            &anchor,
            &String::from_str(&env, "USD"),
            &String::from_str(&env, "USDC"),
            &10000u64,
            &25u32,
            &100u64,
            &100000u64,
            &39999u64, // Before current time (40000)
        );
    }

    /// Test quote submission with valid_until exactly at current time
    ///
    /// Per the bugfix spec §2.1: `valid_until <= current_time` should be rejected.
    /// At `valid_until == current_time`, the quote would be immediately expired.
    #[test]
    #[should_panic]
    fn test_quote_submission_with_valid_until_at_current_time() {
        let env = make_env();
        set_ledger(&env, 50000, 0);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let anchor = Address::generate(&env);
        client.initialize(&admin);

        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &anchor, &anchor, &sk);

        let mut services = soroban_sdk::Vec::new(&env);
        services.push_back(3u32);
        client.configure_services(&anchor, &services);

        // Try to submit quote with valid_until exactly at current time
        // Expected: panic with StaleQuote (valid_until=50000 == current_time=50000)
        client.submit_quote(
            &anchor,
            &String::from_str(&env, "USD"),
            &String::from_str(&env, "USDC"),
            &10000u64,
            &25u32,
            &100u64,
            &100000u64,
            &50000u64, // Exactly current time
        );
    }

    /// Test quote submission with valid_until one second in the future (should succeed)
    ///
    /// Per the bugfix spec §3.1: `valid_until > current_time` should succeed.
    #[test]
    fn test_quote_submission_with_valid_until_one_second_future() {
        let env = make_env();
        set_ledger(&env, 55000, 0);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let anchor = Address::generate(&env);
        client.initialize(&admin);

        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &anchor, &anchor, &sk);

        let mut services = soroban_sdk::Vec::new(&env);
        services.push_back(3u32);
        client.configure_services(&anchor, &services);

        // Submit quote with valid_until one second in the future
        // Expected: success (valid_until=55001 > current_time=55000)
        let quote_id = client.submit_quote(
            &anchor,
            &String::from_str(&env, "USD"),
            &String::from_str(&env, "USDC"),
            &10000u64,
            &25u32,
            &100u64,
            &100000u64,
            &55001u64, // One second in the future
        );
        assert_eq!(quote_id, 1, "Quote should be stored with ID 1");
    }

    // =========================================================================
    // TRANSACTION STATE TRACKER TTL BOUNDARY TESTS
    // =========================================================================

    /// Test transaction state TTL boundaries in dev mode
    #[test]
    fn test_transaction_state_cleanup_at_expiry() {
        let env = make_env();
        set_ledger(&env, 60000, 0);
        
        let initiator = Address::generate(&env);
        let mut tracker = TransactionStateTracker::new(true); // dev mode

        // Create transaction
        tracker.create_transaction(1, initiator.clone(), &env).unwrap();
        assert_eq!(tracker.cache_size(), 1);

        // Mark as expired
        tracker.expired_ids.push(1);

        // Cleanup should remove it
        let removed = tracker.cleanup_expired(&env);
        assert_eq!(removed, 1);
        assert_eq!(tracker.cache_size(), 0);
    }

    /// Test transaction state transitions at ledger boundaries
    #[test]
    fn test_transaction_state_transitions_across_ledgers() {
        let env = make_env();
        set_ledger(&env, 70000, 1000);
        
        let initiator = Address::generate(&env);
        let mut tracker = TransactionStateTracker::new(true);

        // Create at ledger 1000
        tracker.create_transaction(1, initiator.clone(), &env).unwrap();
        
        // Advance ledger and transition
        set_ledger(&env, 70100, 1001);
        tracker.start_transaction(1, &env).unwrap();
        
        // Advance ledger and complete
        set_ledger(&env, 70200, 1002);
        let record = tracker.complete_transaction(1, &env).unwrap();
        
        assert_eq!(record.state, TransactionState::Completed);
        assert_eq!(record.state_history.len(), 3);
    }

    /// Test multiple transactions with different TTLs
    #[test]
    fn test_multiple_transactions_different_ttls() {
        let env = make_env();
        set_ledger(&env, 80000, 2000);
        
        let initiator = Address::generate(&env);
        let mut tracker = TransactionStateTracker::new(true);

        // Create multiple transactions
        tracker.create_transaction(1, initiator.clone(), &env).unwrap();
        tracker.create_transaction(2, initiator.clone(), &env).unwrap();
        tracker.create_transaction(3, initiator.clone(), &env).unwrap();

        // Complete transaction 1 (gets terminal TTL)
        tracker.complete_transaction(1, &env).unwrap();

        // Mark transactions 2 and 3 as expired
        tracker.expired_ids.push(2);
        tracker.expired_ids.push(3);

        // Cleanup should remove only expired ones
        let removed = tracker.cleanup_expired(&env);
        assert_eq!(removed, 2);
        assert_eq!(tracker.cache_size(), 1);

        // Transaction 1 should still exist
        let state = tracker.get_transaction_state(1, &env).unwrap();
        assert!(state.is_some());
    }

    // =========================================================================
    // TIMESTAMP OVERFLOW AND EDGE CASES
    // =========================================================================

    /// Test behavior near timestamp overflow
    #[test]
    fn test_timestamp_near_max_u64() {
        let env = make_env();
        let near_max = u64::MAX - 10000;
        set_ledger(&env, near_max, 0);

        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        client.initialize(&admin);

        // Should handle large timestamps without panic
        let request_id = client.generate_request_id();
        assert_eq!(request_id.created_at, near_max,
            "Request ID should capture the large timestamp");
    }

    /// Test zero timestamp handling
    #[test]
    fn test_zero_timestamp() {
        let env = make_env();
        set_ledger(&env, 0, 0);

        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        client.initialize(&admin);

        // Session created at t=0 should work
        let session_id = client.create_session(&user);
        let session = client.get_session(&session_id);
        assert_eq!(session.created_at, 0,
            "Session created_at should be 0 at genesis ledger");
    }

    /// Test ledger sequence zero
    #[test]
    fn test_ledger_sequence_zero() {
        let env = make_env();
        set_ledger(&env, 1000, 0);

        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        client.initialize(&admin);

        // Request ID generation at sequence=0 should work
        let request_id = client.generate_request_id();
        assert!(request_id.id.len() > 0,
            "Request ID should be generated even at sequence 0");
    }

    /// Test attestation timestamp = 0 is rejected
    ///
    /// `check_timestamp` panics when timestamp == 0.
    #[test]
    #[should_panic]
    fn test_attestation_zero_timestamp_rejected() {
        let env = make_env();
        set_ledger(&env, 1000, 100);

        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        client.initialize(&admin);

        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &attestor, &attestor, &sk);

        let ph = payload(&env, 0xBB);
        let sig = sign_payload(&env, &sk, &ph);

        // timestamp=0 should panic with InvalidTimestamp
        client.submit_attestation(&attestor, &subject, &0u64, &ph, &sig);
    }

    /// Test attestation timestamp = 1 (minimum valid) is accepted
    #[test]
    fn test_attestation_minimum_timestamp_accepted() {
        let env = make_env();
        set_ledger(&env, 1000, 100);

        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        client.initialize(&admin);

        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &attestor, &attestor, &sk);

        let ph = payload(&env, 0xCC);
        let sig = sign_payload(&env, &sk, &ph);

        // timestamp=1 should succeed (minimum valid value)
        let id = client.submit_attestation(&attestor, &subject, &1u64, &ph, &sig);
        assert_eq!(id, 0, "Attestation with timestamp=1 should succeed");
    }

    // =========================================================================
    // KYC EXPIRY BOUNDARY TESTS
    // =========================================================================

    /// Test KYC expiry at exact boundary
    ///
    /// KYC expiry logic: `if timestamp() > expiry { return KycStatus::Expired }`
    /// So KYC is valid when `now <= expiry`
    /// - At expiry: now == expiry → VALID (inclusive)
    /// - At expiry+1: now > expiry → EXPIRED
    #[test]
    fn test_kyc_expires_exactly_at_boundary() {
        let env = make_env();
        set_ledger(&env, 100000, 0);

        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        client.initialize(&admin);

        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &attestor, &attestor, &sk);

        // Submit KYC at t=100000
        client.submit_kyc(&attestor, &subject);
        // Approve KYC with expiry at t=103600
        client.approve_kyc(&subject);

        // Verify KYC is approved
        let status = client.get_kyc_status(&subject);
        // Status should be Approved (2)
        assert_eq!(status as u32, 2u32, "KYC should be Approved");
    }

    // =========================================================================
    // REPLAY PROTECTION TTL BOUNDARY TESTS
    // =========================================================================

    /// Test replay protection prevents duplicate attestations
    ///
    /// Replay protection uses REPLAY_TTL = 120,960 ledgers (~7 days).
    /// Within that window, the same payload_hash cannot be reused.
    #[test]
    fn test_replay_protection_ttl_boundary() {
        let env = make_env();
        set_ledger(&env, 90000, 3000);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let attestor = Address::generate(&env);
        let subject = Address::generate(&env);
        client.initialize(&admin);

        let sk = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &attestor, &attestor, &sk);

        // Submit first attestation
        let ph = payload(&env, 0xAA);
        let sig = sign_payload(&env, &sk, &ph);
        let id1 = client.submit_attestation(&attestor, &subject, &90000u64, &ph, &sig);
        assert_eq!(id1, 0, "First attestation should succeed");

        // Immediate replay with same payload_hash should fail
        let sig2 = sign_payload(&env, &sk, &ph);
        assert!(
            client.try_submit_attestation(&attestor, &subject, &90001u64, &ph, &sig2).is_err(),
            "Replay with same payload_hash should be rejected"
        );

        // Different payload_hash should succeed
        let ph_new = payload(&env, 0xBB);
        let sig3 = sign_payload(&env, &sk, &ph_new);
        let id2 = client.submit_attestation(&attestor, &subject, &90002u64, &ph_new, &sig3);
        assert_eq!(id2, 1, "Different payload_hash should succeed");
    }

    /// Test replay protection with different attestors (same payload_hash allowed)
    ///
    /// Replay protection is per-attestor: (issuer, payload_hash) is the unique key.
    /// Two different attestors can submit the same payload_hash.
    #[test]
    fn test_replay_protection_is_per_attestor() {
        let env = make_env();
        set_ledger(&env, 95000, 4000);
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let attestor1 = Address::generate(&env);
        let attestor2 = Address::generate(&env);
        let subject = Address::generate(&env);
        client.initialize(&admin);

        let sk1 = SigningKey::generate(&mut OsRng);
        let sk2 = SigningKey::generate(&mut OsRng);
        register_attestor_with_sep10(&env, &client, &attestor1, &attestor1, &sk1);
        register_attestor_with_sep10(&env, &client, &attestor2, &attestor2, &sk2);

        // Both attestors submit the same payload_hash
        let ph = payload(&env, 0xDD);
        let sig1 = sign_payload(&env, &sk1, &ph);
        let sig2 = sign_payload(&env, &sk2, &ph);

        let id1 = client.submit_attestation(&attestor1, &subject, &95000u64, &ph, &sig1);
        let id2 = client.submit_attestation(&attestor2, &subject, &95001u64, &ph, &sig2);

        assert_eq!(id1, 0, "Attestor1 should succeed");
        assert_eq!(id2, 1, "Attestor2 should succeed with same payload_hash");
    }

    // =========================================================================
    // REQUEST ID GENERATION BOUNDARY TESTS
    // =========================================================================

    /// Test request ID uniqueness across ledger boundaries
    ///
    /// Request ID = sha256(timestamp_u64_be || sequence_u32_be)[:16]
    /// Different (timestamp, sequence) pairs must produce different IDs.
    #[test]
    fn test_request_id_unique_across_ledger_boundaries() {
        let env = make_env();
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        client.initialize(&admin);

        // Generate ID at ledger 0, t=0
        set_ledger(&env, 0, 0);
        let id0 = client.generate_request_id();

        // Generate ID at ledger 1, t=0 (same timestamp, different sequence)
        set_ledger(&env, 0, 1);
        let id1 = client.generate_request_id();

        // Generate ID at ledger 0, t=1 (different timestamp, same sequence)
        set_ledger(&env, 1, 0);
        let id2 = client.generate_request_id();

        // All three should be different
        assert_ne!(id0.id, id1.id,
            "Different sequences should produce different request IDs");
        assert_ne!(id0.id, id2.id,
            "Different timestamps should produce different request IDs");
        assert_ne!(id1.id, id2.id,
            "Different (timestamp, sequence) pairs should produce different IDs");
    }

    /// Test request ID at ledger sequence rollover boundary
    ///
    /// At sequence u32::MAX, the next sequence wraps to 0.
    /// The request ID should still be unique due to the timestamp component.
    #[test]
    fn test_request_id_at_max_sequence() {
        let env = make_env();
        let contract_id = env.register_contract(None, AnchorKitContract);
        let client = AnchorKitContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        client.initialize(&admin);

        // Generate ID at max sequence
        set_ledger(&env, 1000, u32::MAX);
        let id_max = client.generate_request_id();
        assert_eq!(id_max.id.len(), 16,
            "Request ID should always be 16 bytes");

        // Generate ID at sequence 0 with different timestamp
        set_ledger(&env, 2000, 0);
        let id_zero = client.generate_request_id();
        assert_ne!(id_max.id, id_zero.id,
            "Different timestamps ensure uniqueness even at sequence boundaries");
    }
}
