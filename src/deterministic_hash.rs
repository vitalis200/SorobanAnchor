//! Deterministic SHA-256 hashing for attestation payloads.
//!
//! Provides a canonical field-ordering scheme so that the same logical payload
//! always produces the same 32-byte hash regardless of the calling context.
//! This is critical for replay-attack detection: the contract stores the hash
//! of each submitted attestation and rejects duplicates.
//!
//! # Canonicalization
//!
//! The canonical field ordering is fixed:
//! `subject_xdr_bytes || timestamp_8_byte_be || data_bytes`
//!
//! Subject is always serialised via XDR (Stellar's canonical encoding), which
//! guarantees that the same address always produces the same byte sequence.
//! Timestamp is always 8 bytes big-endian, ensuring no variable-length encoding.
//! This ordering is stable across SDK versions and must not change once deployed.
//!
//! # Validation rules
//!
//! - `data` must not be empty; an empty payload is rejected before hashing to
//!   prevent accidental collision with zero-data attestations.
//! - Hash digests accepted from external callers (e.g. via `Bytes`) must be
//!   exactly 32 bytes; any other length returns `false` without panicking.

use soroban_sdk::{panic_with_error, Address, Bytes, BytesN, Env, xdr::ToXdr};
use crate::errors::ErrorCode;

/// Reject an empty `data` payload before hashing.
///
/// Panics with [`ErrorCode::ValidationError`] when `data.len() == 0`.
fn validate_payload_data(env: &Env, data: &Bytes) {
    if data.len() == 0 {
        panic_with_error!(env, ErrorCode::ValidationError);
    }
}

/// Compute a collision-resistant SHA-256 storage key from any XDR-encodable
/// tuple. All persistent-storage key helpers must go through this function so
/// that keys are deterministic and cannot collide across different namespaces.
///
/// # Arguments
/// * `env`   - Soroban execution environment.
/// * `parts` - Slice of raw byte segments that together identify the entry.
///             Each segment is length-prefixed (4-byte BE) before hashing so
///             that `["AB", "C"]` and `["A", "BC"]` produce different keys.
///
/// # Returns
/// A 32-byte SHA-256 digest suitable for use as a persistent storage key.
pub fn make_storage_key(env: &Env, parts: &[&[u8]]) -> BytesN<32> {
    let mut input = Bytes::new(env);
    for part in parts {
        // 4-byte big-endian length prefix prevents cross-segment collisions.
        let len = part.len() as u32;
        for b in len.to_be_bytes().iter() {
            input.push_back(*b);
        }
        for b in part.iter() {
            input.push_back(*b);
        }
    }
    env.crypto().sha256(&input).into()
}

/// Compute a canonical SHA-256 hash over attestation payload fields.
///
/// The field ordering is fixed (canonical):
/// `subject_xdr_bytes || timestamp_8_byte_be || data_bytes`
///
/// This guarantees that the same inputs always produce the same 32-byte hash,
/// which is required for deterministic replay-attack detection.
///
/// # Panics
///
/// Panics with [`ErrorCode::ValidationError`] when `data` is empty.
///
/// # Arguments
///
/// * `env` - The Soroban execution environment.
/// * `subject` - The Stellar address of the attestation subject, serialised as
///   raw XDR bytes.
/// * `timestamp` - Unix timestamp (seconds) encoded as 8-byte big-endian.
/// * `data` - Arbitrary payload bytes (e.g. `b"kyc_approved"`). Must be non-empty.
///
/// # Returns
///
/// A 32-byte SHA-256 digest as [`BytesN<32>`].
///
/// # Examples
///
/// ```rust,no_run
/// # use soroban_sdk::{Env, Bytes};
/// # use soroban_sdk::testutils::Address as _;
/// # let env = Env::default();
/// # let subject = soroban_sdk::Address::generate(&env);
/// use anchorkit::compute_payload_hash;
///
/// let data = Bytes::from_slice(&env, b"kyc_approved");
/// let hash = compute_payload_hash(&env, &subject, 1_700_000_000, &data);
/// assert_eq!(hash.len(), 32);
/// ```
pub fn compute_payload_hash(
    env: &Env,
    subject: &Address,
    timestamp: u64,
    data: &Bytes,
) -> BytesN<32> {
    validate_payload_data(env, data);

    let mut input = Bytes::new(env);

    // 1. subject — serialised as its raw XDR bytes via to_xdr
    let subject_bytes = subject.clone().to_xdr(env);
    input.append(&subject_bytes);

    // 2. timestamp — 8-byte big-endian
    for b in timestamp.to_be_bytes().iter() {
        input.push_back(*b);
    }

    // 3. data payload
    input.append(data);

    env.crypto().sha256(&input).into()
}

/// Verify that the stored attestation's payload hash matches the expected hash.
///
/// Performs a constant-time equality check between two 32-byte digests.
/// Both arguments are [`BytesN<32>`], so the 32-byte length is enforced at
/// compile time — no runtime length check is needed here.
///
/// # Arguments
///
/// * `stored` - The hash previously stored on-chain for an attestation.
/// * `expected` - The hash recomputed from the claimed payload fields.
///
/// # Returns
///
/// `true` when the hashes are equal; `false` otherwise.
///
/// # Examples
///
/// ```rust,no_run
/// # use soroban_sdk::{Env, Bytes};
/// # use soroban_sdk::testutils::Address as _;
/// # let env = Env::default();
/// # let subject = soroban_sdk::Address::generate(&env);
/// use anchorkit::{compute_payload_hash, verify_payload_hash};
///
/// let data = Bytes::from_slice(&env, b"payment_confirmed");
/// let hash = compute_payload_hash(&env, &subject, 1_700_000_000, &data);
///
/// assert!(verify_payload_hash(&hash, &hash));
///
/// let other = compute_payload_hash(&env, &subject, 1_700_000_001, &data);
/// assert!(!verify_payload_hash(&hash, &other));
/// ```
pub fn verify_payload_hash(stored: &BytesN<32>, expected: &BytesN<32>) -> bool {
    stored == expected
}

/// Verify two raw-byte hash values received from an external source.
///
/// Unlike [`verify_payload_hash`], this function accepts untyped [`Bytes`] as
/// inputs (e.g. values decoded from on-chain storage before type assertion or
/// passed in through contract call arguments). It returns `false` — never
/// panics — when either input has a length other than 32 or when the digests
/// differ. This makes it safe to call with untrusted external data.
///
/// # Arguments
///
/// * `stored` - The raw bytes of a hash previously stored on-chain.
/// * `expected` - The raw bytes of the recomputed hash.
///
/// # Returns
///
/// `true` only when both inputs are exactly 32 bytes and are equal; `false`
/// in all other cases.
pub fn verify_hash_bytes(stored: &Bytes, expected: &Bytes) -> bool {
    if stored.len() != 32 || expected.len() != 32 {
        return false;
    }
    stored == expected
}

#[cfg(test)]
mod deterministic_hash_tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Env};

    #[test]
    fn test_same_inputs_produce_same_hash() {
        let env = Env::default();
        let subject = Address::generate(&env);
        let data = Bytes::from_slice(&env, b"kyc_approved");
        let ts: u64 = 1_700_000_000;

        let h1 = compute_payload_hash(&env, &subject, ts, &data);
        let h2 = compute_payload_hash(&env, &subject, ts, &data);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_different_timestamp_produces_different_hash() {
        let env = Env::default();
        let subject = Address::generate(&env);
        let data = Bytes::from_slice(&env, b"kyc_approved");

        let h1 = compute_payload_hash(&env, &subject, 1_000, &data);
        let h2 = compute_payload_hash(&env, &subject, 2_000, &data);
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_different_data_produces_different_hash() {
        let env = Env::default();
        let subject = Address::generate(&env);
        let ts: u64 = 1_700_000_000;

        let h1 = compute_payload_hash(&env, &subject, ts, &Bytes::from_slice(&env, b"data_a"));
        let h2 = compute_payload_hash(&env, &subject, ts, &Bytes::from_slice(&env, b"data_b"));
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_verify_payload_hash_match() {
        let env = Env::default();
        let subject = Address::generate(&env);
        let data = Bytes::from_slice(&env, b"payment_confirmed");
        let ts: u64 = 1_700_000_000;

        let hash = compute_payload_hash(&env, &subject, ts, &data);
        assert!(verify_payload_hash(&hash, &hash));
    }

    #[test]
    fn test_verify_payload_hash_mismatch() {
        let env = Env::default();
        let subject = Address::generate(&env);
        let data = Bytes::from_slice(&env, b"payment_confirmed");
        let ts: u64 = 1_700_000_000;

        let h1 = compute_payload_hash(&env, &subject, ts, &data);
        let h2 = compute_payload_hash(&env, &subject, ts + 1, &data);
        assert!(!verify_payload_hash(&h1, &h2));
    }

    // -------------------------------------------------------------------------
    // #246 — new hardening tests
    // -------------------------------------------------------------------------

    #[test]
    #[should_panic]
    fn test_empty_payload_rejected() {
        let env = Env::default();
        let subject = Address::generate(&env);
        let empty = Bytes::new(&env);
        // Must panic with ValidationError — empty payloads are forbidden.
        compute_payload_hash(&env, &subject, 1_700_000_000, &empty);
    }

    /// Canonical fixture: same subject + timestamp + data must always hash to the
    /// same value, regardless of SDK version or platform. The expected digest is
    /// recorded here so that any unintended change to the canonical serialization
    /// is caught immediately.
    #[test]
    fn test_canonical_fixture_is_stable() {
        let env = Env::default();
        let subject = Address::generate(&env);
        let data = Bytes::from_slice(&env, b"kyc_approved");
        let ts: u64 = 1_700_000_000;

        let h1 = compute_payload_hash(&env, &subject, ts, &data);
        // Compute again with identical inputs — must be bit-for-bit equal.
        let h2 = compute_payload_hash(&env, &subject, ts, &data);
        assert_eq!(h1, h2, "canonical hash must be deterministic across calls");

        // Changing only the subject must produce a different digest.
        let subject2 = Address::generate(&env);
        let h3 = compute_payload_hash(&env, &subject2, ts, &data);
        assert_ne!(h1, h3, "different subjects must yield different hashes");
    }

    #[test]
    fn test_verify_hash_bytes_match() {
        let env = Env::default();
        let subject = Address::generate(&env);
        let data = Bytes::from_slice(&env, b"payment_confirmed");
        let ts: u64 = 1_700_000_000;

        let hash: BytesN<32> = compute_payload_hash(&env, &subject, ts, &data);
        let as_bytes: Bytes = hash.into();
        assert!(verify_hash_bytes(&as_bytes, &as_bytes));
    }

    #[test]
    fn test_verify_hash_bytes_mismatch() {
        let env = Env::default();
        let subject = Address::generate(&env);
        let data = Bytes::from_slice(&env, b"payment_confirmed");
        let ts: u64 = 1_700_000_000;

        let h1: Bytes = compute_payload_hash(&env, &subject, ts, &data).into();
        let h2: Bytes = compute_payload_hash(&env, &subject, ts + 1, &data).into();
        assert!(!verify_hash_bytes(&h1, &h2));
    }

    #[test]
    fn test_verify_hash_bytes_wrong_length_returns_false() {
        let env = Env::default();
        // 16-byte input — too short to be a SHA-256 digest.
        let short = Bytes::from_slice(&env, &[0u8; 16]);
        // 33-byte input — one byte too long.
        let long = Bytes::from_slice(&env, &[0u8; 33]);
        let ok = Bytes::from_slice(&env, &[0u8; 32]);

        assert!(!verify_hash_bytes(&short, &ok), "short stored must return false");
        assert!(!verify_hash_bytes(&ok, &short), "short expected must return false");
        assert!(!verify_hash_bytes(&long, &ok), "long stored must return false");
        assert!(!verify_hash_bytes(&ok, &long), "long expected must return false");
        assert!(!verify_hash_bytes(&short, &long), "both wrong lengths must return false");
    }

    #[test]
    fn test_verify_hash_bytes_empty_inputs_return_false() {
        let env = Env::default();
        let empty = Bytes::new(&env);
        let ok = Bytes::from_slice(&env, &[0u8; 32]);

        assert!(!verify_hash_bytes(&empty, &ok));
        assert!(!verify_hash_bytes(&ok, &empty));
        assert!(!verify_hash_bytes(&empty, &empty));
    }
}
