#!/usr/bin/env bash
# Storybook asset validation
# Checks that every HTML page in storybook/ is well-formed and that all
# internal href/src links resolve to existing files.
#
# Usage:
#   bash scripts/validate_storybook.sh
#
# Exit codes:
#   0  all checks passed
#   1  one or more checks failed

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
STORYBOOK_DIR="$SCRIPT_DIR/../storybook"

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

PASS=0
FAIL=0
WARN=0

echo "=== AnchorKit Storybook Validation ==="
echo "Directory: $STORYBOOK_DIR"
echo ""

# ── 1. Check all expected pages exist ─────────────────────────────────────────
echo "1. Checking required pages..."

REQUIRED_PAGES=(
    "index.html"
    "sdk-config-form.html"
    "status-monitor.html"
    "webhook-monitor.html"
    "wallet-connector.html"
    "loading-states.html"
    "error-states.html"
    "success-states.html"
    "anchor-capability-card.html"
    "json-viewer.html"
    "precision-fintech.html"
)

for page in "${REQUIRED_PAGES[@]}"; do
    path="$STORYBOOK_DIR/$page"
    if [ -f "$path" ]; then
        echo -e "   ${GREEN}✓${NC} $page"
        ((PASS++))
    else
        echo -e "   ${RED}✗${NC} $page — MISSING"
        ((FAIL++))
    fi
done
echo ""

# ── 2. Check HTML files are non-empty and contain <html> ──────────────────────
echo "2. Checking HTML structure..."

for html_file in "$STORYBOOK_DIR"/*.html; do
    name=$(basename "$html_file")
    if [ ! -s "$html_file" ]; then
        echo -e "   ${RED}✗${NC} $name — empty file"
        ((FAIL++))
        continue
    fi
    if ! grep -qi "<html" "$html_file"; then
        echo -e "   ${RED}✗${NC} $name — missing <html> tag"
        ((FAIL++))
        continue
    fi
    if ! grep -qi "<title" "$html_file"; then
        echo -e "   ${YELLOW}⚠${NC} $name — missing <title> tag"
        ((WARN++))
    else
        echo -e "   ${GREEN}✓${NC} $name"
        ((PASS++))
    fi
done
echo ""

# ── 3. Check internal href links resolve ──────────────────────────────────────
echo "3. Checking internal links..."

BROKEN=0
for html_file in "$STORYBOOK_DIR"/*.html; do
    name=$(basename "$html_file")
    # Extract href="*.html" values (relative links only, skip http/https/#)
    while IFS= read -r raw_link; do
        # Strip surrounding href=" and trailing "
        link=$(echo "$raw_link" | sed 's/^href="//;s/"$//')
        target="$STORYBOOK_DIR/$link"
        if [ ! -f "$target" ]; then
            echo -e "   ${RED}✗${NC} $name → $link (broken)"
            ((BROKEN++))
            ((FAIL++))
        fi
    done < <(grep -oE 'href="[^"#][^"]*\.html"' "$html_file" 2>/dev/null || true)
done

if [ "$BROKEN" -eq 0 ]; then
    echo -e "   ${GREEN}✓${NC} All internal links resolve"
    ((PASS++))
fi
echo ""

# ── 4. Check for common issues ────────────────────────────────────────────────
echo "4. Checking for common issues..."

for html_file in "$STORYBOOK_DIR"/*.html; do
    name=$(basename "$html_file")
    # Warn on absolute /path references that won't work when opened as file://
    if grep -qE 'src="/' "$html_file" 2>/dev/null; then
        echo -e "   ${YELLOW}⚠${NC} $name — absolute src path found (may break in file:// context)"
        ((WARN++))
    fi
done
echo -e "   ${GREEN}✓${NC} No blocking issues found"
echo ""

# ── Summary ───────────────────────────────────────────────────────────────────
echo "========================================"
echo "Results: ${PASS} passed, ${FAIL} failed, ${WARN} warnings"
echo ""

if [ "$FAIL" -gt 0 ]; then
    echo -e "${RED}✗ Validation failed.${NC} Fix the issues above before publishing."
    exit 1
else
    if [ "$WARN" -gt 0 ]; then
        echo -e "${YELLOW}⚠ Validation passed with warnings.${NC}"
    else
        echo -e "${GREEN}✓ All storybook assets are valid.${NC}"
    fi
    echo ""
    echo "To view the storybook locally:"
    echo "  open storybook/index.html"
    echo ""
    echo "To regenerate a component, edit the corresponding .html file in storybook/."
    echo "Each file is self-contained — no build step required."
    exit 0
fi
