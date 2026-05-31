//! Helper utilities for boundary condition testing
//!
//! This module provides reusable test utilities for setting up and verifying
//! boundary conditions across ledger sequences and timestamps.

#![cfg(test)]

use soroban_sdk::{
    testutils::{Ledger, LedgerInfo},
    Env,
};

/// Ledger configuration builder for boundary tests
pub struct BoundaryLedgerBuilder {
    timestamp: u64,
    sequence: u32,
}

impl BoundaryLedgerBuilder {
    pub fn new() -> Self {
        Self {
            timestamp: 0,
            sequence: 0,
        }
    }

    pub fn timestamp(mut self, timestamp: u64) -> Self {
        self.timestamp = timestamp;
        self
    }

    pub fn sequence(mut self, sequence: u32) -> Self {
        self.sequence = sequence;
        self
    }

    pub fn apply(&self, env: &Env) {
        env.ledger().set(LedgerInfo {
            timestamp: self.timestamp,
            protocol_version: 21,
            sequence_number: self.sequence,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });
    }
}

impl Default for BoundaryLedgerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Test scenario for boundary conditions
pub struct BoundaryScenario {
    pub name: &'static str,
    pub start_time: u64,
    pub start_sequence: u32,
    pub duration: u64,
    pub window_length: u32,
}

impl BoundaryScenario {
    /// Create a scenario for testing TTL expiry
    pub fn ttl_expiry(ttl_seconds: u64) -> Self {
        Self {
            name: "TTL Expiry",
            start_time: 10000,
            start_sequence: 1000,
            duration: ttl_seconds,
            window_length: 0,
        }
    }

    /// Create a scenario for testing rate limit windows
    pub fn rate_limit_window(window_length: u32) -> Self {
        Self {
            name: "Rate Limit Window",
            start_time: 20000,
            start_sequence: 2000,
            duration: 0,
            window_length,
        }
    }

    /// Get timestamp one before expiry
    pub fn time_before_expiry(&self) -> u64 {
        self.start_time + self.duration - 1
    }

    /// Get timestamp at exact expiry
    pub fn time_at_expiry(&self) -> u64 {
        self.start_time + self.duration
    }

    /// Get timestamp one after expiry
    pub fn time_after_expiry(&self) -> u64 {
        self.start_time + self.duration + 1
    }

    /// Get sequence one before window expiry
    pub fn sequence_before_expiry(&self) -> u32 {
        self.start_sequence + self.window_length - 1
    }

    /// Get sequence at exact window expiry
    pub fn sequence_at_expiry(&self) -> u32 {
        self.start_sequence + self.window_length
    }

    /// Get sequence one after window expiry
    pub fn sequence_after_expiry(&self) -> u32 {
        self.start_sequence + self.window_length + 1
    }
}

/// Boundary test assertion helpers
pub struct BoundaryAssertions;

impl BoundaryAssertions {
    /// Assert that a value is valid before the boundary
    pub fn assert_valid_before<T, E>(result: Result<T, E>, boundary_name: &str) {
        assert!(
            result.is_ok(),
            "Expected valid result before {} boundary, got error",
            boundary_name
        );
    }

    /// Assert that a value is valid at the boundary
    pub fn assert_valid_at<T, E>(result: Result<T, E>, boundary_name: &str) {
        assert!(
            result.is_ok(),
            "Expected valid result at {} boundary, got error",
            boundary_name
        );
    }

    /// Assert that a value is invalid after the boundary
    pub fn assert_invalid_after<T, E>(result: Result<T, E>, boundary_name: &str) {
        assert!(
            result.is_err(),
            "Expected error after {} boundary, got success",
            boundary_name
        );
    }

    /// Assert timestamp ordering
    pub fn assert_timestamp_order(t1: u64, t2: u64, t3: u64) {
        assert!(t1 < t2, "t1 should be before t2");
        assert!(t2 < t3, "t2 should be before t3");
        assert!(t1 < t3, "t1 should be before t3");
    }

    /// Assert sequence ordering
    pub fn assert_sequence_order(s1: u32, s2: u32, s3: u32) {
        assert!(s1 < s2, "s1 should be before s2");
        assert!(s2 < s3, "s2 should be before s3");
        assert!(s1 < s3, "s1 should be before s3");
    }
}

/// Edge case values for boundary testing
pub struct EdgeCaseValues;

impl EdgeCaseValues {
    pub const ZERO_TIMESTAMP: u64 = 0;
    pub const ZERO_SEQUENCE: u32 = 0;
    pub const MIN_TTL: u64 = 1;
    pub const MIN_WINDOW: u32 = 1;
    pub const MAX_TIMESTAMP: u64 = u64::MAX - 10000;
    pub const MAX_SEQUENCE: u32 = u32::MAX - 1000;
    
    /// Common TTL values in seconds
    pub const ONE_MINUTE: u64 = 60;
    pub const ONE_HOUR: u64 = 3600;
    pub const ONE_DAY: u64 = 86400;
    pub const ONE_WEEK: u64 = 604800;
    pub const ONE_MONTH: u64 = 2592000;
    
    /// Common window lengths in ledgers (assuming ~5s per ledger)
    pub const TEN_LEDGERS: u32 = 10;
    pub const ONE_HUNDRED_LEDGERS: u32 = 100;
    pub const ONE_THOUSAND_LEDGERS: u32 = 1000;
}

#[cfg(test)]
mod boundary_helper_tests {
    use super::*;

    #[test]
    fn test_boundary_scenario_ttl_expiry() {
        let scenario = BoundaryScenario::ttl_expiry(100);
        assert_eq!(scenario.time_before_expiry(), 10099);
        assert_eq!(scenario.time_at_expiry(), 10100);
        assert_eq!(scenario.time_after_expiry(), 10101);
    }

    #[test]
    fn test_boundary_scenario_rate_limit() {
        let scenario = BoundaryScenario::rate_limit_window(50);
        assert_eq!(scenario.sequence_before_expiry(), 2049);
        assert_eq!(scenario.sequence_at_expiry(), 2050);
        assert_eq!(scenario.sequence_after_expiry(), 2051);
    }

    #[test]
    fn test_ledger_builder() {
        let env = Env::default();
        BoundaryLedgerBuilder::new()
            .timestamp(12345)
            .sequence(678)
            .apply(&env);
        
        assert_eq!(env.ledger().timestamp(), 12345);
        assert_eq!(env.ledger().sequence(), 678);
    }

    #[test]
    fn test_timestamp_ordering() {
        BoundaryAssertions::assert_timestamp_order(100, 200, 300);
    }

    #[test]
    fn test_sequence_ordering() {
        BoundaryAssertions::assert_sequence_order(10, 20, 30);
    }

    #[test]
    #[should_panic(expected = "t1 should be before t2")]
    fn test_timestamp_ordering_fails() {
        BoundaryAssertions::assert_timestamp_order(200, 100, 300);
    }
}
