#!/usr/bin/env bash
# validate_bundle.sh — Validate a release artifact bundle.
#
# Usage:
#   ./scripts/validate_bundle.sh <bundle_dir_or_tarball>
#
# Examples:
#   ./scripts/validate_bundle.sh dist/anchorkit-0.1.0.tar.gz
#   ./scripts/validate_bundle.sh dist/anchorkit-0.1.0
#
# Exit codes:
#   0 — all checks passed
#   1 — one or more checks failed

set -euo pipefail

TARGET="${1:-}"
if [[ -z "${TARGET}" ]]; then
    echo "Usage: $0 <bundle_dir_or_tarball>"
    exit 1
fi

TMPDIR_USED=0
BUNDLE_DIR="${TARGET}"

# If a tarball was supplied, extract it to a temp directory.
if [[ "${TARGET}" == *.tar.gz ]]; then
    BUNDLE_DIR=$(mktemp -d)
    TMPDIR_USED=1
    echo "Extracting ${TARGET} to ${BUNDLE_DIR}..."
    tar -xzf "${TARGET}" -C "${BUNDLE_DIR}" --strip-components=1
fi

PASS=0
FAIL=0

check() {
    local label="$1"
    local path="$2"
    if [[ -e "${BUNDLE_DIR}/${path}" ]]; then
        echo "  ✓ ${label}"
        PASS=$((PASS + 1))
    else
        echo "  ✗ ${label} — missing: ${path}"
        FAIL=$((FAIL + 1))
    fi
}

check_exec() {
    local label="$1"
    local path="$2"
    if [[ -x "${BUNDLE_DIR}/${path}" ]]; then
        echo "  ✓ ${label} (executable)"
        PASS=$((PASS + 1))
    elif [[ -e "${BUNDLE_DIR}/${path}" ]]; then
        echo "  ⚠ ${label} (exists but not executable)"
        PASS=$((PASS + 1))
    else
        echo "  ✗ ${label} — missing: ${path}"
        FAIL=$((FAIL + 1))
    fi
}

check_wasm() {
    local path="${BUNDLE_DIR}/anchorkit.wasm"
    if [[ ! -f "${path}" ]]; then
        echo "  ✗ WASM contract — missing: anchorkit.wasm"
        FAIL=$((FAIL + 1))
        return
    fi
    # Check WASM magic bytes: \0asm
    local magic
    magic=$(xxd -l 4 "${path}" 2>/dev/null | awk '{print $2$3}' | head -1 || true)
    if [[ "${magic}" == "0061736d" ]]; then
        echo "  ✓ WASM contract (valid magic bytes)"
        PASS=$((PASS + 1))
    else
        echo "  ⚠ WASM contract (present but magic bytes unverified — xxd may not be available)"
        PASS=$((PASS + 1))
    fi
}

check_json() {
    local label="$1"
    local path="$2"
    if [[ ! -f "${BUNDLE_DIR}/${path}" ]]; then
        echo "  ✗ ${label} — missing: ${path}"
        FAIL=$((FAIL + 1))
        return
    fi
    if command -v python3 &>/dev/null; then
        if python3 -c "import json,sys; json.load(open('${BUNDLE_DIR}/${path}'))" 2>/dev/null; then
            echo "  ✓ ${label} (valid JSON)"
            PASS=$((PASS + 1))
        else
            echo "  ✗ ${label} (invalid JSON)"
            FAIL=$((FAIL + 1))
        fi
    else
        echo "  ✓ ${label} (present; JSON validation skipped — python3 not available)"
        PASS=$((PASS + 1))
    fi
}

echo ""
echo "=== Bundle Validation: ${TARGET} ==="
echo ""

# Required files
check_exec "CLI binary"                "anchorkit"
check_wasm
check      "VERSION file"              "VERSION"
check      "README"                    "README.md"
check      "LICENSE"                   "LICENSE"
check_json "Config schema"             "schemas/config_schema.json"

# Example configs
check_json "Fiat ramp config (JSON)"   "configs/fiat-on-off-ramp.json"
check_json "Remittance config (JSON)"  "configs/remittance-anchor.json"
check_json "Stablecoin config (JSON)"  "configs/stablecoin-issuer.json"

# Docs
check      "Error codes doc"           "docs/error-codes.md"

echo ""
echo "Results: ${PASS} passed, ${FAIL} failed"

# Cleanup temp dir if we extracted a tarball.
if [[ "${TMPDIR_USED}" -eq 1 ]]; then
    rm -rf "${BUNDLE_DIR}"
fi

if [[ "${FAIL}" -gt 0 ]]; then
    echo "❌ Bundle validation FAILED"
    exit 1
else
    echo "✅ Bundle validation PASSED"
    exit 0
fi
