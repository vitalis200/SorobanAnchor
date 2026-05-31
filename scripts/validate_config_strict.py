#!/usr/bin/env python3
"""Validate a config file against the config_schema.json schema.

Supports both JSON and TOML formats. For TOML files, converts to JSON before validation.
"""

import json
import sys
from pathlib import Path

try:
    from jsonschema import validate, ValidationError, Draft7Validator
except ImportError:
    print("ERROR: jsonschema not installed. Run: pip install jsonschema", file=sys.stderr)
    sys.exit(2)

try:
    import tomli as tomllib
except ImportError:
    try:
        import tomllib
    except ImportError:
        tomllib = None


def load_config(path: Path) -> dict:
    """Load config file (JSON or TOML) and return parsed dict."""
    with open(path, 'rb') as f:
        content = f.read()

    if path.suffix.lower() == '.toml':
        if tomllib is None:
            print("ERROR: TOML support not available. Install tomli/tomllib.", file=sys.stderr)
            sys.exit(2)
        return tomllib.loads(content.decode('utf-8'))
    else:
        return json.loads(content)


def load_json(path: str) -> dict:
    """Load JSON from file path."""
    with open(path, 'r') as f:
        return json.load(f)


def main() -> int:
    if len(sys.argv) != 3:
        print(f"Usage: {sys.argv[0]} <config_file> <schema_file>", file=sys.stderr)
        return 2

    config_path = Path(sys.argv[1])
    schema_path = Path(sys.argv[2])

    if not config_path.exists():
        print(f"ERROR: Config file not found: {config_path}", file=sys.stderr)
        return 1

    if not schema_path.exists():
        print(f"ERROR: Schema file not found: {schema_path}", file=sys.stderr)
        return 1

    try:
        config = load_config(config_path)
        schema = load_json(str(schema_path))
    except json.JSONDecodeError as e:
        print(f"ERROR: JSON parse error: {e}", file=sys.stderr)
        return 1
    except Exception as e:
        print(f"ERROR: Failed to parse config: {e}", file=sys.stderr)
        return 1

    validator = Draft7Validator(schema)
    errors = sorted(validator.iter_errors(config), key=lambda e: e.path)

    if errors:
        for err in errors:
            path = " -> ".join(str(p) for p in err.absolute_path) or "root"
            print(f"ERROR: {path}: {err.message}", file=sys.stderr)
        return 1

    return 0


if __name__ == "__main__":
    sys.exit(main())