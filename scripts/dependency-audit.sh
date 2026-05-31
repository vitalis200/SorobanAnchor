#!/usr/bin/env bash
# Dependency audit script for SorobanAnchor.
#
# Usage:
#   ./scripts/dependency-audit.sh           # full audit
#   ./scripts/dependency-audit.sh --quick   # licenses only, no security scan
#
# Requires: cargo, and optionally cargo-audit and cargo-license.
# Install optional tools:
#   cargo install cargo-audit cargo-license

set -euo pipefail

QUICK=false
for arg in "$@"; do
    [[ "$arg" == "--quick" ]] && QUICK=true
done

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

pass() { echo -e "  ${GREEN}✓${NC} $*"; }
fail() { echo -e "  ${RED}✗${NC} $*"; }
warn() { echo -e "  ${YELLOW}⚠${NC} $*"; }
section() { echo -e "\n${BOLD}${CYAN}$*${NC}"; }

echo ""
echo -e "${BOLD}SorobanAnchor Dependency Audit${NC}"
echo -e "Date: $(date -u '+%Y-%m-%d %H:%M UTC')"
echo -e "Branch: $(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo 'unknown')"
echo ""

FAILURES=0

# ── 1. Cargo.lock freshness ────────────────────────────────────────────────────
section "1. Lock file freshness"

if cargo update --dry-run 2>&1 | grep -q "Updating"; then
    warn "Cargo.lock is out of date — run 'cargo update' to pick up patch releases"
else
    pass "Cargo.lock is up to date"
fi

# ── 2. License compliance ─────────────────────────────────────────────────────
section "2. License compliance"

DENIED_LICENSES="GPL AGPL SSPL"

if command -v cargo-license &>/dev/null || cargo license --version &>/dev/null 2>&1; then
    echo ""
    echo "  Direct dependency licenses:"
    echo "  ----------------------------"
    # List direct deps only (from Cargo.toml)
    cargo license --color never --avoid-build-deps 2>/dev/null \
        | grep -v "^anchorkit" \
        | while IFS= read -r line; do
            crate=$(echo "$line" | awk '{print $1}')
            license=$(echo "$line" | sed 's/^[^ ]* //')

            denied=false
            for dl in $DENIED_LICENSES; do
                if echo "$license" | grep -qi "$dl"; then
                    denied=true; break
                fi
            done

            if $denied; then
                echo -e "  ${RED}✗ DENIED${NC}  $crate — $license"
                FAILURES=$((FAILURES + 1))
            else
                echo -e "  ${GREEN}✓${NC}        $crate — $license"
            fi
        done
else
    warn "cargo-license not installed. Install with: cargo install cargo-license"
    echo ""
    echo "  Direct dependencies (via cargo metadata):"
    cargo metadata --no-deps --format-version 1 2>/dev/null \
        | python3 -c "
import sys, json
data = json.load(sys.stdin)
pkg = next((p for p in data['packages'] if p['name'] == 'anchorkit'), None)
if not pkg:
    print('    (package not found)')
    sys.exit(0)
deps = sorted(set(d['name'] for d in pkg['dependencies']))
for d in deps:
    print(f'    - {d} (check crates.io or install cargo-license for license info)')
" 2>/dev/null || echo "    (cargo metadata unavailable; install cargo-license for full report)"
fi

# ── 3. Security vulnerabilities ───────────────────────────────────────────────
section "3. Security vulnerability scan"

if $QUICK; then
    warn "Skipped (--quick mode). Run without --quick for a full security scan."
elif command -v cargo-audit &>/dev/null || cargo audit --version &>/dev/null 2>&1; then
    echo ""
    if cargo audit --deny warnings 2>&1; then
        pass "No known vulnerabilities found"
    else
        fail "Security vulnerabilities detected — see output above"
        FAILURES=$((FAILURES + 1))
    fi
else
    warn "cargo-audit not installed. Install with: cargo install cargo-audit"
    warn "Skipping vulnerability scan."
fi

# ── 4. Dependency count ───────────────────────────────────────────────────────
section "4. Dependency statistics"

TOTAL=$(grep -c '^name = ' Cargo.lock || echo 0)
DIRECT=$(grep -E '^[a-z0-9_-]+ =' Cargo.toml | grep -v '^\[' | wc -l | tr -d ' ')
echo "  Total transitive dependencies : $TOTAL"
echo "  Direct dependencies           : $DIRECT"

if [[ "$TOTAL" -gt 300 ]]; then
    warn "Dependency count ($TOTAL) is high — consider auditing for unused transitive deps"
else
    pass "Dependency count ($TOTAL) is within acceptable range"
fi

# ── 5. Unused dependencies check ─────────────────────────────────────────────
section "5. Unused dependency hint"

echo "  Tip: run 'cargo +nightly udeps' to detect unused dependencies."
echo "  Install with: cargo install cargo-udeps"

# ── Summary ───────────────────────────────────────────────────────────────────
echo ""
echo -e "${BOLD}────────────────────────────────────────${NC}"
if [[ "$FAILURES" -eq 0 ]]; then
    echo -e "${GREEN}${BOLD}✅ Audit complete — no issues found${NC}"
    exit 0
else
    echo -e "${RED}${BOLD}❌ Audit found $FAILURES issue(s) — review output above${NC}"
    exit 1
fi
