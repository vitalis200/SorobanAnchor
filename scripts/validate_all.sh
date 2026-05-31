#!/usr/bin/env bash
# validate_all.sh — Validate all AnchorKit config files against config_schema.json.
# Supports JSON natively; converts TOML to JSON via yq/dasel (falls back to Python).
# Validates with ajv-cli if available, otherwise falls back to validate_config_strict.py.
set -euo pipefail

SCHEMA="config_schema.json"
CONFIGS_DIR="configs"
VALIDATOR_PY="validate_config_strict.py"
FAILED=0

# ── helpers ────────────────────────────────────────────────────────────────────

die() { echo "❌ $*" >&2; exit 1; }

require_schema() {
  [ -f "$SCHEMA" ] || die "Schema not found: $SCHEMA"
}

# Convert a TOML file to a temp JSON file; print the temp path.
toml_to_json() {
  local toml_file="$1"
  local tmp
  tmp="$(mktemp /tmp/anchorkit_XXXXXX.json)"

  if command -v yq &>/dev/null; then
    yq -o=json '.' "$toml_file" > "$tmp"
  elif command -v dasel &>/dev/null; then
    dasel -f "$toml_file" -r toml -w json '.' > "$tmp"
elif command -v python3 &>/dev/null; then
    python3 - "$toml_file" "$tmp" <<'PYEOF'
import sys, json, pathlib
p = pathlib.Path(sys.argv[1])
try:
    import tomllib
except ImportError:
    try:
        import tomli as tomllib
    except ImportError:
        import toml as tomllib
data = tomllib.loads(p.read_text())
json.dump(data, open(sys.argv[2], "w"), indent=2)
PYEOF
  else
     die "No TOML converter found. Install yq, dasel, or python3+toml."
  fi

  echo "$tmp"
}

# Validate a single JSON file against the schema.
validate_json() {
  local json_file="$1"
  local label="$2"

  if command -v ajv &>/dev/null; then
    ajv validate -s "$SCHEMA" -d "$json_file" --spec=draft7 --errors=text 2>&1 \
      || { echo "  ❌ $label"; return 1; }
  elif [ -f "$VALIDATOR_PY" ] && command -v python3 &>/dev/null; then
    python3 "$VALIDATOR_PY" "$json_file" "$SCHEMA" \
      || { echo "  ❌ $label"; return 1; }
  else
    die "No validator found. Install ajv-cli (npm i -g ajv-cli) or python3+jsonschema."
  fi

  echo "  ✅ $label"
}

# ── main ───────────────────────────────────────────────────────────────────────

require_schema

echo "🔍 AnchorKit Config Validation"
echo "Schema: $SCHEMA"
echo ""

TMPFILES=()
cleanup() { rm -f "${TMPFILES[@]}"; }
trap cleanup EXIT

for file in "$CONFIGS_DIR"/*.json "$CONFIGS_DIR"/*.toml; do
  [ -f "$file" ] || continue
  label="$(basename "$file")"

  if [[ "$file" == *.toml ]]; then
    tmp="$(toml_to_json "$file")"
    TMPFILES+=("$tmp")
    validate_json "$tmp" "$label (converted from TOML)" || FAILED=1
  else
    validate_json "$file" "$label" || FAILED=1
  fi
done

echo ""
if [ "$FAILED" -ne 0 ]; then
  echo "❌ One or more configs failed validation."
  exit 1
fi
echo "✅ All configs valid."
