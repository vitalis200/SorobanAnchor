# Code Quality Standards

AnchorKit maintains high code quality through automated formatting, linting, and testing. This document explains the standards and tools used.

## Overview

| Tool | Purpose | Config | Command |
|------|---------|--------|---------|
| **rustfmt** | Code formatting | `rustfmt.toml` | `cargo fmt --all` |
| **clippy** | Linting & best practices | `.clippy.toml` | `cargo clippy --all-targets --all-features` |
| **cargo test** | Unit & integration tests | `Cargo.toml` | `cargo test` |

## Quick Start

### Before committing code:

```bash
make check
```

This runs all checks (formatting, linting, tests) and ensures your code meets project standards.

### Individual commands:

```bash
# Auto-fix formatting
make fmt

# Check formatting without modifying
make fmt-check

# Run linting checks
make lint

# Run tests
make test

# Build release binary
make build

# Build WASM target
make wasm
```

## Formatting with rustfmt

**Configuration:** `rustfmt.toml`

AnchorKit uses `rustfmt` to enforce consistent code style. Key settings:

- **Line length:** 100 characters
- **Indentation:** 4 spaces
- **Import grouping:** std, external crates, internal
- **Trailing commas:** Vertical style
- **Comment wrapping:** 80 characters

### Auto-format code:

```bash
cargo fmt --all
```

### Check formatting without modifying:

```bash
cargo fmt --all -- --check
```

### Format a specific file:

```bash
cargo fmt -- src/contract.rs
```

## Linting with Clippy

**Configuration:** `.clippy.toml`

AnchorKit uses `clippy` with strict warnings-as-errors policy. All clippy warnings must be resolved.

Key settings:

- **Cognitive complexity:** Max 30
- **Type complexity:** Max 500
- **Function arguments:** Max 8
- **Unsafe blocks:** Discouraged

### Run linting checks:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

### Run clippy with explanations:

```bash
cargo clippy --all-targets --all-features -- -D warnings -W clippy::all
```

### Suppress a specific lint (use sparingly):

```rust
#[allow(clippy::lint_name)]
fn my_function() {
    // implementation
}
```

Always add a comment explaining why the lint is suppressed.

## Testing

**Configuration:** `Cargo.toml` (dev-dependencies)

All code changes should include tests. Run tests with:

```bash
cargo test
```

Run tests with output:

```bash
cargo test -- --nocapture
```

Run a specific test:

```bash
cargo test test_name
```

Run tests for a specific module:

```bash
cargo test --lib module_name
```

## CI/CD Pipeline

GitHub Actions automatically runs code quality checks on every push and pull request:

1. **Formatting check** — `cargo fmt -- --check`
2. **Linting** — `cargo clippy -- -D warnings`
3. **Tests** — `cargo test --all-features`
4. **WASM build** — `cargo build --target wasm32-unknown-unknown`

All checks must pass before merging to `main`.

## Pre-commit Hooks

To automatically run checks before committing, install the pre-commit hook:

### Linux/macOS:

```bash
bash scripts/setup-hooks.sh
```

### Windows:

```bash
bash scripts/setup-hooks.sh
```

Or manually:

```bash
cp scripts/pre-commit-hook.sh .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
```

The hook will run:
1. Formatting check
2. Clippy linting
3. Tests

If any check fails, the commit is blocked. Fix the issues and try again.

## Common Issues & Solutions

### Formatting conflicts

If `rustfmt` and `clippy` disagree, `rustfmt` takes precedence:

```bash
cargo fmt --all
cargo clippy --all-targets --all-features
```

### Clippy false positives

If you believe a clippy warning is incorrect, suppress it with a comment:

```rust
#[allow(clippy::lint_name)]
// SAFETY: This is safe because...
fn my_function() {
    // implementation
}
```

### Performance issues

If tools are slow, run them on specific targets:

```bash
# Lint only library code
cargo clippy --lib -- -D warnings

# Format only src directory
cargo fmt -- src/
```

### Unstable formatting

If `cargo fmt` produces different output on different runs, check:

1. Rust version: `rustc --version`
2. rustfmt version: `rustfmt --version`
3. `rustfmt.toml` configuration

Update Rust if needed:

```bash
rustup update
```

## Documentation Standards

All public items should have doc comments:

```rust
/// Brief description.
///
/// Longer explanation if needed.
///
/// # Arguments
///
/// * `param` - Description
///
/// # Returns
///
/// Description of return value
///
/// # Errors
///
/// Conditions that cause errors
///
/// # Examples
///
/// ```rust
/// let result = function(arg);
/// ```
pub fn function(param: Type) -> ReturnType {
    // implementation
}
```

## Benchmarking

For performance-critical code, add benchmarks:

```bash
cargo bench
```

Benchmarks are in `benches/` directory and use `criterion`.

## Profiling

Profile code with:

```bash
cargo build --release
perf record ./target/release/anchorkit
perf report
```

## Resources

- [Rust Style Guide](https://doc.rust-lang.org/1.0.0/style/)
- [Clippy Lints](https://rust-lang.github.io/rust-clippy/)
- [rustfmt Configuration](https://rust-lang.github.io/rustfmt/)
- [Cargo Book](https://doc.rust-lang.org/cargo/)

## Questions?

See [CONTRIBUTING.md](CONTRIBUTING.md) for more information on contributing to AnchorKit.
