#!/bin/bash
# Pre-commit hook for AnchorKit
# Install with: cp scripts/pre-commit-hook.sh .git/hooks/pre-commit && chmod +x .git/hooks/pre-commit

set -e

echo "🔍 Running pre-commit checks..."

# Check formatting
echo "  → Checking code formatting..."
if ! cargo fmt --all -- --check > /dev/null 2>&1; then
    echo "  ✗ Formatting issues found. Run 'cargo fmt --all' to fix."
    exit 1
fi
echo "  ✓ Formatting OK"

# Run clippy
echo "  → Running clippy lints..."
if ! cargo clippy --all-targets --all-features -- -D warnings > /dev/null 2>&1; then
    echo "  ✗ Clippy warnings found. Run 'cargo clippy --all-targets --all-features' to see details."
    exit 1
fi
echo "  ✓ Clippy OK"

# Run tests
echo "  → Running tests..."
if ! cargo test --all-features > /dev/null 2>&1; then
    echo "  ✗ Tests failed. Run 'cargo test' to see details."
    exit 1
fi
echo "  ✓ Tests OK"

echo "✓ All pre-commit checks passed!"
exit 0
