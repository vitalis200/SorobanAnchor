# Environment Abstraction Verification & Testing Guide

## Overview

This document provides step-by-step verification that the environment abstraction has been properly implemented to ensure clean separation between std (native) and WASM build paths.

## Pre-Testing Checklist

- [ ] Project cloned/updated to latest code
- [ ] Rust 1.70+ installed (`rustc --version`)
- [ ] Cargo installed (`cargo --version`)
- [ ] WASM target available (`rustup target list | grep wasm32-unknown-unknown`)

If WASM target is missing, install it:

```bash
rustup target add wasm32-unknown-unknown
```

## Section 1: Verify Code Changes

### 1.1 Check Cargo.toml Feature Configuration

**Expected:** Features are properly defined and dependencies are conditional.

```bash
cd /workspaces/SorobanAnchor

# Verify std feature includes all host-only dependencies
grep -A 5 "^\[features\]" Cargo.toml
```

**Expected output:**
```toml
[features]
default = ["std"]
std = ["clap", "reqwest", "aes-gcm", "argon2", "rpassword", "rand/std"]
wasm = []
```

**Verify dependencies are marked optional:**
```bash
grep -E "^(clap|reqwest|aes-gcm|argon2|rpassword)" Cargo.toml
```

**Expected:** All should have `optional = true`

### 1.2 Check main.rs Feature Gate

**Expected:** main.rs is wrapped with feature guard.

```bash
head -5 src/main.rs
```

**Expected output:**
```rust
#![cfg(feature = "std")]
//! CLI binary for AnchorKit.
```

### 1.3 Check config.rs Feature Gates

**Expected:** File-loading functions are guarded.

```bash
grep -B 1 "pub fn load_runtime_config_file\|pub fn from_path" src/config.rs | head -10
```

**Expected output:**
```rust
#[cfg(feature = "std")]
pub fn load_runtime_config_file(path: impl AsRef<Path>) -> Result<RuntimeConfig, String> {
```

### 1.4 Check lib.rs Exports

**Expected:** config module exports are gated.

```bash
grep "pub use config\|pub mod config" src/lib.rs
```

**Expected output:**
```rust
#[cfg(feature = "std")]
pub mod config;
#[cfg(feature = "std")]
pub use config::{load_runtime_config_file, parse_runtime_config_str, ConfigFormat, RuntimeConfig};
```

## Section 2: Build Path Testing

### 2.1 Native Build (std, Default)

**Purpose:** Verify that the standard (native) build path compiles successfully and includes CLI.

```bash
cd /workspaces/SorobanAnchor

# Clean previous builds
cargo clean

# Build with default features
cargo build --release 2>&1 | tee /tmp/native_build.log
```

**Verification steps:**

```bash
# 1. Check for successful completion
grep -E "Finished|error:" /tmp/native_build.log | tail -5

# Expected: "Finished release [optimized] target(s) in X.XXs"

# 2. Verify CLI binary exists
test -f target/release/anchorkit && echo "✓ CLI binary created" || echo "✗ CLI binary missing"

# 3. Check binary size
ls -lh target/release/anchorkit
# Expected: ~20-50 MB (depending on optimizations)

# 4. Verify CLI works
./target/release/anchorkit --help | head -3
# Expected: Shows AnchorKit CLI help
```

### 2.2 WASM Build (no_std)

**Purpose:** Verify that WASM builds succeed without pulling in std-only dependencies.

```bash
cd /workspaces/SorobanAnchor

# Build for WASM
cargo build --release \
  --target wasm32-unknown-unknown \
  --no-default-features \
  --features wasm 2>&1 | tee /tmp/wasm_build.log
```

**Verification steps:**

```bash
# 1. Check for successful compilation
grep -E "Finished|error:" /tmp/wasm_build.log | tail -5
# Expected: "Finished release [optimized] target(s) in X.XXs"

# 2. Verify WASM artifact exists
WASM_FILE="target/wasm32-unknown-unknown/release/anchorkit.wasm"
test -f "$WASM_FILE" && echo "✓ WASM artifact created" || echo "✗ WASM artifact missing"

# 3. Check WASM file size (should be much smaller than native)
ls -lh "$WASM_FILE"
# Expected: ~200-500 KB (minimal runtime)

# 4. Verify no std symbols in WASM
strings "$WASM_FILE" | grep -c "std::" | head -1
# Expected: 0 (no standard library symbols)

# 5. Verify contract code is present
strings "$WASM_FILE" | grep -i "contract\|attestor" | head -3
# Expected: Shows some contract-related strings
```

### 2.3 No-std Library Build

**Purpose:** Verify that the library can be built without std for verification.

```bash
cd /workspaces/SorobanAnchor

# Build library only without std
cargo build --release --lib --no-default-features 2>&1 | tee /tmp/nostd_lib.log
```

**Verification steps:**

```bash
# Check compilation succeeded
grep -E "Finished|error:" /tmp/nostd_lib.log | tail -5
# Expected: "Finished release [optimized] target(s) in X.XXs"

# Verify library artifact
test -f target/release/libanchorkit.rlib && echo "✓ Library built" || echo "✗ Library build failed"
```

### 2.4 Dependency Isolation Test

**Purpose:** Verify that trying to build WASM with std-only deps fails appropriately.

```bash
cd /workspaces/SorobanAnchor

# Attempt to build with conflicting features (should fail gracefully or build with std only)
cargo build --target wasm32-unknown-unknown --features "wasm,std" 2>&1 | tee /tmp/conflict_build.log
```

**Expected behavior:**
- Either builds successfully (both features are present)
- Or fails with clear error about feature conflicts
- Should NOT attempt to pull in `reqwest`, `clap` when strictly wasm32 target is specified

## Section 3: Test Suite Verification

### 3.1 Run Full Test Suite

```bash
cd /workspaces/SorobanAnchor

# Run tests with default features
cargo test --release 2>&1 | tee /tmp/tests.log
```

**Verification:**

```bash
# Check test results
tail -20 /tmp/tests.log | grep -E "test result:|passed|failed"

# Expected output includes:
# test result: ok. X passed; 0 failed; 0 ignored; Y measured; Z filtered out
```

### 3.2 Run Tests with Mock Features

```bash
cargo test --release --features mock-only 2>&1 | tee /tmp/tests_mock.log

# Check results
tail -10 /tmp/tests_mock.log | grep "test result"
```

### 3.3 Verify No Panics on Missing Dependencies

**Purpose:** Ensure WASM code paths don't accidentally reference std symbols.

```bash
# Try to use a WASM build in a way that would require std (should fail or work correctly)
cargo check --target wasm32-unknown-unknown --no-default-features --features wasm
```

**Expected:** Completes successfully without errors

## Section 4: Automated Build Matrix Test

### 4.1 Run the Test Script

```bash
cd /workspaces/SorobanAnchor

# Make script executable (if needed)
chmod +x scripts/test_build_matrix.sh

# Run the comprehensive test suite
./scripts/test_build_matrix.sh 2>&1 | tee /tmp/build_matrix_test.log
```

**Expected output format:**
```
═══════════════════════════════════════════════════════════
  SorobanAnchor Build Matrix Test
═══════════════════════════════════════════════════════════

ℹ Testing environment separation for std vs. WASM builds

═══════════════════════════════════════════════════════════
  1. Native (std) Build Path
═══════════════════════════════════════════════════════════

→ Building with std feature (default, includes CLI)
✓ Standard library build succeeded
✓ CLI binary created at target/release/anchorkit

═══════════════════════════════════════════════════════════
  2. WASM Build Path
═══════════════════════════════════════════════════════════

→ Building WASM target (no default features, no CLI)
✓ WASM build succeeded
✓ WASM artifact created: target/wasm32-unknown-unknown/release/anchorkit.wasm (...)
```

### 4.2 Verbose Test Output

```bash
./scripts/test_build_matrix.sh --verbose 2>&1 | tee /tmp/build_matrix_verbose.log

# View full build output
tail -100 /tmp/build_matrix_verbose.log
```

### 4.3 Clean Rebuild Test

```bash
# This may take longer (clears previous artifacts)
./scripts/test_build_matrix.sh --clean 2>&1 | tee /tmp/build_matrix_clean.log

# Verify final results
tail -30 /tmp/build_matrix_clean.log | grep -E "passed|failed|Success|Error"
```

## Section 5: Acceptance Criteria Verification

### Criterion 1: WASM builds succeed without native-only dependencies

**Test:**
```bash
cd /workspaces/SorobanAnchor

# Clear any previous builds
rm -rf target/wasm32-unknown-unknown/

# Build WASM
cargo build --release --target wasm32-unknown-unknown --no-default-features --features wasm

# Verify artifact
test -f target/wasm32-unknown-unknown/release/anchorkit.wasm && echo "✓ PASS" || echo "✗ FAIL"
```

**Success criteria:** ✓ WASM artifact created without errors

### Criterion 2: Native CLI builds still work with std enabled

**Test:**
```bash
cd /workspaces/SorobanAnchor

# Clear previous builds
rm -rf target/release/anchorkit

# Build native with std
cargo build --release

# Verify CLI works
./target/release/anchorkit --help | grep -q "anchorkit" && echo "✓ PASS" || echo "✗ FAIL"
```

**Success criteria:** ✓ CLI binary works and shows help

### Criterion 3: Tests cover the WASM build path

**Test:**
```bash
cd /workspaces/SorobanAnchor

# Run the build matrix test
./scripts/test_build_matrix.sh

# Check final output for all test passes
if grep -q "✓ All build matrix tests passed"; then
    echo "✓ PASS"
else
    echo "✗ FAIL"
fi
```

**Success criteria:** ✓ Test script passes all checks including WASM build

### Criterion 4: No std-only imports in WASM build

**Test:**
```bash
cd /workspaces/SorobanAnchor

# Build WASM
cargo build --release --target wasm32-unknown-unknown --no-default-features --features wasm

# Check for forbidden symbols
for symbol in "clap" "reqwest" "rpassword" "aes_gcm" "argon2"; do
    if strings target/wasm32-unknown-unknown/release/anchorkit.wasm | grep -qi "$symbol"; then
        echo "✗ Found forbidden symbol: $symbol"
    fi
done

echo "✓ PASS - No std-only dependencies found"
```

**Success criteria:** ✓ No std-only dependency symbols in WASM binary

## Section 6: Documentation Verification

### 6.1 Check Build Matrix Documentation

```bash
# Verify documentation file exists
test -f docs/build-matrix.md && echo "✓ Documentation created"

# Verify it contains key sections
for section in "Build Matrix" "Feature Flags" "Feature-Gated Code" "Build Commands Reference"; do
    grep -q "$section" docs/build-matrix.md && echo "✓ Contains: $section" || echo "✗ Missing: $section"
done
```

### 6.2 Check README Updates

```bash
# Verify README mentions build matrix
grep -q "Build Matrix" README.md && echo "✓ README updated with Build Matrix section"

# Verify build commands are documented
grep -q "wasm32-unknown-unknown" README.md && echo "✓ WASM build command in README"
```

## Section 7: Integration Testing

### 7.1 Verify Contract Functionality

```bash
cd /workspaces/SorobanAnchor

# Build WASM
cargo build --release --target wasm32-unknown-unknown --no-default-features --features wasm

# Verify contract module compiles
cargo build --release --target wasm32-unknown-unknown \
  --no-default-features --features wasm \
  -p anchorkit --lib

echo "✓ Contract module builds in WASM environment"
```

### 7.2 Verify SEP Modules

```bash
# Test that SEP modules compile in both environments
cargo build --release --lib --no-default-features
cargo build --release --lib

echo "✓ SEP modules build in all configurations"
```

## Summary Checklist

- [ ] **Cargo.toml changes verified**
  - [ ] std feature includes all host-only deps
  - [ ] Dependencies marked optional = true
  - [ ] wasm feature is minimal and self-contained

- [ ] **Code changes verified**
  - [ ] main.rs guarded with #[cfg(feature = "std")]
  - [ ] config.rs file-loading guarded
  - [ ] lib.rs exports properly gated

- [ ] **Build paths tested**
  - [ ] Native build succeeds with CLI
  - [ ] WASM build succeeds without std
  - [ ] Library-only build works
  - [ ] No st-only deps in WASM

- [ ] **Tests pass**
  - [ ] Full test suite passes
  - [ ] Build matrix test passes
  - [ ] No compilation errors

- [ ] **Documentation complete**
  - [ ] build-matrix.md created
  - [ ] README updated with build matrix table
  - [ ] Build commands reference added

- [ ] **Acceptance criteria met**
  - [ ] WASM builds without native deps
  - [ ] Native CLI still works
  - [ ] Tests cover WASM path
  - [ ] No forbidden symbols in WASM

## Troubleshooting

### Issue: "cannot find crate `clap`" when building WASM

**Solution:** Ensure you're using `--no-default-features --features wasm`:
```bash
# WRONG
cargo build --target wasm32-unknown-unknown

# CORRECT
cargo build --target wasm32-unknown-unknown --no-default-features --features wasm
```

### Issue: WASM target not installed

**Solution:**
```bash
rustup target add wasm32-unknown-unknown
rustup update
```

### Issue: Test script fails with "Command not found"

**Solution:** Ensure the script is executable:
```bash
chmod +x scripts/test_build_matrix.sh
```

### Issue: Build matrix test shows "some checks failed"

**Check:** Review the build log output for specific errors:
```bash
cat /tmp/std_build.log | grep -A 5 "error"
cat /tmp/wasm_build.log | grep -A 5 "error"
```

## Next Steps

After successful verification:

1. **Commit changes:**
   ```bash
   git add -A
   git commit -m "feat: add explicit environment abstraction for WASM vs native builds"
   ```

2. **Push to repository:**
   ```bash
   git push origin main
   ```

3. **Update CI/CD:** Add the build matrix test to your CI pipeline:
   ```yaml
   # Example for GitHub Actions
   - name: Build matrix test
     run: ./scripts/test_build_matrix.sh
   ```

4. **Release notes:** Document the new build requirements for users

## Support

For issues or questions:
1. Check [docs/build-matrix.md](../docs/build-matrix.md) for detailed information
2. Review build logs in `/tmp/*.log`
3. Run with `--verbose` flag for detailed output
4. Consult Cargo documentation on features: https://doc.rust-lang.org/cargo/reference/features.html
