use assert_cmd::Command;
use predicates::str::contains;

fn cmd() -> Command {
    Command::cargo_bin("anchorkit").expect("anchorkit binary not found")
}

// ── Group A: missing contract_id validation ───────────────────────────────────

#[test]
fn test_register_missing_contract_id() {
    cmd()
        .args(["register", "--address", "GABC",
               "--services", "kyc",
               "--sep10-token", "T", "--sep10-issuer", "I",
               "--secret-key", "SABC"])
        .env_remove("ANCHOR_CONTRACT_ID")
        .assert()
        .failure()
        .stderr(contains("--contract-id"))
        .stderr(contains("ANCHOR_CONTRACT_ID"));
}

#[test]
fn test_attest_missing_contract_id() {
    cmd()
        .args(["attest", "--subject", "S", "--payload-hash", "H",
               "--issuer", "I", "--secret-key", "SABC"])
        .env_remove("ANCHOR_CONTRACT_ID")
        .assert()
        .failure()
        .stderr(contains("--contract-id"))
        .stderr(contains("ANCHOR_CONTRACT_ID"));
}

#[test]
fn test_quote_missing_contract_id() {
    cmd()
        .args(["quote", "--from", "USDC", "--to", "XLM",
               "--amount", "100", "--secret-key", "SABC"])
        .env_remove("ANCHOR_CONTRACT_ID")
        .assert()
        .failure()
        .stderr(contains("--contract-id"))
        .stderr(contains("ANCHOR_CONTRACT_ID"));
}

#[test]
fn test_revoke_missing_contract_id() {
    cmd()
        .args(["revoke", "--address", "GABC", "--secret-key", "SABC"])
        .env_remove("ANCHOR_CONTRACT_ID")
        .assert()
        .failure()
        .stderr(contains("--contract-id"))
        .stderr(contains("ANCHOR_CONTRACT_ID"));
}

#[test]
fn test_deploy_upgrade_missing_contract_id() {
    cmd()
        .args(["deploy", "--upgrade", "--secret-key", "SABC"])
        .env_remove("ANCHOR_CONTRACT_ID")
        .assert()
        .failure()
        .stderr(contains("--contract-id"))
        .stderr(contains("ANCHOR_CONTRACT_ID"));
}

// ── Group B: contract_id resolved from env var ────────────────────────────────

/// When ANCHOR_CONTRACT_ID is set the contract-id validation passes; the
/// command then fails for another reason (missing signing key), NOT because
/// of a missing contract ID.
#[test]
fn test_register_contract_id_from_env_passes_validation() {
    cmd()
        .args(["register", "--address", "GABC",
               "--services", "kyc",
               "--sep10-token", "T", "--sep10-issuer", "I"])
        .env("ANCHOR_CONTRACT_ID", "CTEST123")
        .env_remove("ANCHOR_ADMIN_SECRET")
        .assert()
        .failure()
        // Error must be about the signing key, not the contract ID
        .stderr(contains("signing key required"))
        .stderr(predicates::str::contains("--contract-id").not());
}

#[test]
fn test_revoke_contract_id_from_env_passes_validation() {
    cmd()
        .args(["revoke", "--address", "GABC"])
        .env("ANCHOR_CONTRACT_ID", "CTEST123")
        .env_remove("ANCHOR_ADMIN_SECRET")
        .assert()
        .failure()
        .stderr(contains("signing key required"))
        .stderr(predicates::str::contains("--contract-id").not());
}

// ── Group C: network fallback note ────────────────────────────────────────────

#[test]
fn test_network_note_when_stellar_network_unset() {
    // `network list` is a harmless read-only command with no STELLAR_NETWORK dependency.
    // When neither STELLAR_NETWORK nor a default profile is set, a note is printed.
    cmd()
        .args(["network", "list"])
        .env_remove("STELLAR_NETWORK")
        .env("HOME", std::env::temp_dir().to_str().unwrap())
        .assert()
        .success()
        .stderr(contains("note:"))
        .stderr(contains("testnet"));
}

#[test]
fn test_no_network_note_when_stellar_network_is_set() {
    cmd()
        .args(["network", "list"])
        .env("STELLAR_NETWORK", "testnet")
        .assert()
        .success()
        .stderr(predicates::str::contains("note:").not());
}

// ── Group D: `env` subcommand ─────────────────────────────────────────────────

#[test]
fn test_env_command_exits_zero() {
    cmd()
        .args(["env"])
        .env_remove("ANCHOR_CONTRACT_ID")
        .env_remove("STELLAR_NETWORK")
        .env("HOME", std::env::temp_dir().to_str().unwrap())
        .assert()
        .success()
        .stdout(contains("Contract ID"))
        .stdout(contains("Network"));
}

#[test]
fn test_env_command_shows_contract_id_from_env() {
    cmd()
        .args(["env"])
        .env("ANCHOR_CONTRACT_ID", "CTEST123ENVTEST")
        .env("HOME", std::env::temp_dir().to_str().unwrap())
        .assert()
        .success()
        .stdout(contains("CTEST123ENVTEST"))
        .stdout(contains("ANCHOR_CONTRACT_ID"));
}

#[test]
fn test_env_command_shows_network_from_env() {
    cmd()
        .args(["env"])
        .env("STELLAR_NETWORK", "mainnet")
        .env("HOME", std::env::temp_dir().to_str().unwrap())
        .assert()
        .success()
        .stdout(contains("mainnet"))
        .stdout(contains("STELLAR_NETWORK"));
}

#[test]
fn test_env_command_shows_builtin_profiles() {
    cmd()
        .args(["env"])
        .env("HOME", std::env::temp_dir().to_str().unwrap())
        .assert()
        .success()
        .stdout(contains("testnet"))
        .stdout(contains("mainnet"))
        .stdout(contains("futurenet"));
}

// ── Group E: commands that do NOT need contract_id ───────────────────────────

#[test]
fn test_doctor_does_not_require_contract_id() {
    // doctor may exit 0 or 1 depending on the environment, but must NOT
    // emit the contract-id validation error.
    let output = cmd()
        .args(["doctor"])
        .env_remove("ANCHOR_CONTRACT_ID")
        .output()
        .expect("failed to run anchorkit doctor");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("--contract-id (or ANCHOR_CONTRACT_ID) is required"),
        "doctor should not require --contract-id, but got: {stderr}"
    );
}

#[test]
fn test_credentials_list_does_not_require_contract_id() {
    cmd()
        .args(["credentials", "list"])
        .env_remove("ANCHOR_CONTRACT_ID")
        .env("HOME", std::env::temp_dir().to_str().unwrap())
        .assert()
        .success()
        .stderr(predicates::str::contains("--contract-id").not());
}

#[test]
fn test_network_list_does_not_require_contract_id() {
    cmd()
        .args(["network", "list"])
        .env_remove("ANCHOR_CONTRACT_ID")
        .env("HOME", std::env::temp_dir().to_str().unwrap())
        .assert()
        .success()
        .stderr(predicates::str::contains("--contract-id (or ANCHOR_CONTRACT_ID) is required").not());
}

// ── Group F: invalid invocations (clap-level) ────────────────────────────────

#[test]
fn test_missing_subcommand_exits_with_usage() {
    cmd()
        .assert()
        .failure()
        .stderr(contains("Usage"));
}

#[test]
fn test_unknown_subcommand() {
    cmd()
        .args(["foobar"])
        .assert()
        .failure()
        .stderr(contains("unrecognized subcommand"));
}

#[test]
fn test_register_missing_required_address_arg() {
    // --contract-id and signing key provided, but --address is missing → clap error
    cmd()
        .args(["register",
               "--contract-id", "CTEST",
               "--services", "kyc",
               "--sep10-token", "T", "--sep10-issuer", "I",
               "--secret-key", "SABC"])
        .assert()
        .failure()
        .stderr(contains("--address"));
}
