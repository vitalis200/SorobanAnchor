#![cfg(feature = "std")]

use std::{fs, path::PathBuf};

use anchorkit::{load_runtime_config_file, parse_runtime_config_str, ConfigFormat};
use jsonschema::JSONSchema;
use serde_json::Value;

const SCHEMA: &str = "config_schema.json";
const CONFIG_DIR: &str = "configs";

fn config_paths() -> Vec<PathBuf> {
    let mut paths: Vec<_> = fs::read_dir(CONFIG_DIR)
        .expect("configs directory should exist")
        .map(|entry| entry.expect("config entry should be readable").path())
        .filter(|path| {
            path.is_file()
                && matches!(
                    path.extension().and_then(|ext| ext.to_str()),
                    Some("json" | "toml")
                )
        })
        .collect();
    paths.sort();
    paths
}

fn config_to_json_value(path: &PathBuf) -> Value {
    let input = fs::read_to_string(path).expect("config should be readable");
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("json") => serde_json::from_str(&input).expect("JSON config should parse"),
        Some("toml") => {
            let value: toml::Value = toml::from_str(&input).expect("TOML config should parse");
            serde_json::to_value(value).expect("TOML config should convert to JSON value")
        }
        _ => panic!("unsupported config extension: {}", path.display()),
    }
}

fn compiled_schema() -> JSONSchema {
    let schema_text = fs::read_to_string(SCHEMA).expect("schema should be readable");
    let schema_json: Value = serde_json::from_str(&schema_text).expect("schema should be JSON");
    JSONSchema::compile(&schema_json).expect("schema should compile as JSON Schema")
}

#[test]
fn config_schema_is_valid_json_schema() {
    let _ = compiled_schema();
}

#[test]
fn all_example_configs_validate_against_schema() {
    let schema = compiled_schema();
    let paths = config_paths();
    assert!(!paths.is_empty(), "expected at least one example config");

    for path in paths {
        let value = config_to_json_value(&path);
        let validation = schema.validate(&value);
        let errors = match validation {
            Ok(()) => Vec::new(),
            Err(errors) => errors.map(|err| err.to_string()).collect(),
        };
        assert!(
            errors.is_empty(),
            "{} failed schema validation:\n{}",
            path.display(),
            errors.join("\n")
        );
    }
}

#[test]
fn all_example_configs_load_with_runtime_parser() {
    let paths = config_paths();
    assert!(!paths.is_empty(), "expected at least one example config");

    for path in paths {
        load_runtime_config_file(&path)
            .unwrap_or_else(|err| panic!("{} failed runtime parsing: {err}", path.display()));
    }
}

#[test]
fn runtime_parser_rejects_unknown_top_level_section() {
    let bad = r#"{
  "contract": { "name": "bad-anchor", "version": "1.0.0", "network": "stellar-testnet" },
  "attestors": {
    "registry": [{
      "name": "kyc-issuer",
      "address": "GBBD6A7KNZF5WNWQEPZP5DYJD2AYUTLXRB6VXJ4RCX4RTNPPQVNF3GQ",
      "role": "kyc-issuer",
      "enabled": true
    }]
  },
  "unsupported": { "value": true }
}"#;

    let result = parse_runtime_config_str(bad, ConfigFormat::Json);
    assert!(result.is_err(), "unsupported top-level section should be rejected");
}

#[test]
fn runtime_parser_rejects_malformed_nested_shape() {
    let bad = r#"{
  "contract": { "name": "bad-anchor", "version": "1.0.0", "network": "stellar-testnet" },
  "attestors": {
    "registry": [{
      "name": "kyc-issuer",
      "address": "GBBD6A7KNZF5WNWQEPZP5DYJD2AYUTLXRB6VXJ4RCX4RTNPPQVNF3GQ",
      "role": "kyc-issuer",
      "enabled": true,
      "mystery_flag": true
    }]
  }
}"#;

    let result = parse_runtime_config_str(bad, ConfigFormat::Json);
    assert!(result.is_err(), "unknown nested attestor field should be rejected");
}

#[test]
fn runtime_parser_rejects_unknown_attestor_references() {
    let bad = r#"{
  "contract": { "name": "bad-anchor", "version": "1.0.0", "network": "stellar-testnet" },
  "attestors": {
    "registry": [{
      "name": "kyc-issuer",
      "address": "GBBD6A7KNZF5WNWQEPZP5DYJD2AYUTLXRB6VXJ4RCX4RTNPPQVNF3GQ",
      "role": "kyc-issuer",
      "enabled": true
    }]
  },
  "operations": {
    "templates": [{
      "id": "missing-attestor",
      "name": "Missing Attestor",
      "attestor": "not-registered",
      "operation_type": "kyc",
      "required_fields": ["user_id"],
      "replay_protection": "enabled"
    }]
  }
}"#;

    let result = parse_runtime_config_str(bad, ConfigFormat::Json);
    assert!(result.is_err(), "unknown operation attestor should be rejected");
}
