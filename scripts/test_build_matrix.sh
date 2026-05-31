#!/bin/bash
#
# test_build_matrix.sh
# ═════════════════════════════════════════════════════════════════════════════
#
# Comprehensive build matrix test for SorobanAnchor.
# Tests both the native (std) and WASM (no_std) build paths to ensure proper
# feature separation and environment abstraction.
#
# Usage:
#   ./scripts/test_build_matrix.sh [--help|--verbose|--clean]
#
# Options:
#   --help      Show this help message
#   --verbose   Print detailed build output (not just errors)
#   --clean     Run `cargo clean` before tests
#

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
VERBOSE="${VERBOSE:-0}"
CLEAN_BUILD="${CLEAN_BUILD:-0}"

# Color codes for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# ═════════════════════════════════════════════════════════════════════════════
# Functions
# ═════════════════════════════════════════════════════════════════════════════

print_header() {
    echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
    echo -e "${BLUE}  $1${NC}"
    echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
}

print_subheader() {
    echo -e "${YELLOW}→ $1${NC}"
}

print_success() {
    echo -e "${GREEN}✓ $1${NC}"
}

print_error() {
    echo -e "${RED}✗ $1${NC}"
}

print_info() {
    echo -e "${BLUE}ℹ $1${NC}"
}

# ─────────────────────────────────────────────────────────────────────────────
# Test: Standard library build (native + CLI)
# ─────────────────────────────────────────────────────────────────────────────

test_std_build() {
    print_subheader "Building with std feature (default, includes CLI)"
    
    if CARGO_TERM_COLOR=always cargo build --release 2>&1 | tee /tmp/std_build.log | grep -E "error|warning|Compiling|Finished"; then
        if grep -q "Finished" /tmp/std_build.log; then
            print_success "Standard library build succeeded"
            
            # Verify the binary exists
            if [ -f "target/release/anchorkit" ]; then
                print_success "CLI binary created at target/release/anchorkit"
                return 0
            else
                print_error "CLI binary not found at target/release/anchorkit"
                return 1
            fi
        else
            print_error "Build did not complete successfully"
            return 1
        fi
    else
        print_error "Standard library build failed"
        cat /tmp/std_build.log | grep -A 5 "error" || true
        return 1
    fi
}

# ─────────────────────────────────────────────────────────────────────────────
# Test: WASM build (no_std, no default features)
# ─────────────────────────────────────────────────────────────────────────────

test_wasm_build() {
    print_subheader "Building WASM target (no default features, no CLI)"
    
    # First, ensure the WASM target is installed
    if ! rustup target list --installed | grep -q "wasm32-unknown-unknown"; then
        print_info "Installing wasm32-unknown-unknown target..."
        rustup target add wasm32-unknown-unknown
    fi
    
    if CARGO_TERM_COLOR=always cargo build --release --target wasm32-unknown-unknown --no-default-features --features wasm 2>&1 | tee /tmp/wasm_build.log | grep -E "error|warning|Compiling|Finished"; then
        if grep -q "Finished" /tmp/wasm_build.log; then
            print_success "WASM build succeeded"
            
            # Verify the WASM artifact exists
            WASM_FILE="target/wasm32-unknown-unknown/release/anchorkit.wasm"
            if [ -f "$WASM_FILE" ]; then
                SIZE=$(ls -lh "$WASM_FILE" | awk '{print $5}')
                print_success "WASM artifact created: $WASM_FILE ($SIZE)"
                return 0
            else
                print_error "WASM artifact not found at $WASM_FILE"
                return 1
            fi
        else
            print_error "WASM build did not complete successfully"
            return 1
        fi
    else
        print_error "WASM build failed"
        cat /tmp/wasm_build.log | grep -A 10 "error" || true
        return 1
    fi
}

# ─────────────────────────────────────────────────────────────────────────────
# Test: No-std library build (for verification)
# ─────────────────────────────────────────────────────────────────────────────

test_nostd_lib_build() {
    print_subheader "Building library only (no_std, native)"
    
    if CARGO_TERM_COLOR=always cargo build --release --lib --no-default-features 2>&1 | tee /tmp/nostd_lib.log | grep -E "error|warning|Compiling|Finished"; then
        if grep -q "Finished" /tmp/nostd_lib.log; then
            print_success "No-std library build succeeded"
            return 0
        else
            print_error "No-std library build did not complete successfully"
            return 1
        fi
    else
        print_error "No-std library build failed"
        cat /tmp/nostd_lib.log | grep -A 10 "error" || true
        return 1
    fi
}

# ─────────────────────────────────────────────────────────────────────────────
# Test: Check that std-only modules are not exported in WASM builds
# ─────────────────────────────────────────────────────────────────────────────

test_feature_isolation() {
    print_subheader "Verifying feature isolation (main.rs should not compile in WASM)"
    
    # Attempt to check that main.rs is gated
    if grep -q "#\[cfg(feature = \"std\")\]" "$PROJECT_ROOT/src/main.rs"; then
        print_success "main.rs is correctly guarded with #[cfg(feature = \"std\")]"
    else
        print_error "main.rs is not guarded with feature gate"
        return 1
    fi
    
    # Verify config.rs has std gates
    if grep -q "#\[cfg(feature = \"std\")\]" "$PROJECT_ROOT/src/config.rs"; then
        print_success "config.rs std functions are correctly guarded"
    else
        print_error "config.rs does not have std feature guards"
        return 1
    fi
    
    return 0
}

# ─────────────────────────────────────────────────────────────────────────────
# Test: Run the test suite
# ─────────────────────────────────────────────────────────────────────────────

test_suite() {
    print_subheader "Running test suite (std environment)"
    
    if CARGO_TERM_COLOR=always cargo test --release 2>&1 | tee /tmp/test.log | grep -E "test result|running|FAILED"; then
        if grep -q "test result: ok" /tmp/test.log; then
            print_success "Test suite passed"
            return 0
        else
            print_error "Some tests failed"
            return 1
        fi
    else
        print_error "Test execution failed"
        return 1
    fi
}

# ─────────────────────────────────────────────────────────────────────────────
# Main
# ─────────────────────────────────────────────────────────────────────────────

main() {
    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --help)
                echo "SorobanAnchor Build Matrix Test"
                echo ""
                echo "Usage: $0 [--help|--verbose|--clean]"
                echo ""
                echo "Options:"
                echo "  --help      Show this help message"
                echo "  --verbose   Print full build output"
                echo "  --clean     Run cargo clean before tests"
                exit 0
                ;;
            --verbose)
                VERBOSE=1
                shift
                ;;
            --clean)
                CLEAN_BUILD=1
                shift
                ;;
            *)
                echo "Unknown option: $1"
                exit 1
                ;;
        esac
    done
    
    cd "$PROJECT_ROOT"
    
    print_header "SorobanAnchor Build Matrix Test"
    print_info "Testing environment separation for std vs. WASM builds"
    echo ""
    
    # Clean if requested
    if [ "$CLEAN_BUILD" = "1" ]; then
        print_subheader "Cleaning previous builds..."
        cargo clean
        echo ""
    fi
    
    # Track overall success
    FAILED=0
    
    # Run tests
    print_header "Build Matrix Tests"
    echo ""
    
    print_header "1. Native (std) Build Path"
    if ! test_std_build; then
        FAILED=$((FAILED + 1))
    fi
    echo ""
    
    print_header "2. WASM Build Path"
    if ! test_wasm_build; then
        FAILED=$((FAILED + 1))
    fi
    echo ""
    
    print_header "3. No-std Library Build"
    if ! test_nostd_lib_build; then
        FAILED=$((FAILED + 1))
    fi
    echo ""
    
    print_header "4. Feature Isolation Checks"
    if ! test_feature_isolation; then
        FAILED=$((FAILED + 1))
    fi
    echo ""
    
    print_header "5. Test Suite"
    if ! test_suite; then
        FAILED=$((FAILED + 1))
    fi
    echo ""
    
    # Summary
    print_header "Test Summary"
    if [ "$FAILED" = "0" ]; then
        print_success "All build matrix tests passed!"
        echo ""
        print_info "Build artifacts:"
        echo "  - Native CLI: target/release/anchorkit"
        echo "  - WASM:       target/wasm32-unknown-unknown/release/anchorkit.wasm"
        exit 0
    else
        print_error "$FAILED test suite(s) failed"
        echo ""
        print_info "See output above for details. Build logs saved to /tmp/:"
        echo "  - /tmp/std_build.log"
        echo "  - /tmp/wasm_build.log"
        echo "  - /tmp/nostd_lib.log"
        echo "  - /tmp/test.log"
        exit 1
    fi
}

main "$@"
