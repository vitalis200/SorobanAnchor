#!/bin/bash
# Quality check script for AnchorKit
# Runs formatting and linting checks locally
# Usage: bash scripts/quality-check.sh [target]
# Targets: all (default), native, wasm

set -e

TARGET="${1:-all}"

echo "🔍 Running quality checks for target: $TARGET"
echo ""

case "$TARGET" in
    all)
        echo "📋 Checking formatting..."
        cargo fmt --all -- --check
        echo "✓ Formatting OK"
        echo ""
        
        echo "🔗 Running clippy on all targets..."
        cargo clippy --all-targets --all-features -- -D warnings
        echo "✓ Clippy OK"
        echo ""
        
        echo "🧪 Running tests..."
        cargo test
        echo "✓ Tests OK"
        ;;
    
    native)
        echo "📋 Checking formatting..."
        cargo fmt --all -- --check
        echo "✓ Formatting OK"
        echo ""
        
        echo "🔗 Running clippy on native targets..."
        cargo clippy --lib --bins --tests --examples -- -D warnings
        echo "✓ Clippy OK"
        echo ""
        
        echo "🧪 Running tests..."
        cargo test
        echo "✓ Tests OK"
        ;;
    
    wasm)
        echo "📋 Checking formatting..."
        cargo fmt --all -- --check
        echo "✓ Formatting OK"
        echo ""
        
        echo "🔗 Running clippy on WASM target..."
        cargo clippy --target wasm32-unknown-unknown --no-default-features --features wasm -- -D warnings
        echo "✓ Clippy OK"
        echo ""
        
        echo "🏗️  Building WASM..."
        cargo build --release --target wasm32-unknown-unknown --no-default-features --features wasm
        echo "✓ WASM build OK"
        ;;
    
    *)
        echo "Error: Unknown target '$TARGET'"
        echo "Valid targets: all, native, wasm"
        exit 1
        ;;
esac

echo ""
echo "✓ All quality checks passed!"
