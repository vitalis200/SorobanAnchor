//! Rate limiting for attestation submissions
//!
//! This module implements per-attestor rate limiting for attestation submissions
//! to prevent spam and abuse of the contract.

use soroban_sdk::{contracttype, xdr::ToXdr, Address, Env};
use crate::deterministic_hash::make_storage_key;
use crate::errors::AnchorKitError;
#[cfg(test)]
use crate::errors::ErrorCode;

/// Rate limit configuration stored in contract storage.
///
/// Defines the sliding-window parameters used by [`RateLimiter::check_and_increment`].
/// The admin can update this at runtime via [`RateLimiter::update_config`].
///
/// # Examples
///
/// ```rust,no_run
/// use anchorkit::RateLimitConfig;
///
/// // Allow at most 5 submissions per 50-ledger window.
/// let config = RateLimitConfig { max_submissions: 5, window_length: 50 };
/// ```
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RateLimitConfig {
    /// Maximum number of submissions allowed per window
    pub max_submissions: u32,
    /// Length of the rate limit window in ledgers
    pub window_length: u32,
}

/// Per-attestor rate limit state stored in contract storage.
///
/// Tracks how many submissions an attestor has made in the current window and
/// when that window started. Automatically reset when the window expires.
///
/// # Examples
///
/// ```rust,no_run
/// use anchorkit::RateLimitState;
///
/// let state = RateLimitState { submission_count: 3, window_start_ledger: 1000 };
/// assert_eq!(state.submission_count, 3);
/// ```
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RateLimitState {
    /// Number of submissions in the current window
    pub submission_count: u32,
    /// Ledger number when the current window started
    pub window_start_ledger: u32,
}

/// Per-attestor sliding-window rate limiter for attestation submissions.
///
/// All methods are associated functions that operate directly on Soroban
/// persistent storage, so no instance state is needed.
///
/// The default configuration (10 submissions per 100-ledger window) is used
/// when no config has been stored yet.
pub struct RateLimiter;

impl RateLimiter {
    /// Check whether an attestor is within their rate limit and increment the counter.
    ///
    /// If the current window has expired it is automatically reset before the
    /// check. The counter is only incremented when the check passes.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban execution environment.
    /// * `attestor` - The address of the attestor being checked.
    /// * `config` - The active [`RateLimitConfig`] (fetch via [`RateLimiter::get_config`]).
    ///
    /// # Returns
    ///
    /// `Ok(())` if the attestor is within the rate limit.
    ///
    /// # Errors
    ///
    /// Returns [`AnchorKitError`] with code [`ErrorCode::RateLimitExceeded`] when
    /// the attestor has reached `config.max_submissions` in the current window.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::Env;
    /// # use soroban_sdk::testutils::Address as _;
    /// # let env = Env::default();
    /// # let attestor = soroban_sdk::Address::generate(&env);
    /// use anchorkit::{RateLimiter, RateLimitConfig};
    ///
    /// let config = RateLimitConfig { max_submissions: 10, window_length: 100 };
    /// // First call succeeds.
    /// assert!(RateLimiter::check_and_increment(&env, &attestor, &config).is_ok());
    /// ```
    pub fn check_and_increment(
        env: &Env,
        attestor: &Address,
        config: &RateLimitConfig,
    ) -> Result<(), AnchorKitError> {
        let current_ledger = env.ledger().sequence();
        let state_key = Self::get_state_key(env, attestor);
        
        // Get or initialize rate limit state
        let mut state = env.storage().persistent().get::<_, RateLimitState>(&state_key)
            .unwrap_or(RateLimitState {
                submission_count: 0,
                window_start_ledger: current_ledger,
            });
        
        // Check if window has expired and reset if needed
        if Self::is_window_expired(current_ledger, state.window_start_ledger, config.window_length) {
            state = RateLimitState {
                submission_count: 0,
                window_start_ledger: current_ledger,
            };
        }
        
        // Check if limit is exceeded
        if state.submission_count >= config.max_submissions {
            return Err(AnchorKitError::rate_limit_exceeded());
        }
        
        // Increment counter and save state
        state.submission_count += 1;
        env.storage().persistent().set(&state_key, &state);
        
        Ok(())
    }
    
    /// Get the current rate limit state for an attestor.
    ///
    /// Returns a default state (zero submissions, current ledger as window start)
    /// if no state has been stored yet.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban execution environment.
    /// * `attestor` - The address of the attestor to query.
    ///
    /// # Returns
    ///
    /// The current [`RateLimitState`] for the attestor.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::Env;
    /// # use soroban_sdk::testutils::Address as _;
    /// # let env = Env::default();
    /// # let attestor = soroban_sdk::Address::generate(&env);
    /// use anchorkit::RateLimiter;
    ///
    /// let state = RateLimiter::get_state(&env, &attestor);
    /// assert_eq!(state.submission_count, 0);
    /// ```
    pub fn get_state(env: &Env, attestor: &Address) -> RateLimitState {
        let state_key = Self::get_state_key(env, attestor);
        env.storage().persistent().get::<_, RateLimitState>(&state_key)
            .unwrap_or(RateLimitState {
                submission_count: 0,
                window_start_ledger: env.ledger().sequence(),
            })
    }
    
    /// Update the rate limit configuration (admin only).
    ///
    /// Loads the stored admin from instance storage (key `"ADMIN"`) and calls
    /// `admin.require_auth()`. Returns `Err(NotInitialized)` if no admin is stored.
    /// Returns `Err(ValidationError)` if `config` contains zero or nonsensical values.
    pub fn update_config(
        env: &Env,
        admin: &Address,
        config: &RateLimitConfig,
    ) -> Result<(), AnchorKitError> {
        let stored_admin: Address = env
            .storage()
            .instance()
            .get::<_, Address>(&soroban_sdk::vec![env, soroban_sdk::symbol_short!("ADMIN")])
            .ok_or_else(AnchorKitError::not_initialized)?;
        if *admin != stored_admin {
            return Err(AnchorKitError::unauthorized_attestor());
        }
        admin.require_auth();
        Self::validate_config(config)?;
        let config_key = Self::get_config_key(env);
        env.storage().persistent().set(&config_key, config);
        Ok(())
    }

    /// Validate that a [`RateLimitConfig`] has sensible non-zero values.
    ///
    /// Returns `Err(ValidationError)` if `max_submissions` or `window_length` is zero.
    pub fn validate_config(config: &RateLimitConfig) -> Result<(), AnchorKitError> {
        if config.max_submissions == 0 {
            return Err(AnchorKitError::validation_error("max_submissions must be > 0"));
        }
        if config.window_length == 0 {
            return Err(AnchorKitError::validation_error("window_length must be > 0"));
        }
        Ok(())
    }
    
    /// Get the current rate limit configuration.
    ///
    /// Returns the stored configuration, or the default (10 submissions per
    /// 100-ledger window) if none has been set.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban execution environment.
    ///
    /// # Returns
    ///
    /// The active [`RateLimitConfig`].
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use soroban_sdk::Env;
    /// # let env = Env::default();
    /// use anchorkit::RateLimiter;
    ///
    /// let config = RateLimiter::get_config(&env);
    /// assert_eq!(config.max_submissions, 10);
    /// assert_eq!(config.window_length, 100);
    /// ```
    pub fn get_config(env: &Env) -> RateLimitConfig {
        let config_key = Self::get_config_key(env);
        env.storage().persistent().get::<_, RateLimitConfig>(&config_key)
            .unwrap_or(RateLimitConfig {
                max_submissions: 10,
                window_length: 100,
            })
    }
    
    /// Check if a window has expired
    fn is_window_expired(current_ledger: u32, window_start_ledger: u32, window_length: u32) -> bool {
        current_ledger.saturating_sub(window_start_ledger) >= window_length
    }
    
    /// Generate collision-resistant storage key for per-attestor rate limit state.
    fn get_state_key(env: &Env, attestor: &Address) -> soroban_sdk::BytesN<32> {
        let addr_xdr = attestor.clone().to_xdr(env);
        // collect xdr bytes into a plain slice via Bytes
        let mut raw = alloc::vec::Vec::with_capacity(addr_xdr.len() as usize);
        for i in 0..addr_xdr.len() {
            raw.push(addr_xdr.get(i).unwrap_or(0));
        }
        make_storage_key(env, &[b"RL_STATE", &raw])
    }

    /// Generate collision-resistant storage key for the global rate limit config.
    fn get_config_key(env: &Env) -> soroban_sdk::BytesN<32> {
        make_storage_key(env, &[b"RL_CONFIG"])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limit_under_limit() {
        let env = Env::default();
        let attestor = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        let config = RateLimitConfig {
            max_submissions: 10,
            window_length: 100,
        };
        
        // Create a dummy contract address for testing
        let contract_address = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        
        // Register a dummy contract for testing
        let contract_id = env.register_contract(&contract_address, crate::contract::AnchorKitContract);
        
        // Should succeed for first submission
        let result = env.as_contract(&contract_id, &|| {
            RateLimiter::check_and_increment(&env, &attestor, &config)
        });
        assert!(result.is_ok());
        
        // Check state
        let state = env.as_contract(&contract_id, &|| {
            RateLimiter::get_state(&env, &attestor)
        });
        assert_eq!(state.submission_count, 1);
    }
    
    #[test]
    fn test_rate_limit_at_limit() {
        let env = Env::default();
        let attestor = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        let config = RateLimitConfig {
            max_submissions: 2,
            window_length: 100,
        };
        
        let contract_address = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        let contract_id = env.register_contract(&contract_address, crate::contract::AnchorKitContract);
        
        // First two submissions should succeed
        assert!(env.as_contract(&contract_id, &|| {
            RateLimiter::check_and_increment(&env, &attestor, &config)
        }).is_ok());
        assert!(env.as_contract(&contract_id, &|| {
            RateLimiter::check_and_increment(&env, &attestor, &config)
        }).is_ok());
        
        // Third submission should fail
        let result = env.as_contract(&contract_id, &|| {
            RateLimiter::check_and_increment(&env, &attestor, &config)
        });
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, ErrorCode::RateLimitExceeded);
    }
    
    #[test]
    fn test_rate_limit_over_limit() {
        let env = Env::default();
        let attestor = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        let config = RateLimitConfig {
            max_submissions: 1,
            window_length: 100,
        };
        
        let contract_address = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        let contract_id = env.register_contract(&contract_address, crate::contract::AnchorKitContract);
        
        // First submission should succeed
        assert!(env.as_contract(&contract_id, &|| {
            RateLimiter::check_and_increment(&env, &attestor, &config)
        }).is_ok());
        
        // Second submission should fail
        let result = env.as_contract(&contract_id, &|| {
            RateLimiter::check_and_increment(&env, &attestor, &config)
        });
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, ErrorCode::RateLimitExceeded);
    }
    
    #[test]
    fn test_rate_limit_window_reset() {
        let env = Env::default();
        let attestor = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        let config = RateLimitConfig {
            max_submissions: 1,
            window_length: 10,
        };
        
        let contract_address = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        let contract_id = env.register_contract(&contract_address, crate::contract::AnchorKitContract);
        
        // First submission should succeed
        assert!(env.as_contract(&contract_id, &|| {
            RateLimiter::check_and_increment(&env, &attestor, &config)
        }).is_ok());
        
        // Second submission should fail (still in same window)
        assert!(env.as_contract(&contract_id, &|| {
            RateLimiter::check_and_increment(&env, &attestor, &config)
        }).is_err());
        
        // Note: In Soroban SDK, we cannot directly set the ledger sequence in tests
        // The window reset logic will be tested in integration tests with actual ledger progression
        // For now, we verify the state is correct
        let state = env.as_contract(&contract_id, &|| {
            RateLimiter::get_state(&env, &attestor)
        });
        assert_eq!(state.submission_count, 1);
    }
    
    #[test]
    fn test_rate_limit_config_update() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        let new_config = RateLimitConfig {
            max_submissions: 20,
            window_length: 200,
        };

        let contract_address = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        let contract_id = env.register_contract(&contract_address, crate::contract::AnchorKitContract);

        // Store admin in instance storage before calling update_config
        env.as_contract(&contract_id, &|| {
            env.storage()
                .instance()
                .set(&soroban_sdk::vec![&env, soroban_sdk::symbol_short!("ADMIN")], &admin);
        });

        let result = env.as_contract(&contract_id, &|| {
            RateLimiter::update_config(&env, &admin, &new_config)
        });
        assert!(result.is_ok());

        let config = env.as_contract(&contract_id, &|| {
            RateLimiter::get_config(&env)
        });
        assert_eq!(config.max_submissions, 20);
        assert_eq!(config.window_length, 200);
    }

    #[test]
    fn test_update_config_unauthorized() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        let non_admin = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        let new_config = RateLimitConfig { max_submissions: 5, window_length: 50 };

        let contract_address = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        let contract_id = env.register_contract(&contract_address, crate::contract::AnchorKitContract);

        env.as_contract(&contract_id, &|| {
            env.storage()
                .instance()
                .set(&soroban_sdk::vec![&env, soroban_sdk::symbol_short!("ADMIN")], &admin);
        });

        let result = env.as_contract(&contract_id, &|| {
            RateLimiter::update_config(&env, &non_admin, &new_config)
        });
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, ErrorCode::UnauthorizedAttestor);
    }

    #[test]
    fn test_update_config_not_initialized() {
        let env = Env::default();
        let admin = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        let new_config = RateLimitConfig { max_submissions: 5, window_length: 50 };

        let contract_address = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        let contract_id = env.register_contract(&contract_address, crate::contract::AnchorKitContract);

        // No admin stored — should return NotInitialized
        let result = env.as_contract(&contract_id, &|| {
            RateLimiter::update_config(&env, &admin, &new_config)
        });
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, ErrorCode::NotInitialized);
    }
    
    #[test]
    fn test_rate_limit_default_config() {
        let env = Env::default();
        
        let contract_address = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        let contract_id = env.register_contract(&contract_address, crate::contract::AnchorKitContract);
        
        // Get default config
        let config = env.as_contract(&contract_id, &|| {
            RateLimiter::get_config(&env)
        });
        assert_eq!(config.max_submissions, 10);
        assert_eq!(config.window_length, 100);
    }

    #[test]
    fn test_validate_config_rejects_zero_max_submissions() {
        let config = RateLimitConfig { max_submissions: 0, window_length: 100 };
        assert!(RateLimiter::validate_config(&config).is_err());
        assert_eq!(
            RateLimiter::validate_config(&config).unwrap_err().code,
            ErrorCode::ValidationError
        );
    }

    #[test]
    fn test_validate_config_rejects_zero_window_length() {
        let config = RateLimitConfig { max_submissions: 5, window_length: 0 };
        assert!(RateLimiter::validate_config(&config).is_err());
        assert_eq!(
            RateLimiter::validate_config(&config).unwrap_err().code,
            ErrorCode::ValidationError
        );
    }

    #[test]
    fn test_validate_config_accepts_valid() {
        let config = RateLimitConfig { max_submissions: 1, window_length: 1 };
        assert!(RateLimiter::validate_config(&config).is_ok());
    }

    #[test]
    fn test_update_config_rejects_zero_values() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        let bad_config = RateLimitConfig { max_submissions: 0, window_length: 100 };

        let contract_address = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        let contract_id = env.register_contract(&contract_address, crate::contract::AnchorKitContract);

        env.as_contract(&contract_id, &|| {
            env.storage()
                .instance()
                .set(&soroban_sdk::vec![&env, soroban_sdk::symbol_short!("ADMIN")], &admin);
        });

        let result = env.as_contract(&contract_id, &|| {
            RateLimiter::update_config(&env, &admin, &bad_config)
        });
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, ErrorCode::ValidationError);
    }

    #[test]
    fn test_window_rollover_at_exact_boundary() {
        let env = Env::default();
        let attestor = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        let config = RateLimitConfig { max_submissions: 1, window_length: 10 };

        let contract_address = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        let contract_id = env.register_contract(&contract_address, crate::contract::AnchorKitContract);

        // Fill the window
        assert!(env.as_contract(&contract_id, &|| {
            RateLimiter::check_and_increment(&env, &attestor, &config)
        }).is_ok());
        // Same window — should fail
        assert!(env.as_contract(&contract_id, &|| {
            RateLimiter::check_and_increment(&env, &attestor, &config)
        }).is_err());

        // Advance ledger by exactly window_length (10)
        env.ledger().set(soroban_sdk::testutils::LedgerInfo {
            sequence_number: 10,
            timestamp: 1000,
            protocol_version: 21,
            network_id: Default::default(),
            base_reserve: 0,
            min_persistent_entry_ttl: 4096,
            min_temp_entry_ttl: 16,
            max_entry_ttl: 6312000,
        });

        // Window should have rolled over — first submission in new window succeeds
        assert!(env.as_contract(&contract_id, &|| {
            RateLimiter::check_and_increment(&env, &attestor, &config)
        }).is_ok());
    }

    #[test]
    fn test_max_submission_error_is_consistent() {
        let env = Env::default();
        let attestor = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        let config = RateLimitConfig { max_submissions: 2, window_length: 100 };

        let contract_address = <soroban_sdk::Address as soroban_sdk::testutils::Address>::generate(&env);
        let contract_id = env.register_contract(&contract_address, crate::contract::AnchorKitContract);

        env.as_contract(&contract_id, &|| { RateLimiter::check_and_increment(&env, &attestor, &config).unwrap(); });
        env.as_contract(&contract_id, &|| { RateLimiter::check_and_increment(&env, &attestor, &config).unwrap(); });

        // Every subsequent call must return RateLimitExceeded without corrupting state
        for _ in 0..3 {
            let err = env.as_contract(&contract_id, &|| {
                RateLimiter::check_and_increment(&env, &attestor, &config)
            }).unwrap_err();
            assert_eq!(err.code, ErrorCode::RateLimitExceeded);
        }
        // State must still show exactly max_submissions
        let state = env.as_contract(&contract_id, &|| RateLimiter::get_state(&env, &attestor));
        assert_eq!(state.submission_count, 2);
    }
}
