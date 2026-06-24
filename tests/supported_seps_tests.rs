//! Tests for issue #353: contract-level supported SEP list and feature flags.

use anchorkit::contract::{
    AnchorKitContract, SepFeatureFlags, SEP_10, SEP_24, SEP_38, SEP_6,
};
use soroban_sdk::Env;

#[test]
fn supported_seps_returns_expected_list() {
    let env = Env::default();
    let seps = AnchorKitContract::supported_seps(env);
    assert_eq!(seps.len(), 4);
    assert!(seps.contains(&SEP_6));
    assert!(seps.contains(&SEP_10));
    assert!(seps.contains(&SEP_24));
    assert!(seps.contains(&SEP_38));
}

#[test]
fn supported_seps_constants_have_correct_values() {
    assert_eq!(SEP_6, 6);
    assert_eq!(SEP_10, 10);
    assert_eq!(SEP_24, 24);
    assert_eq!(SEP_38, 38);
}

#[test]
fn supported_sep_feature_flags_all_enabled() {
    let env = Env::default();
    let flags: SepFeatureFlags = AnchorKitContract::supported_sep_feature_flags(env);
    assert!(flags.sep6);
    assert!(flags.sep10);
    assert!(flags.sep24);
    assert!(flags.sep38);
}
