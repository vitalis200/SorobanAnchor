//! Feature gate isolation tests.
//!
//! Verifies that each feature flag (`std`, `wasm`, `mock-only`, `stress-tests`):
//!   - Enables exactly the code it claims to enable.
//!   - Does not expose symbols that belong to a different feature.
//!   - Produces correct output when its gated functionality is exercised.
//!
//! Run the full matrix with:
//!   cargo test --test feature_gate_tests --features std,mock-only
//!
//! Individual feature slices:
//!   cargo test --test feature_gate_tests                        # default (std)
//!   cargo test --test feature_gate_tests --features mock-only
//!   cargo test --test feature_gate_tests --features stress-tests

#[cfg(test)]
mod feature_gate_tests {

    // ── `std` feature ─────────────────────────────────────────────────────────

    /// The `std` feature enables the `config` module.
    /// When `std` is active, `load_runtime_config_file` must load real config files.
    #[cfg(feature = "std")]
    #[test]
    fn test_std_enables_config_file_loading() {
        use anchorkit::load_runtime_config_file;

        // Use an existing config fixture from the configs/ directory
        let result = load_runtime_config_file("configs/fiat-on-off-ramp.toml");
        assert!(
            result.is_ok(),
            "load_runtime_config_file must succeed for configs/fiat-on-off-ramp.toml: {:?}",
            result.err()
        );
        let cfg = result.unwrap();
        // Verify the contract section is present (all configs must have it)
        assert!(!cfg.contract.name.is_empty(), "contract.name must be non-empty");
        assert!(!cfg.contract.network.is_empty(), "contract.network must be non-empty");
    }

    /// When `std` is active, JSON config loading also works.
    #[cfg(feature = "std")]
    #[test]
    fn test_std_enables_json_config_loading() {
        use anchorkit::load_runtime_config_file;

        let result = load_runtime_config_file("configs/fiat-on-off-ramp.json");
        assert!(
            result.is_ok(),
            "load_runtime_config_file must succeed for configs/fiat-on-off-ramp.json: {:?}",
            result.err()
        );
        let cfg = result.unwrap();
        assert!(!cfg.contract.name.is_empty());
    }

    /// Core library symbols must always be available regardless of `std`.
    #[test]
    fn test_core_symbols_always_available() {
        // domain validation
        let res = anchorkit::validate_anchor_domain("https://anchor.example.com");
        assert!(res.is_ok(), "validate_anchor_domain must always be available");

        // error types
        let _err: anchorkit::AnchorKitError;

        // deterministic hash — verify it's in scope (function pointer cast, no call)
        let _fn = anchorkit::compute_payload_hash;
    }

    // ── `wasm` feature ────────────────────────────────────────────────────────

    /// When `wasm` is NOT active, the host-side SEP modules must be present.
    #[cfg(not(feature = "wasm"))]
    #[test]
    fn test_non_wasm_exposes_sep6() {
        use anchorkit::{RawDepositResponse, initiate_deposit};

        let raw = RawDepositResponse {
            transaction_id: "txn-wasm-check".into(),
            how: "Send to bank account".into(),
            extra_info: None,
            min_amount: Some(1),
            max_amount: Some(1000),
            fee_fixed: None,
            status: Some("pending_external".into()),
            clawback_enabled: None,
            stellar_memo: None,
            stellar_memo_type: None,
            asset_code: Some("USDC".into()),
        };
        let deposit = initiate_deposit(raw).expect("initiate_deposit must work in non-wasm builds");
        assert_eq!(deposit.transaction_id, "txn-wasm-check");
    }

    /// When `wasm` is NOT active, the SEP-24 interactive flow must be accessible.
    #[cfg(not(feature = "wasm"))]
    #[test]
    fn test_non_wasm_exposes_sep24() {
        use anchorkit::{RawInteractiveDepositResponse, initiate_interactive_deposit};

        let raw = RawInteractiveDepositResponse {
            url: "https://anchor.example.com/sep24/deposit".into(),
            id: "txn-sep24-check".into(),
        };
        let resp = initiate_interactive_deposit(raw)
            .expect("initiate_interactive_deposit must work in non-wasm builds");
        assert_eq!(resp.id, "txn-sep24-check");
    }

    /// When `wasm` is NOT active, SEP-38 quote parsing must be accessible.
    #[cfg(not(feature = "wasm"))]
    #[test]
    fn test_non_wasm_exposes_sep38() {
        use anchorkit::sep38::{RawFirmQuote, request_firm_quote};

        let raw = RawFirmQuote {
            id: "q-check".into(),
            expires_at: "9999999999".into(),
            price: "1.05".into(),
            sell_amount: "100".into(),
            buy_amount: "105".into(),
            sell_asset: "XLM".into(),   // plain uppercase code required by normalize_asset_code
            buy_asset: "USDC".into(),
        };
        let quote = request_firm_quote(raw, 1_000_000_000)
            .expect("request_firm_quote must work in non-wasm builds");
        assert_eq!(quote.id, "q-check");
    }

    /// When the `wasm` feature IS active, only on-chain-safe types must be exposed.
    /// This test is compile-only: if it compiles with `--features wasm`, the gate works.
    #[cfg(feature = "wasm")]
    #[test]
    fn test_wasm_build_has_core_types() {
        // These must compile in wasm builds
        let _: anchorkit::AnchorKitError;
        let _: anchorkit::ErrorCode;
        let _: anchorkit::TransactionState;
        let _: anchorkit::RateLimiter;
    }

    // ── `mock-only` feature ───────────────────────────────────────────────────

    /// The `mock-only` feature must expose all documented mock builders.
    #[cfg(feature = "mock-only")]
    #[test]
    fn test_mock_deposit_response_is_valid() {
        use anchorkit::mock::mock_deposit_response;
        use anchorkit::initiate_deposit;

        let raw = mock_deposit_response();
        let deposit = initiate_deposit(raw)
            .expect("mock_deposit_response must pass initiate_deposit validation");

        assert_eq!(deposit.transaction_id, anchorkit::mock::MOCK_TXN_ID);
        assert!(!deposit.how.is_empty(), "mock deposit must have instructions");
    }

    #[cfg(feature = "mock-only")]
    #[test]
    fn test_mock_withdrawal_response_is_valid() {
        use anchorkit::mock::mock_withdrawal_response;
        use anchorkit::initiate_withdrawal;

        let raw = mock_withdrawal_response();
        let withdrawal = initiate_withdrawal(raw)
            .expect("mock_withdrawal_response must pass initiate_withdrawal validation");

        assert!(!withdrawal.transaction_id.is_empty());
        assert!(!withdrawal.account_id.is_empty());
    }

    #[cfg(feature = "mock-only")]
    #[test]
    fn test_mock_interactive_deposit_response_is_valid() {
        use anchorkit::mock::mock_interactive_deposit_response;
        use anchorkit::initiate_interactive_deposit;

        let raw = mock_interactive_deposit_response();
        let resp = initiate_interactive_deposit(raw)
            .expect("mock_interactive_deposit_response must pass validation");

        assert_eq!(resp.id, anchorkit::mock::MOCK_TXN_ID_24);
        assert!(
            resp.url.starts_with("https://"),
            "mock URL must use HTTPS, got: {}",
            resp.url
        );
    }

    #[cfg(feature = "mock-only")]
    #[test]
    fn test_mock_interactive_withdrawal_response_is_valid() {
        use anchorkit::mock::mock_interactive_withdrawal_response;
        use anchorkit::sep24::initiate_interactive_withdrawal;

        let raw = mock_interactive_withdrawal_response();
        let resp = initiate_interactive_withdrawal(raw)
            .expect("mock_interactive_withdrawal_response must pass validation");

        assert!(resp.url.starts_with("https://"));
    }

    #[cfg(feature = "mock-only")]
    #[test]
    fn test_mock_sep24_transaction_responses_are_valid() {
        use anchorkit::mock::{mock_sep24_transaction_pending, mock_sep24_transaction_completed};
        use anchorkit::sep24::fetch_sep24_transaction_status;

        let pending = mock_sep24_transaction_pending();
        assert_eq!(pending.status, "pending_user_transfer_start");

        let completed = mock_sep24_transaction_completed();
        assert_eq!(completed.status, "completed");

        // Both must parse without error
        let p = fetch_sep24_transaction_status(pending)
            .expect("pending sep24 mock must parse");
        assert!(!p.id.is_empty());

        let c = fetch_sep24_transaction_status(completed)
            .expect("completed sep24 mock must parse");
        assert!(!c.id.is_empty());
    }

    #[cfg(feature = "mock-only")]
    #[test]
    fn test_mock_sep38_price_is_valid() {
        use anchorkit::mock::mock_price;
        use anchorkit::sep38::fetch_prices;

        let raw = mock_price();
        let price = fetch_prices(raw).expect("mock_price must pass fetch_prices validation");
        assert!(!price.buy_asset.is_empty());
        assert!(!price.sell_asset.is_empty());
    }

    #[cfg(feature = "mock-only")]
    #[test]
    fn test_mock_firm_quote_is_valid_and_not_expired() {
        use anchorkit::mock::{mock_firm_quote, MOCK_EXPIRES_AT};
        use anchorkit::sep38::{request_firm_quote, is_quote_expired};

        let raw = mock_firm_quote();
        // Use a timestamp well before expiry
        let now = MOCK_EXPIRES_AT - 3600;
        let quote = request_firm_quote(raw, now)
            .expect("mock_firm_quote must pass request_firm_quote validation");

        assert_eq!(quote.id, "mock-quote-001");
        assert!(!is_quote_expired(&quote, now), "mock quote must not be expired at t=now-1h");
    }

    #[cfg(feature = "mock-only")]
    #[test]
    fn test_mock_anchor_capabilities_fields() {
        use anchorkit::mock::{mock_anchor_capabilities, MOCK_ANCHOR_URL, MOCK_ASSET_CODE};

        let caps = mock_anchor_capabilities();
        assert_eq!(caps.anchor_url, MOCK_ANCHOR_URL);
        assert_eq!(caps.asset_code, MOCK_ASSET_CODE);
        assert!(caps.supports_sep6);
        assert!(caps.supports_sep24);
        assert!(caps.supports_sep38);
    }

    #[cfg(feature = "mock-only")]
    #[test]
    fn test_mock_transaction_status_variants() {
        use anchorkit::mock::{mock_transaction_response_pending, mock_transaction_response_completed};
        use anchorkit::fetch_transaction_status;

        let pending_raw = mock_transaction_response_pending();
        assert_eq!(pending_raw.status, "pending_external");

        let completed_raw = mock_transaction_response_completed();
        assert_eq!(completed_raw.status, "completed");

        // Both must parse without error
        let p = fetch_transaction_status(pending_raw)
            .expect("pending mock must parse");
        assert_eq!(p.status.as_str(), "pending_external");

        let c = fetch_transaction_status(completed_raw)
            .expect("completed mock must parse");
        assert_eq!(c.status.as_str(), "completed");
    }

    // ── `stress-tests` feature ────────────────────────────────────────────────

    /// The `stress-tests` feature gates the load simulation test file.
    /// When active, the `stress_tests` compile flag must be set.
    #[cfg(feature = "stress-tests")]
    #[test]
    fn test_stress_tests_feature_is_active() {
        // Compile-time proof: this block only exists when stress-tests is enabled.
        // The load_simulation_tests.rs file's #![cfg(feature = "stress-tests")]
        // ensures those tests only run with this feature.
        assert!(
            cfg!(feature = "stress-tests"),
            "stress-tests feature should be active when this test runs"
        );
    }

    /// When `stress-tests` is NOT active, this confirms the guard works at compile time.
    #[cfg(not(feature = "stress-tests"))]
    #[test]
    fn test_stress_tests_feature_not_active_by_default() {
        assert!(
            !cfg!(feature = "stress-tests"),
            "stress-tests must be off by default to keep normal CI fast"
        );
    }

    // ── Feature isolation: mutually exclusive gates ───────────────────────────

    /// Confirms that `wasm` and host-only modules are mutually exclusive at the
    /// type level: SEP-6 symbols must not be reachable via the top-level re-export
    /// when `wasm` is enabled. This is enforced at compile time by the
    /// `#[cfg(not(feature = "wasm"))]` gates in lib.rs.
    ///
    /// We test the positive case (non-wasm) above; the negative case (wasm) is
    /// verified by the feature gate compile matrix in scripts/test-feature-gates.sh.
    #[cfg(not(feature = "wasm"))]
    #[test]
    fn test_sep6_reachable_in_non_wasm_build() {
        // If this compiles, the `#[cfg(not(feature = "wasm"))]` gate is working.
        let _ = core::mem::size_of::<anchorkit::RawDepositResponse>();
    }

    // ── Combined feature matrix ───────────────────────────────────────────────

    /// std + mock-only: both feature sets must coexist without conflict.
    #[cfg(all(feature = "std", feature = "mock-only"))]
    #[test]
    fn test_std_and_mock_only_coexist() {
        use anchorkit::load_runtime_config_file;
        use anchorkit::mock::mock_deposit_response;
        use anchorkit::initiate_deposit;

        // std path: load a real config file
        let cfg = load_runtime_config_file("configs/fiat-on-off-ramp.toml")
            .expect("std config load must succeed");
        assert!(!cfg.contract.name.is_empty());

        // mock-only path: parse without network
        let deposit = initiate_deposit(mock_deposit_response())
            .expect("mock deposit must be valid");
        assert!(!deposit.transaction_id.is_empty());
    }
}
