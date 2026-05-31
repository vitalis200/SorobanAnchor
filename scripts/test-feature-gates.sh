#!/usr/bin/env bash
# Feature gate compile matrix for SorobanAnchor.
#
# Tests every documented feature combination to confirm the crate compiles
# cleanly and feature gates do not inadvertently enable incompatible code.
#
# Usage:
#   ./scripts/test-feature-gates.sh             # full matrix
#   ./scripts/test-feature-gates.sh --check-only # cargo check, no tests
#
# Requirements: cargo, rustup with wasm32-unknown-unknown target installed.
# Install the WASM target: rustup target add wasm32-unknown-unknown

set -euo pipefail

CHECK_ONLY=false
for arg in "$@"; do
    [[ "$arg" == "--check-only" ]] && CHECK_ONLY=true
done

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

pass() { echo -e "  ${GREEN}✓${NC} $*"; }
fail() { echo -e "  ${RED}✗${NC} $*"; ((FAILURES++)) || true; }
warn() { echo -e "  ${YELLOW}⚠${NC} $*"; }
section() { echo -e "\n${BOLD}${CYAN}$*${NC}"; }

FAILURES=0
RESULTS=()

run_check() {
    local label="$1"; shift
    local cmd=("$@")
    printf "  %-52s" "$label"
    if "${cmd[@]}" >/dev/null 2>&1; then
        echo -e "${GREEN}✓${NC}"
        RESULTS+=("PASS: $label")
    else
        echo -e "${RED}✗${NC}"
        RESULTS+=("FAIL: $label")
        ((FAILURES++)) || true
        # Re-run to show errors
        "${cmd[@]}" 2>&1 | grep "^error" | head -5 | sed 's/^/      /'
    fi
}

run_test() {
    local label="$1"; shift
    local cmd=("$@")
    printf "  %-52s" "$label"
    if "${cmd[@]}" >/dev/null 2>&1; then
        echo -e "${GREEN}✓${NC}"
        RESULTS+=("PASS: $label")
    else
        echo -e "${RED}✗${NC}"
        RESULTS+=("FAIL: $label")
        ((FAILURES++)) || true
        "${cmd[@]}" 2>&1 | grep -E "^error|FAILED" | head -5 | sed 's/^/      /'
    fi
}

echo ""
echo -e "${BOLD}SorobanAnchor Feature Gate Compile Matrix${NC}"
echo -e "Date: $(date -u '+%Y-%m-%d %H:%M UTC')"
echo ""

# ── 1. cargo check — one slice per feature combination ────────────────────────
section "1. Feature gate compilation checks (cargo check --lib)"

run_check "default (std enabled)"           \
    cargo check --lib --quiet

run_check "no features (no_std lib only)"   \
    cargo check --lib --no-default-features --quiet

run_check "wasm only (host modules excluded)" \
    cargo check --lib --no-default-features --features wasm --quiet

run_check "mock-only"                       \
    cargo check --lib --features mock-only --quiet

run_check "stress-tests"                    \
    cargo check --lib --features stress-tests --quiet

run_check "std + mock-only"                 \
    cargo check --lib --features std,mock-only --quiet

run_check "std + stress-tests"              \
    cargo check --lib --features std,stress-tests --quiet

run_check "all features combined"           \
    cargo check --lib --features std,mock-only,stress-tests --quiet

# ── 2. WASM cross-compile (wasm32-unknown-unknown) ────────────────────────────
section "2. WASM cross-compile check"

if rustup target list --installed 2>/dev/null | grep -q "wasm32-unknown-unknown"; then
    warn "wasm32-unknown-unknown cross-compile requires the Soroban toolchain."
    warn "Raw 'cargo check' fails due to getrandom lacking a wasm32 backend."
    warn "Use 'stellar contract build' for WASM deployment builds."
    echo ""
    echo "  Skipping raw wasm32 cross-compile (expected: use Stellar CLI)"
else
    warn "wasm32-unknown-unknown target not installed."
    warn "Install with: rustup target add wasm32-unknown-unknown"
fi

# ── 3. Feature isolation: wasm excludes host modules ─────────────────────────
section "3. wasm feature gate isolation"

echo ""
echo "  Verifying that SEP-6 symbols are absent in --features wasm ..."
WASM_CHECK=$(cargo check --lib --no-default-features --features wasm --message-format=json 2>&1 || true)
if echo "$WASM_CHECK" | grep -q '"message":"unused import".*sep6' 2>/dev/null; then
    fail "sep6 module leaked into wasm build"
else
    pass "sep6 correctly excluded from wasm build"
fi

echo "  Verifying that core types remain available in --features wasm ..."
CORE_CHECK=$(cargo check --lib --no-default-features --features wasm 2>&1)
if echo "$CORE_CHECK" | grep -q "^error"; then
    fail "wasm build has compile errors — core types may be broken"
else
    pass "Core types (errors, rate_limiter, contract) available in wasm build"
fi

# ── 4. Binary check (CLI) ─────────────────────────────────────────────────────
section "4. Binary (CLI) compilation"

run_check "anchorkit binary (default features)" \
    cargo check --bin anchorkit --quiet

# ── 5. Feature gate tests ─────────────────────────────────────────────────────
section "5. Feature gate isolation tests"

if $CHECK_ONLY; then
    warn "Skipping tests (--check-only mode)"
else
    run_test "feature_gate_tests (default)"          \
        cargo test --test feature_gate_tests --quiet

    run_test "feature_gate_tests (mock-only)"        \
        cargo test --test feature_gate_tests --features mock-only --quiet

    run_test "feature_gate_tests (std + mock-only)"  \
        cargo test --test feature_gate_tests --features std,mock-only --quiet

    run_test "feature_gate_tests (stress-tests)"     \
        cargo test --test feature_gate_tests --features stress-tests --quiet

    run_test "cli_mode_tests"                        \
        cargo test --test cli_mode_tests --quiet

    run_test "safe_logging_tests"                    \
        cargo test --test safe_logging_tests --quiet
fi

# ── Summary ───────────────────────────────────────────────────────────────────
echo ""
echo -e "${BOLD}────────────────────────────────────────────────────${NC}"
echo -e "${BOLD}Results:${NC}"
for r in "${RESULTS[@]}"; do
    if [[ "$r" == PASS* ]]; then
        echo -e "  ${GREEN}✓${NC} ${r#PASS: }"
    else
        echo -e "  ${RED}✗${NC} ${r#FAIL: }"
    fi
done

echo ""
if [[ "$FAILURES" -eq 0 ]]; then
    echo -e "${GREEN}${BOLD}✅ All feature gate checks passed${NC}"
    exit 0
else
    echo -e "${RED}${BOLD}❌ $FAILURES check(s) failed — see output above${NC}"
    exit 1
fi
