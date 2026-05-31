#![cfg(feature = "std")]
//! CLI binary for AnchorKit.
//!
//! This binary is only available when building with the `std` feature (the default).
//! For WASM builds, disable default features:
//!   cargo build --target wasm32-unknown-unknown --no-default-features --features wasm

use clap::{Parser, Subcommand};
use serde::Serialize;
use std::fs::{self, File};
use std::io::{self, ErrorKind, Read};

// ── SecretKey wrapper ──────────────────────────────────────────────────────────

/// Opaque wrapper around a Stellar secret key string.
/// Does not implement Debug or Display to prevent accidental logging.
struct SecretKey(String);

impl SecretKey {
    fn new(s: impl Into<String>) -> Self {
        SecretKey(s.into())
    }
}

impl std::ops::Deref for SecretKey {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl AsRef<std::ffi::OsStr> for SecretKey {
    fn as_ref(&self) -> &std::ffi::OsStr {
        self.0.as_ref()
    }
}

// ── Secret key wrapper (zeroizing) ───────────────────────────────────────────
//
// Prevents accidental secret leakage through Debug/Display, and zeroizes the
// key material when the value is dropped (post-use or on error paths).

struct SecretKey(String);

impl SecretKey {
    fn new(raw: impl Into<String>) -> Self { Self(raw.into()) }
    fn expose(&self) -> &str { &self.0 }
}

impl std::fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretKey([REDACTED])")
    }
}

impl std::fmt::Display for SecretKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl std::ops::Deref for SecretKey {
    type Target = str;
    fn deref(&self) -> &str { &self.0 }
}

impl Drop for SecretKey {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.0.zeroize();
    }
}

// ── Network profile management ────────────────────────────────────────────────

#[derive(Serialize, serde::Deserialize, Clone, Debug)]
struct NetworkProfile {
    name: String,
    rpc_url: String,
    network_passphrase: String,
    horizon_url: Option<String>,
    #[serde(default)]
    is_default: bool,
}

fn networks_path() -> std::path::PathBuf {
    let dir = dirs_home().join(".anchorkit");
    std::fs::create_dir_all(&dir).ok();
    dir.join("networks.json")
}

fn dirs_home() -> std::path::PathBuf {
    std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
}

fn secure_read_file(path: &str) -> Result<String, std::io::Error> {
    let path_buf = std::path::Path::new(path);
    // Ensure the file exists
    if !path_buf.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("file does not exist: {path}"),
        ));
    }
    // Reject symlinks to avoid symlink attacks
    if let Ok(metadata) = path_buf.metadata() {
        if metadata.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("symlink file is not allowed: {path}"),
            ));
        }
    }
    // Ensure it's a regular file
    if let Ok(metadata) = path_buf.metadata() {
        if !metadata.file_type().is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("not a regular file: {path}"),
            ));
        }
    }
    // Open for reading (checks readability)
    let mut file = std::fs::File::open(path_buf)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    Ok(contents)
}

fn load_network_profiles() -> Vec<NetworkProfile> {
    let path = networks_path();
    if !path.exists() { return Vec::new(); }
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    serde_json::from_str(&content).unwrap_or_default()
}

fn save_network_profiles(profiles: &[NetworkProfile]) {
    let path = networks_path();
    let json = serde_json::to_string_pretty(profiles).unwrap_or_default();
    std::fs::write(path, json).ok();
}

fn find_profile<'a>(profiles: &'a [NetworkProfile], name: &str) -> Option<&'a NetworkProfile> {
    profiles.iter().find(|p| p.name == name)
}

fn rpc_url_for(network: &str) -> String {
    let profiles = load_network_profiles();
    if let Some(p) = find_profile(&profiles, network) {
        return p.rpc_url.clone();
    }
    rpc_url(network).to_string()
}

fn passphrase_for(network: &str) -> String {
    let profiles = load_network_profiles();
    if let Some(p) = find_profile(&profiles, network) {
        return p.network_passphrase.clone();
    }
    passphrase(network).to_string()
}

fn default_network() -> String {
    let profiles = load_network_profiles();
    profiles.iter()
        .find(|p| p.is_default)
        .map(|p| p.name.clone())
        .unwrap_or_else(|| "testnet".to_string())
}

/// Return the contract ID to use, checking the per-command arg first, then
/// the global flag / ANCHOR_CONTRACT_ID env var.  Exits with a clear error
/// if neither is set.
fn require_contract_id(global: Option<String>, local: Option<String>, command: &str) -> String {
    local.or(global).unwrap_or_else(|| {
        eprintln!("error: --contract-id (or ANCHOR_CONTRACT_ID) is required for `{command}`");
        eprintln!("hint:  pass --contract-id <ID>  or  export ANCHOR_CONTRACT_ID=<ID>");
        std::process::exit(1);
    })
}

/// Resolve the signing source from flags or environment.
/// Priority: --secret-key > ANCHOR_ADMIN_SECRET > --keypair-file > --credential-name
fn resolve_source(secret_key: Option<&str>, keypair_file: Option<&str>, credential_name: Option<&str>) -> SecretKey {
    if let Some(sk) = secret_key {
        return SecretKey::new(sk);
    }
    if let Ok(sk) = std::env::var("ANCHOR_ADMIN_SECRET") {
        if !sk.is_empty() {
            return SecretKey::new(sk);
        }
    }
    if let Some(path) = keypair_file {
        let raw = match secure_read_file(path) {
            Ok(content) => content,
            Err(e) => {
                eprintln!("error: cannot read keypair file '{path}': {e}");
                std::process::exit(1);
            }
        };
        // Support JSON {"secret_key":"S..."} or plain text.
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
            if let Some(sk) = v.get("secret_key").and_then(|s| s.as_str()) {
                return SecretKey::new(sk);
            }
        }
        return SecretKey::new(raw.trim());
    }
    if let Some(name) = credential_name {
        if no_interactive {
            eprintln!("error: --credential-name requires an interactive password prompt; \
                       use --secret-key, --ephemeral-token, or ANCHOR_ADMIN_SECRET in non-interactive mode");
            std::process::exit(1);
        }
        let password = rpassword::prompt_password("Keystore password: ")
            .unwrap_or_else(|e| { eprintln!("error: failed to read password: {e}"); std::process::exit(1); });
        return keystore_get_decrypted(name, &password);
    }
    eprintln!("error: signing key required — provide one of:");
    eprintln!("  --secret-key <KEY>");
    eprintln!("  export ANCHOR_ADMIN_SECRET=<KEY>");
    eprintln!("  --keypair-file <PATH>");
    eprintln!("  --credential-name <NAME>  (use: anchorkit credentials add --name <NAME>)");
    std::process::exit(1);
}

fn normalize_stellar_public_address(field: &str, address: &str) -> String {
    match normalize_stellar_account_id(address) {
        Ok(normalized) => normalized,
        Err(err) => {
            eprintln!("error: invalid {field}: {0}", err.message);
            std::process::exit(1);
        }
    }
}

// ── RPC helpers ───────────────────────────────────────────────────────────────

fn rpc_url(network: &str) -> &'static str {
    match network {
        "mainnet"   => "https://horizon.stellar.org",
        "futurenet" => "https://rpc-futurenet.stellar.org",
        _           => "https://soroban-testnet.stellar.org",
    }
}

fn passphrase(network: &str) -> &'static str {
    match network {
        "mainnet"   => "Public Global Stellar Network ; September 2015",
        "futurenet" => "Test SDF Future Network ; October 2022",
        _           => "Test SDF Network ; September 2015",
    }
}

fn stellar_invoke(
    contract_id: &str,
    // SECURITY: `source` is a Stellar secret key passed to the Stellar CLI via
    // `--source`.  It is intentionally exposed here because the upstream CLI
    // requires it as a positional argument.  It must never be echoed to stdout
    // or included in log messages; only the exit status and stdout of the child
    // process are surfaced to the caller.
    source: &SecretKey,
    network: &str,
    fn_args: &[&str],
) -> String {
    let url = rpc_url_for(network);
    let phrase = passphrase_for(network);
    let source: &str = source; // coerce &SecretKey → &str for uniform array element type
    let output = std::process::Command::new("stellar")
        .args(["contract", "invoke",
               "--id", contract_id,
               "--source", source,
               "--rpc-url", &url,
               "--network-passphrase", &phrase,
               "--"])
        .args(fn_args)
        .output()
        .unwrap_or_else(|e| { eprintln!("error: failed to run stellar contract invoke — is the Stellar CLI installed? ({e})"); std::process::exit(1); });

    if output.status.success() {
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    } else {
        // Emit only the child's stderr; the secret key is not present there.
        eprintln!("{}", String::from_utf8_lossy(&output.stderr).trim());
        std::process::exit(1);
    }
}

// ── CLI definition ────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "anchorkit", about = "SorobanAnchor CLI")]
struct Cli {
    /// Contract ID to invoke (or set ANCHOR_CONTRACT_ID)
    #[arg(long, global = true, env = "ANCHOR_CONTRACT_ID")]
    contract_id: Option<String>,

    /// Stellar network: testnet | mainnet | futurenet | <custom> (or set STELLAR_NETWORK)
    #[arg(long, global = true, env = "STELLAR_NETWORK")]
    network: Option<String>,

    /// Disable all interactive prompts; batch scripts use this to avoid hanging on input.
    /// Also enabled by setting ANCHORKIT_NO_INTERACTIVE=1.
    #[arg(long, global = true, env = "ANCHORKIT_NO_INTERACTIVE")]
    no_interactive: bool,

    /// One-time ephemeral signing token (highest priority over other key sources; zeroized after use).
    /// Intended for single-operation authorization in automated flows.
    /// Also settable via ANCHORKIT_EPHEMERAL_TOKEN.
    #[arg(long, global = true, env = "ANCHORKIT_EPHEMERAL_TOKEN")]
    ephemeral_token: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Deploy contract to a network
    Deploy {
        #[arg(long, default_value = "testnet")]
        network: String,
        #[arg(long, default_value = "default")]
        source: String,
        /// Admin address for post-deployment initialization
        #[arg(long)]
        admin: Option<String>,
        /// Validate without deploying
        #[arg(long)]
        dry_run: bool,
        /// List deployment history
        #[arg(long)]
        list: bool,
        /// Upgrade an existing contract instead of deploying a new one.
        /// Requires --contract-id (or ANCHOR_CONTRACT_ID) and --secret-key / ANCHOR_ADMIN_SECRET.
        #[arg(long)]
        upgrade: bool,
        /// Secret key used to sign the upgrade transaction (overrides ANCHOR_ADMIN_SECRET)
        #[arg(long)]
        secret_key: Option<String>,
        /// Path to a JSON or plain-text keypair file (used when --secret-key is absent)
        #[arg(long)]
        keypair_file: Option<String>,
    },
    /// Register an attestor
    Register {
        #[arg(long)] address: String,
        #[arg(long, value_delimiter = ',')] services: Vec<String>,
        #[arg(long)] contract_id: Option<String>,
        #[arg(long, default_value = "testnet")] network: String,
        #[arg(long)] secret_key: Option<String>,
        #[arg(long)] keypair_file: Option<String>,
        /// Name of a credential stored in the keystore (alternative to --secret-key)
        #[arg(long)] credential_name: Option<String>,
        #[arg(long)] sep10_token: String,
        #[arg(long)] sep10_issuer: String,
    },
    /// Submit an attestation
    Attest {
        #[arg(long)] subject: String,
        #[arg(long)] payload_hash: String,
        #[arg(long)] contract_id: Option<String>,
        #[arg(long, default_value = "testnet")] network: String,
        #[arg(long)] secret_key: Option<String>,
        #[arg(long)] keypair_file: Option<String>,
        /// Name of a credential stored in the keystore (alternative to --secret-key)
        #[arg(long)] credential_name: Option<String>,
        #[arg(long)] issuer: String,
        #[arg(long)] session_id: Option<u64>,
    },
    /// Get the best quote for a currency pair
    Quote {
        /// Source asset code (e.g. USDC)
        #[arg(long)] from: String,
        /// Destination asset code (e.g. XLM)
        #[arg(long)] to: String,
        /// Amount in base asset units
        #[arg(long)] amount: u64,
        #[arg(long)] contract_id: Option<String>,
        #[arg(long, default_value = "testnet")] network: String,
        #[arg(long)] secret_key: Option<String>,
        #[arg(long)] keypair_file: Option<String>,
        /// Name of a credential stored in the keystore (alternative to --secret-key)
        #[arg(long)] credential_name: Option<String>,
    },
    /// Fetch SEP-6 transaction status from an anchor URL
    Status {
        /// Transaction ID to look up
        #[arg(long)] tx_id: String,
        /// Anchor base URL (e.g. https://anchor.example.com)
        #[arg(long)] anchor_url: String,
    },
    /// Revoke an attestor
    Revoke {
        #[arg(long)] address: String,
        #[arg(long)] contract_id: Option<String>,
        #[arg(long, default_value = "testnet")] network: String,
        #[arg(long)] secret_key: Option<String>,
        #[arg(long)] keypair_file: Option<String>,
        /// Name of a credential stored in the keystore (alternative to --secret-key)
        #[arg(long)] credential_name: Option<String>,
    },
    /// Manage stored credentials (encrypted secret keys)
    Credentials {
        #[command(subcommand)]
        action: CredentialsAction,
    },
    /// Check environment setup
    Doctor {
        /// Attempt to automatically fix issues
        #[arg(long)]
        fix: bool,
    },
    /// Query contract health, metadata freshness, and rate limiter status
    Health {
        /// Contract ID to query (or set ANCHOR_CONTRACT_ID)
        #[arg(long)]
        contract_id: String,
        #[arg(long, default_value = "testnet")]
        network: String,
        #[arg(long)]
        secret_key: Option<String>,
        #[arg(long)]
        keypair_file: Option<String>,
        /// Anchor address to check metadata freshness for (optional)
        #[arg(long)]
        anchor: Option<String>,
        /// Attestor address to check rate limiter health for (optional)
        #[arg(long)]
        attestor: Option<String>,
    },
    /// Manage custom network profiles
    Network {
        #[command(subcommand)]
        action: NetworkAction,
    },
}

#[derive(Subcommand)]
enum NetworkAction {
    /// Add a custom network profile
    Add {
        #[arg(long)] name: String,
        #[arg(long)] rpc_url: String,
        #[arg(long)] passphrase: String,
        #[arg(long)] horizon_url: Option<String>,
    },
    /// List all configured network profiles
    List,
    /// Remove a custom network profile
    Remove {
        #[arg(long)] name: String,
    },
    /// Set the default network
    SetDefault {
        #[arg(long)] name: String,
    },
}

#[derive(Subcommand)]
enum CredentialsAction {
    /// Store an encrypted credential
    Add {
        #[arg(long)] name: String,
        /// Secret key value (prompted if omitted)
        #[arg(long)] value: Option<String>,
    },
    /// Retrieve and print a stored credential
    Get {
        #[arg(long)] name: String,
    },
    /// List all stored credential names
    List,
    /// Remove a stored credential
    Remove {
        #[arg(long)] name: String,
    },
}

// ── Output types (JSON) ───────────────────────────────────────────────────────

#[derive(Serialize, serde::Deserialize)]
struct QuoteOutput {
    quote_id: u64,
    anchor: String,
    base_asset: String,
    quote_asset: String,
    rate: u64,
    fee_percentage: u32,
    minimum_amount: u64,
    maximum_amount: u64,
    valid_until: u64,
}

#[derive(Serialize)]
struct StatusOutput {
    transaction_id: String,
    kind: String,
    status: String,
    amount_in: Option<u64>,
    amount_out: Option<u64>,
    amount_fee: Option<u64>,
    message: Option<String>,
}

// ── Command implementations ───────────────────────────────────────────────────

// ── Deployments record ────────────────────────────────────────────────────────

#[derive(Serialize, serde::Deserialize, Clone)]
struct DeploymentRecord {
    contract_id: String,
    network: String,
    timestamp: u64,
    initialized: bool,
}

fn deployments_path() -> std::path::PathBuf {
    let dir = std::path::Path::new(".anchorkit");
    std::fs::create_dir_all(dir).ok();
    dir.join("deployments.json")
}

fn load_deployments() -> Vec<DeploymentRecord> {
    let path = deployments_path();
    if !path.exists() { return Vec::new(); }
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    serde_json::from_str(&content).unwrap_or_default()
}

fn save_deployments(records: &[DeploymentRecord]) {
    let path = deployments_path();
    let json = serde_json::to_string_pretty(records).unwrap_or_default();
    std::fs::write(path, json).ok();
}

// ── Pre-deployment validation ─────────────────────────────────────────────────

fn pre_deploy_validate(network: &str) -> bool {
    let mut ok = true;

    // 1. WASM artifact exists
    let wasm = "target/wasm32-unknown-unknown/release/anchorkit.wasm";
    if std::path::Path::new(wasm).exists() {
        println!("  ✓ WASM artifact found");
    } else {
        eprintln!("  ✗ WASM not found at {wasm} — run: cargo build --release --target wasm32-unknown-unknown --no-default-features --features wasm");
        ok = false;
    }

    // 2. Config files valid
    let config_check = check_config_files();
    if config_check.passed {
        println!("  ✓ Config files valid");
    } else {
        eprintln!("  ✗ {}", config_check.message);
        ok = false;
    }

    // 3. Network reachable
    let net_check = check_network_connectivity(network);
    if net_check.passed {
        println!("  ✓ Network reachable");
    } else {
        eprintln!("  ✗ {}", net_check.message);
        ok = false;
    }

    ok
}

/// Upgrade an existing contract to a freshly-built WASM.
///
/// Steps:
///   1. Build the WASM artifact.
///   2. Upload the WASM to the network and obtain its hash.
///   3. Call `upgrade(new_wasm_hash)` on the contract.
///   4. Call `migrate()` to apply any state-schema changes.
fn upgrade_contract(contract_id: &str, network: &str, source: &SecretKey) {
    println!("\n🔍 Pre-upgrade validation ({network})...");
    if !pre_deploy_validate(network) {
        eprintln!("\n❌ Pre-upgrade validation failed. Aborting.");
        std::process::exit(1);
    }
    println!("✅ Validation passed.\n");

    // Build WASM.
    println!("Building WASM...");
    let build = std::process::Command::new("cargo")
        .args([
            "build", "--release",
            "--target", "wasm32-unknown-unknown",
            "--no-default-features", "--features", "wasm",
        ])
        .status()
        .unwrap_or_else(|e| { eprintln!("error: failed to run cargo build: {e}"); std::process::exit(1); });
    if !build.success() {
        eprintln!("WASM build failed");
        std::process::exit(1);
    }

    let wasm = "target/wasm32-unknown-unknown/release/anchorkit.wasm";
    let net_url = rpc_url_for(network);
    let net_phrase = passphrase_for(network);

    // Upload WASM and capture the resulting hash.
    println!("Uploading WASM to {network}...");
    let source_str: &str = source; // coerce &SecretKey → &str for uniform array element type
    let upload_output = std::process::Command::new("stellar")
        .args([
            "contract", "upload",
            "--wasm", wasm,
            "--source", source_str,
            "--rpc-url", &net_url,
            "--network-passphrase", &net_phrase,
        ])
        .output()
        .unwrap_or_else(|e| { eprintln!("error: failed to run stellar contract upload — is the Stellar CLI installed? ({e})"); std::process::exit(1); });

    if !upload_output.status.success() {
        eprintln!("{}", String::from_utf8_lossy(&upload_output.stderr).trim());
        std::process::exit(1);
    }

    let new_wasm_hash = String::from_utf8_lossy(&upload_output.stdout).trim().to_string();
    println!("New WASM hash: {new_wasm_hash}");

    // Call upgrade() on the contract.
    println!("Calling upgrade() on contract {contract_id}...");
    stellar_invoke(contract_id, source, network, &[
        "upgrade",
        "--new_wasm_hash", &new_wasm_hash,
    ]);

    // Call migrate() to apply state-schema changes (idempotent).
    println!("Calling migrate() on contract {contract_id}...");
    stellar_invoke(contract_id, source, network, &["migrate"]);

    println!("✅ Contract upgraded successfully.");
    println!("   Contract ID : {contract_id}");
    println!("   New WASM    : {new_wasm_hash}");
}

fn deploy(network: &str, source: &str, admin: Option<&str>, dry_run: bool, list: bool) {
    // --list: print deployment history and exit
    if list {
        let records = load_deployments();
        if records.is_empty() {
            println!("No deployments recorded.");
        } else {
            println!("{}", serde_json::to_string_pretty(&records).unwrap_or_default());
        }
        return;
    }

    println!("\n🔍 Pre-deployment validation ({network})...");
    if !pre_deploy_validate(network) {
        eprintln!("\n❌ Pre-deployment validation failed. Aborting.");
        std::process::exit(1);
    }
    println!("✅ Validation passed.\n");

    if dry_run {
        println!("--dry-run: skipping actual deployment.");
        return;
    }

    // Build WASM
    println!("Building WASM...");
    let build = std::process::Command::new("cargo")
        .args(["build", "--release", "--target", "wasm32-unknown-unknown",
               "--no-default-features", "--features", "wasm"])
        .status()
        .unwrap_or_else(|e| { eprintln!("error: failed to run cargo build: {e}"); std::process::exit(1); });
    if !build.success() { eprintln!("WASM build failed"); std::process::exit(1); }

    let wasm = "target/wasm32-unknown-unknown/release/anchorkit.wasm";
    println!("Deploying {wasm} to {network}...");
    let net_url = rpc_url_for(network);
    let net_phrase = passphrase_for(network);
    let output = std::process::Command::new("stellar")
        .args(["contract", "deploy", "--wasm", wasm,
               // SECURITY: `source` is a Stellar secret key required by the
               // Stellar CLI.  It is passed only as a subprocess argument and
               // is never echoed to stdout or included in log messages.
               "--source", source,
               "--rpc-url", &net_url,
               "--network-passphrase", &net_phrase])
        .output()
        .unwrap_or_else(|e| { eprintln!("error: failed to run stellar contract deploy — is the Stellar CLI installed? ({e})"); std::process::exit(1); });

    if !output.status.success() {
        eprintln!("{}", String::from_utf8_lossy(&output.stderr).trim());
        std::process::exit(1);
    }

    let contract_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
    println!("Contract ID: {contract_id}");

    // Save to deployments.json
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
    let mut records = load_deployments();
    let mut record = DeploymentRecord {
        contract_id: contract_id.clone(),
        network: network.to_string(),
        timestamp,
        initialized: false,
    };

    // Post-deployment initialization.
    // `admin_addr` is a Stellar *public* address (G...) or the alias "default".
    // If the caller omitted --admin, we fall back to the source identifier
    // (which may be a key alias, not the raw secret).  We print only the
    // admin address, never the signing key.
    let admin_addr = admin.unwrap_or("default");
    println!("Initializing contract with admin {admin_addr}...");
    let init_result = std::process::Command::new("stellar")
        .args(["contract", "invoke",
               "--id", &contract_id,
               // SECURITY: `source` passed only as subprocess arg, not logged.
               "--source", source,
               "--rpc-url", &net_url,
               "--network-passphrase", &net_phrase,
               "--", "initialize",
               "--admin", admin_addr])
        .output();

    match init_result {
        Ok(out) if out.status.success() => {
            println!("✅ Contract initialized.");
            record.initialized = true;
        }
        Ok(out) => {
            eprintln!("⚠️  Post-deployment initialization failed:");
            eprintln!("{}", String::from_utf8_lossy(&out.stderr).trim());
            eprintln!("\nContract ID: {contract_id}");
            eprintln!("To initialize manually: stellar contract invoke --id {contract_id} --source <SIGNING_KEY_OR_ALIAS> -- initialize --admin <ADMIN_ADDRESS>");
        }
        Err(e) => {
            eprintln!("⚠️  Could not run initialization: {e}");
            eprintln!("Contract ID: {contract_id}");
        }
    }

    records.push(record);
    save_deployments(&records);
    println!("Deployment saved to .anchorkit/deployments.json");
}

fn parse_services(services: &[String]) -> Vec<u32> {
    services.iter().map(|s| match s.trim() {
        "deposits"    => 1,
        "withdrawals" => 2,
        "quotes"      => 3,
        "kyc"         => 4,
        other => { eprintln!("Unknown service: {other}"); std::process::exit(1); }
    }).collect()
}

fn register(
    address: &str, services: &[String], contract_id: &str,
    network: &str, source: &SecretKey, sep10_token: &str, sep10_issuer: &str,
) {
    let address = normalize_stellar_public_address("attestor address", address);
    let sep10_issuer = normalize_stellar_public_address("SEP-10 issuer address", sep10_issuer);
    let service_ids = parse_services(services)
        .iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",");

    // SECURITY: sep10_token is a bearer token.  It is passed only as a
    // subprocess argument to the Stellar CLI and is never echoed to stdout.
    stellar_invoke(contract_id, source, network, &[
        "register_attestor",
        "--attestor", &address,
        "--sep10_token", sep10_token,
        "--sep10_issuer", &sep10_issuer,
        "--public_key", "0000000000000000000000000000000000000000000000000000000000000000",
    ]);
    stellar_invoke(contract_id, source, network, &[
        "configure_services",
        "--anchor", &address,
        "--services", &service_ids,
    ]);
    println!("Attestor {address} registered and services configured.");
}

fn attest(
    subject: &str, payload_hash: &str, contract_id: &str,
    network: &str, source: &SecretKey, issuer: &str, session_id: Option<u64>,
) {
    let subject = normalize_stellar_public_address("subject address", subject);
    let issuer = normalize_stellar_public_address("issuer address", issuer);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs().to_string();

    // NOTE: `--signature` should be a real Ed25519 signature over the payload.
    // The placeholder below reuses `payload_hash` only for local/test use.
    // Production callers must supply a proper signature via a dedicated flag.
    let session_str;
    let result = if let Some(sid) = session_id {
        session_str = sid.to_string();
        stellar_invoke(contract_id, source, network, &[
            "submit_attestation_with_session",
            "--session_id", &session_str,
            "--issuer", &issuer, "--subject", &subject,
            "--timestamp", &timestamp,
            "--payload_hash", payload_hash,
            "--signature", payload_hash,  // placeholder — replace with real sig
        ])
    } else {
        stellar_invoke(contract_id, source, network, &[
            "submit_attestation",
            "--issuer", &issuer, "--subject", &subject,
            "--timestamp", &timestamp,
            "--payload_hash", payload_hash,
            "--signature", payload_hash,  // placeholder — replace with real sig
        ])
    };
    println!("Attestation ID: {result}");
}

fn quote(from: &str, to: &str, amount: u64, contract_id: &str, network: &str, source: &SecretKey) {
    let amount_str = amount.to_string();
    // route_transaction takes a RoutingOptions XDR; pass individual fields via stellar CLI args
    let raw = stellar_invoke(contract_id, source, network, &[
        "route_transaction",
        "--base_asset", from,
        "--quote_asset", to,
        "--amount", &amount_str,
        "--operation_type", "1",   // 1 = deposit
        "--strategy", "lowest_fee",
        "--min_reputation", "0",
        "--max_anchors", "10",
        "--require_kyc", "false",
    ]);

    // The stellar CLI returns XDR or JSON; parse as JSON first, fall back to raw print
    let out: QuoteOutput = serde_json::from_str(&raw).unwrap_or_else(|_| {
        // stellar CLI may return a plain contract-encoded value; surface it as-is
        eprintln!("note: could not parse quote as JSON, raw output:\n{raw}");
        std::process::exit(1);
    });
    match serde_json::to_string_pretty(&out) {
        Ok(s) => println!("{s}"),
        Err(e) => { eprintln!("error: failed to serialize quote output: {e}"); std::process::exit(1); }
    }
}

fn status(tx_id: &str, anchor_url: &str) {
    let url = format!("{}/sep6/transaction?id={}", anchor_url.trim_end_matches('/'), tx_id);
    let resp = reqwest::blocking::get(&url)
        .unwrap_or_else(|e| { eprintln!("error: request failed: {e}"); std::process::exit(1); });

    if !resp.status().is_success() {
        eprintln!("error: anchor returned HTTP {}", resp.status());
        std::process::exit(1);
    }

    let body: serde_json::Value = resp.json()
        .unwrap_or_else(|e| { eprintln!("error: invalid JSON from anchor: {e}"); std::process::exit(1); });

    // SEP-6 wraps the transaction under a "transaction" key
    let tx = body.get("transaction").unwrap_or(&body);

    let kind = tx.get("kind").and_then(|v| v.as_str()).unwrap_or("deposit").to_string();
    let out = StatusOutput {
        transaction_id: tx.get("id").and_then(|v| v.as_str()).unwrap_or(tx_id).to_string(),
        kind,
        status: tx.get("status").and_then(|v| v.as_str()).unwrap_or("unknown").to_string(),
        amount_in:  tx.get("amount_in").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()),
        amount_out: tx.get("amount_out").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()),
        amount_fee: tx.get("amount_fee").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()),
        message:    tx.get("message").and_then(|v| v.as_str()).map(|s| s.to_string()),
    };
    match serde_json::to_string_pretty(&out) {
        Ok(s) => println!("{s}"),
        Err(e) => { eprintln!("error: failed to serialize status output: {e}"); std::process::exit(1); }
    }
}

fn revoke(address: &str, contract_id: &str, network: &str, source: &SecretKey) {
    stellar_invoke(contract_id, source, network, &[
        "revoke_attestor",
        "--attestor", &address,
    ]);
    println!("{{\"revoked\": true, \"address\": \"{address}\"}}");
}

// ── Doctor command ────────────────────────────────────────────────────────────

struct CheckResult {
    passed: bool,
    warning: bool,
    message: String,
}

impl CheckResult {
    fn pass(msg: impl Into<String>) -> Self {
        Self { passed: true, warning: false, message: msg.into() }
    }
    fn fail(msg: impl Into<String>) -> Self {
        Self { passed: false, warning: false, message: msg.into() }
    }
    fn warn(msg: impl Into<String>) -> Self {
        Self { passed: true, warning: true, message: msg.into() }
    }
    fn icon(&self) -> &str {
        if !self.passed { "✗" } else if self.warning { "⚠" } else { "✓" }
    }
    fn color(&self) -> &str {
        if !self.passed { "\x1b[31m" } else if self.warning { "\x1b[33m" } else { "\x1b[32m" }
    }
}

fn check_stellar_cli() -> CheckResult {
    match std::process::Command::new("stellar").arg("--version").output() {
        Ok(output) => {
            let version_str = String::from_utf8_lossy(&output.stdout);
            if let Some(version_line) = version_str.lines().next() {
                // Parse version like "stellar 21.0.0"
                if let Some(ver) = version_line.split_whitespace().nth(1) {
                    if let Some(major) = ver.split('.').next().and_then(|s| s.parse::<u32>().ok()) {
                        if major >= 21 {
                            return CheckResult::pass(format!("Stellar CLI {} installed", ver));
                        } else {
                            return CheckResult::fail(format!("Stellar CLI {} found, but v21+ required", ver));
                        }
                    }
                }
            }
            CheckResult::warn("Stellar CLI installed but version could not be parsed")
        }
        Err(_) => CheckResult::fail("Stellar CLI not found in PATH"),
    }
}

fn check_wasm_target(fix: bool) -> CheckResult {
    let output = std::process::Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output();
    
    match output {
        Ok(out) => {
            let targets = String::from_utf8_lossy(&out.stdout);
            if targets.contains("wasm32-unknown-unknown") {
                CheckResult::pass("wasm32-unknown-unknown target installed")
            } else if fix {
                println!("  Attempting to install wasm32-unknown-unknown...");
                let install = std::process::Command::new("rustup")
                    .args(["target", "add", "wasm32-unknown-unknown"])
                    .status();
                if install.map(|s| s.success()).unwrap_or(false) {
                    CheckResult::pass("wasm32-unknown-unknown target installed (auto-fixed)")
                } else {
                    CheckResult::fail("wasm32-unknown-unknown target missing and auto-fix failed")
                }
            } else {
                CheckResult::fail("wasm32-unknown-unknown target not installed (run: rustup target add wasm32-unknown-unknown)")
            }
        }
        Err(_) => CheckResult::fail("rustup not found"),
    }
}

fn check_contract_id_env() -> CheckResult {
    match std::env::var("ANCHOR_CONTRACT_ID") {
        Ok(id) if !id.is_empty() => CheckResult::pass(format!("ANCHOR_CONTRACT_ID set: {}", &id[..id.len().min(16)])),
        _ => CheckResult::warn("ANCHOR_CONTRACT_ID not set (required for contract operations)"),
    }
}

fn check_admin_secret_env() -> CheckResult {
    match std::env::var("ANCHOR_ADMIN_SECRET") {
        Ok(secret) if !secret.is_empty() && secret.starts_with('S') => {
            // Confirm presence and basic format only — never log the value.
            CheckResult::pass("ANCHOR_ADMIN_SECRET set and appears valid (starts with 'S')")
        }
        Ok(secret) if !secret.is_empty() => {
            // Value present but does not look like a Stellar secret key.
            // Do NOT include the value or any prefix in the message.
            CheckResult::fail("ANCHOR_ADMIN_SECRET is set but does not appear to be a valid Stellar secret key (expected 'S...' format)")
        }
        Ok(_) => CheckResult::warn("ANCHOR_ADMIN_SECRET is set but empty"),
        Err(_) => CheckResult::warn("ANCHOR_ADMIN_SECRET not set (required for signing operations)"),
    }
}

fn check_network_connectivity(network: &str) -> CheckResult {
    let url = rpc_url_for(network);
    check_network_connectivity_url(&url)
}

fn check_contract_deployment(contract_id: &str, network: &str) -> CheckResult {
    // Use the SecretKey wrapper so the value is never accidentally logged.
    // Fall back to the "default" alias (a named key in the Stellar CLI keystore)
    // rather than embedding a raw secret in the subprocess arguments.
    let source = std::env::var("ANCHOR_ADMIN_SECRET")
        .ok()
        .filter(|s| !s.is_empty())
        .map(SecretKey::new)
        .unwrap_or_else(|| SecretKey::new("default"));

    let source_str: &str = &*source; // coerce SecretKey → &str for uniform array element type
    let output = std::process::Command::new("stellar")
        .args(["contract", "invoke",
               "--id", contract_id,
               "--source", source_str,
               "--rpc-url", &rpc_url_for(network),
               "--network-passphrase", &passphrase_for(network),
               "--",
               "get_attestor_count"])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            CheckResult::pass(format!("Contract {} is deployed and responding", &contract_id[..contract_id.len().min(16)]))
        }
        Ok(_) => CheckResult::fail("Contract exists but failed to respond (may not be initialized)"),
        Err(_) => CheckResult::fail("Failed to query contract"),
    }
}

fn check_config_files() -> CheckResult {
    let config_dir = std::path::Path::new("configs");
    if !config_dir.exists() {
        return CheckResult::warn("configs/ directory not found");
    }
    
    let mut valid_count = 0;
    let mut total_count = 0;
    
    if let Ok(entries) = std::fs::read_dir(config_dir) {
        for entry in entries.flatten() {
            if let Some(ext) = entry.path().extension() {
                if ext == "json" || ext == "toml" {
                    total_count += 1;
                    if ext == "json" {
                        if let Ok(content) = std::fs::read_to_string(entry.path()) {
                            if serde_json::from_str::<serde_json::Value>(&content).is_ok() {
                                valid_count += 1;
                            }
                        }
                    } else {
                        valid_count += 1; // Basic check for TOML
                    }
                }
            }
        }
    }
    
    if total_count == 0 {
        CheckResult::warn("No config files found in configs/")
    } else if valid_count == total_count {
        CheckResult::pass(format!("{} config file(s) validated", total_count))
    } else {
        CheckResult::fail(format!("{}/{} config files are valid", valid_count, total_count))
    }
}

fn doctor(network: &str, fix: bool) {
    println!("\n🔍 SorobanAnchor Environment Check\n");
    
    let checks = vec![
        ("Stellar CLI", check_stellar_cli()),
        ("WASM Target", check_wasm_target(fix)),
        ("Contract ID", check_contract_id_env()),
        ("Admin Secret", check_admin_secret_env()),
        ("Network", check_network_connectivity(network)),
    ];
    
    let mut all_passed = true;
    
    for (name, result) in &checks {
        println!("  {} {} {}", result.color(), result.icon(), name);
        println!("    {}\x1b[0m", result.message);
        if !result.passed {
            all_passed = false;
        }
    }
    
    // Optional checks that require contract ID
    if let Ok(contract_id) = std::env::var("ANCHOR_CONTRACT_ID") {
        if !contract_id.is_empty() {
            let deployment_check = check_contract_deployment(&contract_id, network);
            println!("  {} {} Contract Deployment", deployment_check.color(), deployment_check.icon());
            println!("    {}\x1b[0m", deployment_check.message);
            if !deployment_check.passed {
                all_passed = false;
            }
        }
    }
    
    let config_check = check_config_files();
    println!("  {} {} Config Files", config_check.color(), config_check.icon());
    println!("    {}\x1b[0m", config_check.message);
    if !config_check.passed {
        all_passed = false;
    }
    
    println!();
    if all_passed {
        println!("✅ All checks passed! Your environment is ready.\n");
        std::process::exit(0);
    } else {
        println!("❌ Some checks failed. Please address the issues above.\n");
        if !fix {
            println!("Tip: Run with --fix to automatically resolve fixable issues.\n");
        }
        std::process::exit(1);
    }
}

// ── Health check command (#268) ───────────────────────────────────────────────

fn health_check(contract_id: &str, network: &str, source: &SecretKey, anchor: Option<&str>, attestor: Option<&str>) {
    println!("\n🏥 AnchorKit Health Check\n");

    // 1. Overall service health
    let status_raw = stellar_invoke(contract_id, source, network, &["get_health_status"]);
    let status_label = match status_raw.trim().trim_matches('"') {
        "0" | "Healthy"     => "\x1b[32m✓ Healthy\x1b[0m",
        "1" | "Degraded"    => "\x1b[33m⚠ Degraded\x1b[0m",
        _                   => "\x1b[31m✗ Unavailable\x1b[0m",
    };
    println!("  Service Status : {status_label}");

    // 2. Metadata freshness (optional — only when --anchor is supplied)
    if let Some(anchor_addr) = anchor {
        let freshness_raw = stellar_invoke(contract_id, source, network, &[
            "get_metadata_freshness",
            "--anchor", anchor_addr,
        ]);
        // Parse the returned struct fields from JSON-like output
        let state_label = if freshness_raw.contains("\"Fresh\"") || freshness_raw.contains("\"state\":0") {
            "\x1b[32mFresh\x1b[0m"
        } else if freshness_raw.contains("\"Stale\"") || freshness_raw.contains("\"state\":2") {
            "\x1b[33mStale — refresh recommended\x1b[0m"
        } else if freshness_raw.contains("\"Expired\"") || freshness_raw.contains("\"state\":3") {
            "\x1b[31mExpired — must refresh\x1b[0m"
        } else {
            "\x1b[31mMissing — no cache entry\x1b[0m"
        };
        println!("  Metadata Cache : {state_label}");
        println!("  Anchor         : {anchor_addr}");
    }

    // 3. Rate limiter health (optional — only when --attestor is supplied)
    if let Some(attestor_addr) = attestor {
        let rl_raw = stellar_invoke(contract_id, source, network, &[
            "get_rate_limiter_health",
            "--attestor", attestor_addr,
        ]);
        let throttled = rl_raw.contains("\"is_throttled\":true") || rl_raw.contains("is_throttled: true");
        let rl_label = if throttled {
            "\x1b[31m✗ Throttled\x1b[0m"
        } else {
            "\x1b[32m✓ OK\x1b[0m"
        };
        println!("  Rate Limiter   : {rl_label}");
        println!("  Attestor       : {attestor_addr}");
        if throttled {
            eprintln!("\n  ⚠  Attestor has reached the submission limit for the current window.");
        }
    }

    println!();
}

// ── Network command ───────────────────────────────────────────────────────────

fn network_cmd(action: NetworkAction) {
    match action {
        NetworkAction::Add { name, rpc_url, passphrase, horizon_url } => {
            // Validate RPC URL connectivity before saving
            let check = check_network_connectivity_url(&rpc_url);
            if !check.passed {
                eprintln!("error: RPC URL validation failed: {}", check.message);
                std::process::exit(1);
            }
            let mut profiles = load_network_profiles();
            if find_profile(&profiles, &name).is_some() {
                eprintln!("error: network '{}' already exists. Remove it first.", name);
                std::process::exit(1);
            }
            profiles.push(NetworkProfile {
                name: name.clone(),
                rpc_url,
                network_passphrase: passphrase,
                horizon_url,
                is_default: false,
            });
            save_network_profiles(&profiles);
            println!("Network '{}' added.", name);
        }
        NetworkAction::List => {
            let profiles = load_network_profiles();
            // Always show built-ins
            let builtins = [
                ("testnet",   "https://soroban-testnet.stellar.org",  "Test SDF Network ; September 2015"),
                ("mainnet",   "https://horizon.stellar.org",           "Public Global Stellar Network ; September 2015"),
                ("futurenet", "https://rpc-futurenet.stellar.org",     "Test SDF Future Network ; October 2022"),
            ];
            println!("{:<16} {:<45} {}", "NAME", "RPC URL", "PASSPHRASE");
            for (name, url, phrase) in &builtins {
                println!("{:<16} {:<45} {} (built-in)", name, url, phrase);
            }
            for p in &profiles {
                let default_marker = if p.is_default { " (default)" } else { "" };
                println!("{:<16} {:<45} {}{}", p.name, p.rpc_url, p.network_passphrase, default_marker);
            }
        }
        NetworkAction::Remove { name } => {
            let mut profiles = load_network_profiles();
            let before = profiles.len();
            profiles.retain(|p| p.name != name);
            if profiles.len() == before {
                eprintln!("error: network '{}' not found.", name);
                std::process::exit(1);
            }
            save_network_profiles(&profiles);
            println!("Network '{}' removed.", name);
        }
        NetworkAction::SetDefault { name } => {
            let mut profiles = load_network_profiles();
            // Allow setting built-in names as default (stored as a marker profile)
            let found = profiles.iter().any(|p| p.name == name);
            if !found {
                // Check if it's a built-in
                let builtins = ["testnet", "mainnet", "futurenet"];
                if !builtins.contains(&name.as_str()) {
                    eprintln!("error: network '{}' not found.", name);
                    std::process::exit(1);
                }
            }
            for p in &mut profiles {
                p.is_default = p.name == name;
            }
            save_network_profiles(&profiles);
            println!("Default network set to '{}'.", name);
        }
    }
}

fn check_network_connectivity_url(url: &str) -> CheckResult {
    match reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .and_then(|client| client.get(url).send())
    {
        Ok(resp) if resp.status().is_success() || resp.status().as_u16() == 404 => {
            CheckResult::pass(format!("RPC URL {} reachable", url))
        }
        Ok(resp) => CheckResult::warn(format!("RPC URL {} responded with HTTP {}", url, resp.status())),
        Err(e) => CheckResult::fail(format!("Cannot connect to {}: {}", url, e)),
    }
}

// ── Keystore (AES-256-GCM encrypted credential store) ─────────────────────────

use aes_gcm::{Aes256Gcm, KeyInit, aead::Aead};
use aes_gcm::aead::rand_core::RngCore;
use argon2::{Argon2, PasswordHasher, password_hash::SaltString};

fn keystore_path() -> std::path::PathBuf {
    let dir = dirs_home().join(".anchorkit");
    std::fs::create_dir_all(&dir).ok();
    dir.join("credentials.json")
}

fn keystore_load() -> std::collections::HashMap<String, String> {
    let path = keystore_path();
    if !path.exists() { return std::collections::HashMap::new(); }
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    serde_json::from_str(&content).unwrap_or_default()
}

fn keystore_save(store: &std::collections::HashMap<String, String>) {
    let path = keystore_path();
    let json = serde_json::to_string_pretty(store).unwrap_or_default();
    std::fs::write(path, json).ok();
}

/// Derive a 32-byte key from password using Argon2id with a fixed salt derived from the name.
fn derive_key(password: &str, name: &str) -> [u8; 32] {
    let salt_raw = format!("anchorkit-{name}");
    let salt_padded = format!("{:>22}", &salt_raw[..salt_raw.len().min(22)]);
    let salt = SaltString::from_b64(&base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        salt_padded.as_bytes(),
    )).unwrap_or_else(|_| SaltString::generate(&mut rand::thread_rng()));
    let argon2 = Argon2::default();
    let hash = argon2.hash_password(password.as_bytes(), &salt)
        .unwrap_or_else(|e| { eprintln!("error: key derivation failed: {e}"); std::process::exit(1); });
    let hash_bytes = hash.hash.unwrap();
    let mut key = [0u8; 32];
    let bytes = hash_bytes.as_bytes();
    key[..bytes.len().min(32)].copy_from_slice(&bytes[..bytes.len().min(32)]);
    key
}

fn keystore_encrypt(password: &str, name: &str, plaintext: &str) -> String {
    use aes_gcm::aead::generic_array::GenericArray;
    let key_bytes = derive_key(password, name);
    let cipher = Aes256Gcm::new(GenericArray::from_slice(&key_bytes));
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = aes_gcm::Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher.encrypt(nonce, plaintext.as_bytes())
        .unwrap_or_else(|e| { eprintln!("error: encryption failed: {e}"); std::process::exit(1); });
    // Store as base64(nonce) + "." + base64(ciphertext)
    use base64::Engine;
    format!(
        "{}.{}",
        base64::engine::general_purpose::STANDARD.encode(nonce_bytes),
        base64::engine::general_purpose::STANDARD.encode(ciphertext),
    )
}

fn keystore_decrypt(password: &str, name: &str, stored: &str) -> Result<String, String> {
    use aes_gcm::aead::generic_array::GenericArray;
    use base64::Engine;
    let parts: Vec<&str> = stored.splitn(2, '.').collect();
    if parts.len() != 2 {
        return Err("invalid stored credential format".to_string());
    }
    let nonce_bytes = base64::engine::general_purpose::STANDARD.decode(parts[0])
        .map_err(|e| format!("base64 decode nonce: {e}"))?;
    let ciphertext = base64::engine::general_purpose::STANDARD.decode(parts[1])
        .map_err(|e| format!("base64 decode ciphertext: {e}"))?;
    let key_bytes = derive_key(password, name);
    let cipher = Aes256Gcm::new(GenericArray::from_slice(&key_bytes));
    let nonce = aes_gcm::Nonce::from_slice(&nonce_bytes);
    let plaintext = cipher.decrypt(nonce, ciphertext.as_ref())
        .map_err(|_| "decryption failed — wrong password?".to_string())?;
    String::from_utf8(plaintext).map_err(|e| format!("utf8: {e}"))
}

fn keystore_get_decrypted(name: &str, password: &str) -> SecretKey {
    let store = keystore_load();
    let stored = store.get(name)
        .unwrap_or_else(|| { eprintln!("error: credential '{}' not found", name); std::process::exit(1); });
    let plaintext = keystore_decrypt(password, name, stored)
        .unwrap_or_else(|e| { eprintln!("error: failed to decrypt credential: {e}"); std::process::exit(1); });
    SecretKey::new(plaintext)
}

fn credentials_add(name: &str, value: Option<&str>, no_interactive: bool) {
    if no_interactive {
        eprintln!("error: 'credentials add' requires interactive password prompts; \
                   not supported with --no-interactive / ANCHORKIT_NO_INTERACTIVE");
        std::process::exit(1);
    }
    let secret = match value {
        Some(v) => v.to_string(),
        None => rpassword::prompt_password("Secret key value: ")
            .unwrap_or_else(|e| { eprintln!("error: {e}"); std::process::exit(1); }),
    };
    let password = rpassword::prompt_password("Keystore password: ")
        .unwrap_or_else(|e| { eprintln!("error: {e}"); std::process::exit(1); });
    let confirm = rpassword::prompt_password("Confirm password: ")
        .unwrap_or_else(|e| { eprintln!("error: {e}"); std::process::exit(1); });
    if password != confirm {
        eprintln!("error: passwords do not match");
        std::process::exit(1);
    }
    let encrypted = keystore_encrypt(&password, name, &secret);
    let mut store = keystore_load();
    store.insert(name.to_string(), encrypted);
    keystore_save(&store);
    println!("Credential '{}' stored.", name);
}

fn credentials_get(name: &str, no_interactive: bool) {
    if no_interactive {
        eprintln!("error: 'credentials get' requires an interactive password prompt; \
                   not supported with --no-interactive / ANCHORKIT_NO_INTERACTIVE");
        std::process::exit(1);
    }
    let password = rpassword::prompt_password("Keystore password: ")
        .unwrap_or_else(|e| { eprintln!("error: {e}"); std::process::exit(1); });
    let secret = keystore_get_decrypted(name, &password);
    println!("{}", secret.expose());
}

fn credentials_list() {
    let store = keystore_load();
    if store.is_empty() {
        println!("No credentials stored.");
    } else {
        for name in store.keys() {
            println!("{name}");
        }
    }
}

fn credentials_remove(name: &str) {
    let mut store = keystore_load();
    if store.remove(name).is_none() {
        eprintln!("error: credential '{}' not found", name);
        std::process::exit(1);
    }
    keystore_save(&store);
    println!("Credential '{}' removed.", name);
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();
    let global_contract_id = cli.contract_id.clone();
    let network = cli.network.unwrap_or_else(|| {
        let n = default_network();
        if std::env::var("STELLAR_NETWORK").is_err() && !load_network_profiles().iter().any(|p| p.is_default) {
            eprintln!("note: STELLAR_NETWORK not set — using '{n}' (set STELLAR_NETWORK or: anchorkit network set-default --name <NAME>)");
        }
        n
    });
    match cli.command {
        Commands::Deploy { network: cmd_net, source, admin, dry_run, list, upgrade, secret_key, keypair_file } => {
            let net = cmd_net;
            if upgrade {
                let contract_id = require_contract_id(global_contract_id, None, "deploy --upgrade");
                let signing_source = resolve_source(secret_key.as_deref(), keypair_file.as_deref(), None);
                upgrade_contract(&contract_id, &net, &signing_source);
            } else {
                deploy(&net, &source, admin.as_deref(), dry_run, list);
            }
        }
        Commands::Register { address, services, contract_id, network: cmd_net, secret_key, keypair_file, credential_name, sep10_token, sep10_issuer } => {
            let cid = require_contract_id(global_contract_id, contract_id, "register");
            let net = cmd_net;
            let source = resolve_source(secret_key.as_deref(), keypair_file.as_deref(), credential_name.as_deref());
            register(&address, &services, &cid, &net, &source, &sep10_token, &sep10_issuer);
        }
        Commands::Attest { subject, payload_hash, contract_id, network: cmd_net, secret_key, keypair_file, credential_name, issuer, session_id } => {
            let cid = require_contract_id(global_contract_id, contract_id, "attest");
            let source = resolve_source(secret_key.as_deref(), keypair_file.as_deref(), credential_name.as_deref());
            attest(&subject, &payload_hash, &cid, &cmd_net, &source, &issuer, session_id);
        }
        Commands::Quote { from, to, amount, contract_id, network: cmd_net, secret_key, keypair_file, credential_name } => {
            let cid = require_contract_id(global_contract_id, contract_id, "quote");
            let source = resolve_source(secret_key.as_deref(), keypair_file.as_deref(), credential_name.as_deref());
            quote(&from, &to, amount, &cid, &cmd_net, &source);
        }
        Commands::Status { tx_id, anchor_url } => {
            status(&tx_id, &anchor_url);
        }
        Commands::Revoke { address, contract_id, network: cmd_net, secret_key, keypair_file, credential_name } => {
            let cid = require_contract_id(global_contract_id, contract_id, "revoke");
            let source = resolve_source(secret_key.as_deref(), keypair_file.as_deref(), credential_name.as_deref());
            revoke(&address, &cid, &cmd_net, &source);
        }
        Commands::Doctor { fix } => {
            doctor(&network, fix);
        }
        Commands::Health { contract_id, network: cmd_net, secret_key, keypair_file, anchor, attestor } => {
            let source = resolve_source(secret_key.as_deref(), keypair_file.as_deref(), None);
            health_check(&contract_id, &cmd_net, &source, anchor.as_deref(), attestor.as_deref());
        }
        Commands::Network { action } => {
            network_cmd(action);
        }
        Commands::Credentials { action } => {
            match action {
                CredentialsAction::Add { name, value } => {
                    credentials_add(&name, value.as_deref(), no_interactive);
                }
                CredentialsAction::Get { name } => {
                    credentials_get(&name, no_interactive);
                }
                CredentialsAction::List => {
                    credentials_list();
                }
                CredentialsAction::Remove { name } => {
                    credentials_remove(&name);
                }
            }
        }
    }
}
