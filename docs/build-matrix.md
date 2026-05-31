# Environment Abstraction: Std vs. WASM Build Paths

## Overview

SorobanAnchor targets two distinct environments with different runtime capabilities:

1. **Native (std)**: Runs on standard Rust with full OS support (networking, filesystem, threading, etc.)
2. **WASM (no_std)**: Runs on Soroban (Stellar's smart contract environment) with minimal runtime

This document describes the build matrix, feature flags, and how environment-specific code is organized.

## Build Matrix

### ✓ Supported Configurations

| Build Target | Features | Purpose | CLI | Artifacts |
|---|---|---|---|---|
| `x86_64-unknown-linux-gnu` (default) | `std` (default) | Native development & deployment | ✓ Yes | `target/release/anchorkit` (binary) + library |
| `x86_64-unknown-linux-gnu` | `std,mock-only` | Native testing with mock helpers | ✓ Yes | library + test binaries |
| `wasm32-unknown-unknown` | `wasm` | On-chain smart contract | ✗ No | `target/wasm32-unknown-unknown/release/anchorkit.wasm` |
| `x86_64-unknown-linux-gnu` | (empty, no-defaults) | Bare library without std or WASM | ✗ No | `target/release/libanchorkit.rlib` |

### ✗ Invalid Configurations

These combinations will fail to compile:

- `wasm` + `std` together (mutually exclusive)
- `--no-default-features` without `--features wasm` (missing core contract code)
- Any build targeting WASM with `reqwest`, `clap`, or filesystem dependencies

## Feature Flags

### `std` (Default)

**Enabled by default.** Includes standard library support and all host-only modules.

**Pulls in:**
- `clap` — CLI argument parsing
- `reqwest` — HTTP client for fetching anchor responses
- `aes-gcm`, `argon2`, `rpassword` — Encrypted credential storage
- `src/main.rs` — CLI binary (wrapped with `#[cfg(feature = "std")]`)
- `src/config.rs::load_runtime_config_file()` — File-based config loading

**Use when:**
- Building the `anchorkit` CLI binary
- Running tests locally
- Creating native service binaries

**Example:**
```bash
# Standard build (includes CLI)
cargo build --release

# Explicit std feature (same as above)
cargo build --release --features std
```

### `wasm`

**Required for WASM / Soroban deployment.** Disables std and all host-only code.

**Disables:**
- Standard library (`#![no_std]`)
- All CLI dependencies (`clap`, `reqwest`, `rpassword`, `aes-gcm`, `argon2`)
- File I/O and networking modules
- `src/main.rs` binary (not compiled)

**Enables:**
- WASM-compatible modules only
- Contract layer (`contract::AnchorKitContract`)
- SEP normalization layers (`sep6`, `sep24`, `sep38`)
- On-chain utilities (`sep10_jwt`, `deterministic_hash`, `transaction_state_tracker`, etc.)

**Use when:**
- Compiling to `wasm32-unknown-unknown` for Soroban
- Building the smart contract
- Targeting resource-constrained environments

**Example:**
```bash
# WASM build for Soroban
cargo build --release \
  --target wasm32-unknown-unknown \
  --no-default-features \
  --features wasm
```

### `mock-only`

**Optional.** Enables mock/test helpers for unit testing.

**Used in:**
- Test suites
- Development environments
- Benchmarking

**Example:**
```bash
cargo test --features mock-only
```

### `stress-tests`

**Optional.** Enables load-simulation and stress-test suite.

**Used in:**
- Performance evaluation
- Capacity planning

**Example:**
```bash
cargo test --features stress-tests
```

## Feature-Gated Code

### Conditional Compilation Rules

1. **CLI binary** (`src/main.rs`)
   - Guarded with `#![cfg(feature = "std")]` at the top of the file
   - Only compiled when `std` feature is present
   - Depends on `clap`, `reqwest`, `rpassword`, `aes-gcm`, `argon2`

2. **Config file loading** (`src/config.rs`)
   - `parse_runtime_config_str()` — Available in all builds (no I/O)
   - `load_runtime_config_file()` — Guarded with `#[cfg(feature = "std")]`

3. **Library exports** (`src/lib.rs`)
   - Core SEP modules, validators, and contract layer — Available in all builds
   - `config::*` types and functions — Guarded with `#[cfg(feature = "std")]`
   - Keystore/credential functions — Only in CLI (behind `#![cfg(feature = "std")]` in main.rs)

### Dependency Feature Flags

All std-only dependencies are marked as `optional = true` and pulled in by the `std` feature:

```toml
[dependencies]
clap = { version = "4.5", optional = true }
reqwest = { version = "0.12", optional = true }
aes-gcm = { version = "0.10.3", optional = true }
argon2 = { version = "0.5.3", optional = true }
rpassword = { version = "7.3", optional = true }

[features]
std = ["clap", "reqwest", "aes-gcm", "argon2", "rpassword"]
```

## Build Commands Reference

### Native (Development & Testing)

```bash
# Build with default features (std)
cargo build

# Build release binary with optimizations
cargo build --release

# Run tests
cargo test
cargo test --release

# Run with custom features
cargo test --features mock-only,stress-tests
```

### WASM (Soroban Deployment)

```bash
# Install WASM target (one-time setup)
rustup target add wasm32-unknown-unknown

# Build WASM contract
cargo build --release \
  --target wasm32-unknown-unknown \
  --no-default-features \
  --features wasm

# Result: target/wasm32-unknown-unknown/release/anchorkit.wasm
```

### Verification

```bash
# Test both build paths
./scripts/test_build_matrix.sh

# Verbose output
./scripts/test_build_matrix.sh --verbose

# Clean rebuild
./scripts/test_build_matrix.sh --clean
```

## Common Build Issues

### Issue: "cannot find crate `clap`" when building WASM

**Cause:** Building WASM without disabling default features.

**Fix:**
```bash
# Wrong:
cargo build --target wasm32-unknown-unknown

# Correct:
cargo build --target wasm32-unknown-unknown --no-default-features --features wasm
```

### Issue: "unresolved import `std::fs`" in WASM build

**Cause:** Code is trying to use `std` without the feature guard.

**Fix:** Ensure file I/O code is wrapped with `#[cfg(feature = "std")]`:
```rust
#[cfg(feature = "std")]
pub fn load_config(path: &Path) -> Result<Config, String> {
    use std::fs;
    let content = fs::read_to_string(path)?;
    // ...
}
```

### Issue: CLI binary not created when building

**Cause:** Building without the `std` feature.

**Fix:**
```bash
# Ensure std is included:
cargo build --release --features std

# Or use default:
cargo build --release
```

## Architecture Diagram

```
┌─────────────────────────────────────────────────────────────┐
│                    src/lib.rs (core)                        │
│  ┌──────────┬──────────────┬──────────┬──────────────────┐  │
│  │ contract │ sep6/sep24   │ sep38    │ validators       │  │
│  │ sep10_jwt│ rate_limiter │ domain   │ response_*       │  │
│  │ deterministic_hash      │ retry    │ streaming_monitor│  │
│  └──────────┴──────────────┴──────────┴──────────────────┘  │
│                 ✓ Works in both std & WASM                   │
└─────────────────────────────────────────────────────────────┘
                            ▲
                  ┌─────────┴─────────┐
                  │                   │
         ┌────────▼───────────┐  ┌────▼──────────────┐
         │  std Feature       │  │  wasm Feature     │
         │  (Native)          │  │  (WASM/Soroban)   │
         └────────┬───────────┘  └────┬──────────────┘
                  │                   │
         ┌────────▼───────────┐       │
         │ Std Dependencies   │       │
         ├────────────────────┤       │
         │ - clap (CLI)       │       │
         │ - reqwest (HTTP)   │       │
         │ - aes-gcm (crypto) │       │
         │ - argon2 (KDF)     │       │
         │ - rpassword (PIN)  │       │
         └────────┬───────────┘       │
                  │                   │
         ┌────────▼───────────┐  ┌────▼──────────────┐
         │ src/main.rs        │  │ (not compiled)    │
         │ (CLI binary)       │  │                   │
         │ - deploy           │  │                   │
         │ - register         │  │                   │
         │ - attest           │  │                   │
         │ - credentials      │  │                   │
         └────────────────────┘  └───────────────────┘
```

## Testing the Build Matrix

### Automated Testing

Run the comprehensive build matrix test:

```bash
./scripts/test_build_matrix.sh
```

This script:
1. ✓ Builds native release binary
2. ✓ Builds WASM smart contract
3. ✓ Builds no-std library
4. ✓ Verifies feature isolation
5. ✓ Runs test suite
6. ✓ Reports all artifacts

### Manual Testing

```bash
# 1. Native build succeeds and produces CLI
cargo build --release
test -f target/release/anchorkit && echo "✓ CLI created"

# 2. WASM build succeeds
cargo build --release --target wasm32-unknown-unknown --no-default-features --features wasm
test -f target/wasm32-unknown-unknown/release/anchorkit.wasm && echo "✓ WASM created"

# 3. Verify WASM binary size is reasonable
ls -lh target/wasm32-unknown-unknown/release/anchorkit.wasm

# 4. Library-only build works
cargo build --release --lib --no-default-features

# 5. Tests pass
cargo test --release
```

## Production Deployment

### For Soroban Smart Contracts

1. Build WASM:
   ```bash
   cargo build --release \
     --target wasm32-unknown-unknown \
     --no-default-features \
     --features wasm
   ```

2. Use the artifact:
   ```bash
   stellar contract deploy \
     --wasm target/wasm32-unknown-unknown/release/anchorkit.wasm \
     --rpc-url <RPC_URL> \
     --source <SOURCE_KEY> \
     --network-passphrase "<PASSPHRASE>"
   ```

### For Native Services

1. Build CLI:
   ```bash
   cargo build --release
   ```

2. Deploy binary:
   ```bash
   ./target/release/anchorkit deploy --network testnet
   ```

## Maintenance Guidelines

### When Adding New Dependencies

1. **For std-only features:**
   - Add as `optional = true`
   - Include in the `std` feature list
   - Guard usages with `#[cfg(feature = "std")]`

2. **For WASM-compatible features:**
   - Add as a regular (required) dependency
   - Ensure it targets `no_std` and `wasm32-unknown-unknown`
   - No feature guards needed

### When Adding New Modules

1. **For core modules (usable in both environments):**
   - No feature guards needed
   - Add to `src/lib.rs` normally
   - Example: `sep10_jwt`, `rate_limiter`

2. **For std-only modules:**
   - Create with `#![cfg(feature = "std")]` at the top
   - Or gate individual functions with `#[cfg(feature = "std")]`
   - Example: `config.rs` (partially), `main.rs` (entirely)

### When Modifying Cargo.toml

- Keep `default = ["std"]` for ergonomic native development
- Ensure the `std` feature properly pulls in all std-only deps
- Verify `wasm` feature is self-contained (no std deps)
- Update this document with any new feature combinations

## See Also

- [Cargo Features Documentation](https://doc.rust-lang.org/cargo/reference/features.html)
- [Soroban SDK Rust Docs](https://docs.rs/soroban-sdk/)
- [Stellar Ecosystem Proposals (SEPs)](https://github.com/stellar/stellar-protocol/tree/master/ecosystem)
