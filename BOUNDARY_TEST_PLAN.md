# Ledger Boundary Condition Test Plan

## Executive Summary

This document outlines the comprehensive test plan for verifying boundary conditions in ledger sequence and timestamp-based features to prevent off-by-one errors in production.

## Objectives

1. **Identify** all features that depend on ledger sequences and timestamps
2. **Implement** boundary condition tests for each feature
3. **Verify** correct behavior at, before, and after boundaries
4. **Document** expected behavior and edge cases
5. **Ensure** no off-by-one failures in production

## Scope

### In Scope
- Rate limiting window transitions
- Metadata cache TTL expiration
- Session TTL expiration
- Quote validity periods
- Transaction state TTL management
- Replay protection expiration
- Request ID generation
- Tracing span timestamps

### Out of Scope
- Network-level ledger consensus
- Stellar Core ledger management
- Off-chain timestamp synchronization

## Test Strategy

### 1. Boundary Identification

For each time-based feature, identify three critical points:
- **Before Boundary** (boundary - 1): Should be VALID
- **At Boundary** (boundary): Should be VALID (inclusive) or INVALID (exclusive) based on specification
- **After Boundary** (boundary + 1): Should be INVALID

### 2. Test Categories

#### A. Unit Tests
- Test individual boundary conditions in isolation
- Use mocked ledger state
- Fast execution (<1ms per test)

#### B. Integration Tests
- Test boundary conditions across multiple components
- Use real contract deployment
- Moderate execution time (<100ms per test)

#### C. Property-Based Tests
- Generate random boundary values
- Verify invariants hold for all inputs
- Comprehensive coverage

#### D. Stress Tests
- Test behavior under high load at boundaries
- Verify no race conditions
- Performance validation

### 3. Test Implementation

#### Test File Structure
```
tests/
├── ledger_boundary_tests.rs          # Main boundary tests
├── boundary_test_helpers.rs          # Reusable test utilities
├── rate_limiter_boundary_tests.rs    # Rate limiter specific
├── cache_boundary_tests.rs           # Cache TTL specific
├── session_boundary_tests.rs         # Session TTL specific
└── quote_boundary_tests.rs           # Quote validity specific
```

## Detailed Test Cases

### 1. Rate Limiter Window Boundaries

#### Test Case 1.1: Window Expires at Exact Boundary
**Objective:** Verify rate limit window expires at exactly `window_length` ledgers

**Setup:**
- Configure rate limiter: `max_submissions=2`, `window_length=100`
- Start at ledger 1000
- Submit 2 attestations (reach limit)

**Test Steps:**
1. Advance to ledger 1099 (before boundary)
   - **Expected:** Submission fails (still rate limited)
2. Advance to ledger 1100 (at boundary)
   - **Expected:** Submission succeeds (window reset)
3. Advance to ledger 1101 (after boundary)
   - **Expected:** Submission succeeds (window reset)

**Acceptance Criteria:**
- ✅ Window does NOT reset at ledger 1099
- ✅ Window DOES reset at ledger 1100
- ✅ No off-by-one error

#### Test Case 1.2: Minimum Window Length
**Objective:** Verify correct behavior with `window_length=1`

**Setup:**
- Configure rate limiter: `max_submissions=1`, `window_length=1`
- Start at ledger 5000

**Test Steps:**
1. Submit attestation at ledger 5000
   - **Expected:** Success
2. Submit attestation at ledger 5000
   - **Expected:** Fail (rate limited)
3. Advance to ledger 5001
   - **Expected:** Success (window reset)

**Acceptance Criteria:**
- ✅ Window resets after exactly 1 ledger
- ✅ No off-by-one error with minimum window

#### Test Case 1.3: Ledger Sequence Overflow
**Objective:** Verify overflow protection near `u32::MAX`

**Setup:**
- Start at ledger `u32::MAX - 50`
- Configure rate limiter: `max_submissions=1`, `window_length=100`

**Test Steps:**
1. Submit attestation at ledger `u32::MAX - 50`
   - **Expected:** Success
2. Advance to ledger `u32::MAX`
   - **Expected:** No panic, graceful handling

**Acceptance Criteria:**
- ✅ No panic or overflow
- ✅ Saturating arithmetic prevents wraparound

### 2. Metadata Cache TTL Boundaries

#### Test Case 2.1: Cache Expires at Exact TTL
**Objective:** Verify cache expires at `cached_at + ttl_seconds`

**Setup:**
- Cache metadata at t=1000 with TTL=100

**Test Steps:**
1. Read at t=1099 (before expiry)
   - **Expected:** Success (cache valid)
2. Read at t=1100 (at expiry)
   - **Expected:** Success (inclusive boundary)
3. Read at t=1101 (after expiry)
   - **Expected:** Fail (cache expired)

**Acceptance Criteria:**
- ✅ Cache valid at t=1100 (inclusive)
- ✅ Cache expired at t=1101
- ✅ No off-by-one error

#### Test Case 2.2: Zero TTL
**Objective:** Verify immediate expiry with TTL=0

**Setup:**
- Cache metadata at t=2000 with TTL=0

**Test Steps:**
1. Read at t=2000 (same time)
   - **Expected:** Success (just cached)
2. Read at t=2001 (one second later)
   - **Expected:** Fail (expired)

**Acceptance Criteria:**
- ✅ TTL=0 causes immediate expiry
- ✅ No undefined behavior

#### Test Case 2.3: Stale-While-Revalidate Boundaries
**Objective:** Verify SWR three-state transition

**Setup:**
- Cache at t=5000 with primary_ttl=100, stale_ttl=50

**Test Steps:**
1. Read at t=5099 (before primary expiry)
   - **Expected:** FRESH (needs_refresh=false)
2. Read at t=5100 (at primary expiry)
   - **Expected:** FRESH (needs_refresh=false)
3. Read at t=5101 (in stale window)
   - **Expected:** STALE (needs_refresh=true, data available)
4. Read at t=5149 (before total expiry)
   - **Expected:** STALE (needs_refresh=true, data available)
5. Read at t=5150 (at total expiry)
   - **Expected:** STALE (needs_refresh=true, data available)
6. Read at t=5151 (after total expiry)
   - **Expected:** EXPIRED (error)

**Acceptance Criteria:**
- ✅ Three distinct states: FRESH → STALE → EXPIRED
- ✅ Boundaries are inclusive where specified
- ✅ No off-by-one errors in state transitions

### 3. Session TTL Boundaries

#### Test Case 3.1: Session Expires at Exact TTL
**Objective:** Verify session expires at `created_at + session_ttl_seconds`

**Setup:**
- Create session at t=10000 with default TTL=3600

**Test Steps:**
1. Use session at t=13599 (before expiry)
   - **Expected:** Success
2. Use session at t=13600 (at expiry)
   - **Expected:** Success (inclusive)
3. Use session at t=13601 (after expiry)
   - **Expected:** Fail (expired)

**Acceptance Criteria:**
- ✅ Session valid at t=13600
- ✅ Session expired at t=13601
- ✅ No off-by-one error

#### Test Case 3.2: Custom Session TTL
**Objective:** Verify custom TTL boundaries

**Setup:**
- Create session at t=20000 with custom TTL=100

**Test Steps:**
1. Verify session.session_ttl_seconds = 100
2. Use session at t=20100 (at expiry)
   - **Expected:** Success
3. Use session at t=20101 (after expiry)
   - **Expected:** Fail

**Acceptance Criteria:**
- ✅ Custom TTL respected
- ✅ Boundary behavior consistent

### 4. Quote Validity Boundaries

#### Test Case 4.1: Quote Expires at valid_until
**Objective:** Verify quote validity at `valid_until` timestamp

**Setup:**
- Submit quote at t=30000 with valid_until=30500

**Test Steps:**
1. Route at t=30499 (before expiry)
   - **Expected:** Quote included
2. Route at t=30500 (at expiry)
   - **Expected:** Quote included (inclusive)
3. Route at t=30501 (after expiry)
   - **Expected:** Quote filtered out

**Acceptance Criteria:**
- ✅ Quote valid at t=30500
- ✅ Quote expired at t=30501
- ✅ Routing filters expired quotes

#### Test Case 4.2: Reject Past valid_until
**Objective:** Prevent submission of already-expired quotes

**Setup:**
- Current time t=40000

**Test Steps:**
1. Submit quote with valid_until=39999
   - **Expected:** Panic (StaleQuote)
2. Submit quote with valid_until=40000
   - **Expected:** Panic (StaleQuote)
3. Submit quote with valid_until=40001
   - **Expected:** Success

**Acceptance Criteria:**
- ✅ Quotes with valid_until <= current_time rejected
- ✅ Only future quotes accepted

### 5. Transaction State TTL Boundaries

#### Test Case 5.1: Cleanup at Expiry
**Objective:** Verify expired transactions are cleaned up

**Setup:**
- Create transactions with various TTLs
- Mark some as expired

**Test Steps:**
1. Call cleanup_expired()
2. Verify only expired transactions removed
3. Verify non-expired transactions remain

**Acceptance Criteria:**
- ✅ Expired transactions removed
- ✅ Active transactions preserved
- ✅ Terminal state transactions have shorter TTL

### 6. Edge Cases

#### Test Case 6.1: Zero Values
**Objective:** Verify handling of zero timestamps and sequences

**Test Steps:**
1. Create session at t=0
2. Generate request ID at sequence=0
3. Verify no panics or undefined behavior

**Acceptance Criteria:**
- ✅ Zero values handled gracefully
- ✅ No division by zero
- ✅ No underflow

#### Test Case 6.2: Maximum Values
**Objective:** Verify handling near maximum values

**Test Steps:**
1. Set timestamp to u64::MAX - 10000
2. Set sequence to u32::MAX - 1000
3. Perform operations
4. Verify no overflow

**Acceptance Criteria:**
- ✅ No overflow panics
- ✅ Saturating arithmetic used
- ✅ Graceful degradation

## Test Execution

### Prerequisites
```bash
# Install Rust toolchain
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install Soroban CLI
cargo install --locked soroban-cli

# Verify installation
cargo --version
soroban --version
```

### Running Tests

#### All Boundary Tests
```bash
cd SorobanAnchor
cargo test --test ledger_boundary_tests --verbose
```

#### Specific Test Category
```bash
# Rate limiter tests
cargo test --test ledger_boundary_tests rate_limit

# Cache tests
cargo test --test ledger_boundary_tests cache

# Session tests
cargo test --test ledger_boundary_tests session

# Quote tests
cargo test --test ledger_boundary_tests quote
```

#### With Coverage
```bash
cargo install cargo-tarpaulin
cargo tarpaulin --test ledger_boundary_tests --out Html
```

### Continuous Integration

Add to `.github/workflows/test.yml`:
```yaml
name: Boundary Tests

on: [push, pull_request]

jobs:
  boundary-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: actions-rs/toolchain@v1
        with:
          toolchain: stable
      - name: Run boundary tests
        run: cargo test --test ledger_boundary_tests --verbose
      - name: Check for off-by-one errors
        run: |
          cargo test --test ledger_boundary_tests 2>&1 | \
          grep -i "off-by-one" && exit 1 || exit 0
```

## Success Metrics

### Quantitative Metrics
- ✅ 100% of identified boundary conditions have tests
- ✅ All tests pass consistently
- ✅ Code coverage >95% for boundary logic
- ✅ Zero off-by-one errors in production

### Qualitative Metrics
- ✅ Clear documentation of expected behavior
- ✅ Easy to add new boundary tests
- ✅ Fast test execution (<5 seconds total)
- ✅ Maintainable test code

## Risk Assessment

### High Risk Areas
1. **Rate Limiter Window Transitions**
   - Impact: Could allow spam or block legitimate users
   - Mitigation: Comprehensive boundary tests + property-based testing

2. **Session Expiry**
   - Impact: Security vulnerability if sessions don't expire
   - Mitigation: Multiple test cases + manual verification

3. **Quote Validity**
   - Impact: Financial loss if expired quotes accepted
   - Mitigation: Strict validation + boundary tests

### Medium Risk Areas
1. **Cache TTL**
   - Impact: Stale data served to users
   - Mitigation: SWR pattern + boundary tests

2. **Transaction State TTL**
   - Impact: Storage bloat or premature deletion
   - Mitigation: Configurable TTLs + cleanup tests

### Low Risk Areas
1. **Request ID Generation**
   - Impact: Duplicate IDs (unlikely)
   - Mitigation: Timestamp + sequence combination

## Maintenance Plan

### Regular Activities
- **Weekly:** Review test results in CI
- **Monthly:** Update test cases for new features
- **Quarterly:** Audit boundary logic for changes
- **Annually:** Full security review of time-based features

### When to Update Tests
- Adding new time-based features
- Modifying TTL values
- Changing boundary logic
- After any off-by-one bug report

## Appendix

### A. Boundary Condition Checklist

For each new time-based feature, verify:
- [ ] Boundary identified (exact expiry point)
- [ ] Test at boundary - 1 (before)
- [ ] Test at boundary (exact)
- [ ] Test at boundary + 1 (after)
- [ ] Test with minimum values
- [ ] Test with maximum values
- [ ] Test with zero values
- [ ] Documentation updated
- [ ] CI integration added

### B. Common Pitfalls

1. **Using `<` instead of `<=`**
   - Symptom: Expires one unit too early
   - Fix: Use inclusive comparison at boundary

2. **Forgetting saturating arithmetic**
   - Symptom: Overflow panic
   - Fix: Use `.saturating_add()`, `.saturating_sub()`

3. **Mixing ledger and timestamp units**
   - Symptom: Incorrect expiry calculations
   - Fix: Clear variable naming, unit tests

4. **Not testing edge cases**
   - Symptom: Production failures with unusual values
   - Fix: Comprehensive edge case coverage

### C. References

- [Soroban Documentation](https://soroban.stellar.org/docs)
- [Stellar Protocol](https://github.com/stellar/stellar-protocol)
- [Off-by-One Errors](https://en.wikipedia.org/wiki/Off-by-one_error)
- [Property-Based Testing](https://hypothesis.works/articles/what-is-property-based-testing/)

### D. Glossary

- **Boundary**: The exact point where a condition changes from valid to invalid
- **TTL**: Time To Live - duration before expiration
- **Ledger Sequence**: Monotonically increasing counter for Stellar ledgers
- **Timestamp**: Unix timestamp in seconds
- **Window**: Duration measured in ledgers for rate limiting
- **SWR**: Stale-While-Revalidate - caching strategy with grace period
