//! Runtime configuration loading and shape validation.

#[cfg(feature = "std")]
extern crate std;

extern crate alloc;

use alloc::{
    collections::BTreeMap,
    format,
    string::{String, ToString},
    vec::Vec,
};

use serde::Deserialize;
use serde_json::Value;

#[cfg(feature = "std")]
use std::{fs, io::{self, ErrorKind, Read}, path::Path};

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfig {
    pub contract: ContractConfig,
    pub attestors: AttestorsConfig,
    pub sessions: Option<SessionsConfig>,
    pub operations: Option<OperationsConfig>,
    pub remittance: Option<RemittanceConfig>,
    pub stablecoin: Option<StablecoinConfig>,
    pub compliance: Option<Value>,
    pub storage: Option<StorageConfig>,
    pub security: Option<SecurityConfig>,
    pub monitoring: Option<MonitoringConfig>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ContractConfig {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub network: String,
    pub admin_address: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AttestorsConfig {
    pub registry: Vec<AttestorConfig>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AttestorConfig {
    pub name: String,
    pub address: String,
    pub description: Option<String>,
    pub endpoint: Option<String>,
    pub contact_email: Option<String>,
    pub role: String,
    pub enabled: bool,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SessionsConfig {
    pub enable_session_tracking: Option<bool>,
    pub session_timeout_seconds: Option<u64>,
    pub operations_per_session: Option<u64>,
    pub audit_log_retention_days: Option<u64>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct OperationsConfig {
    pub templates: Option<Vec<OperationTemplateConfig>>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct OperationTemplateConfig {
    pub id: String,
    pub name: String,
    pub attestor: String,
    pub operation_type: String,
    pub required_fields: Vec<String>,
    pub replay_protection: String,
    pub description: Option<String>,
    pub payload_schema: Option<Value>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RemittanceConfig {
    pub corridors: Option<Vec<RemittanceCorridorConfig>>,
    pub exchange_rate: Option<ExchangeRateConfig>,
    pub fee_structure: Option<Vec<FeeStructureConfig>>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RemittanceCorridorConfig {
    pub source: String,
    pub destination: String,
    pub local_currency: String,
    pub settlement_method: String,
    pub expected_settlement_hours: Option<u64>,
    pub minimum_amount: Option<f64>,
    pub maximum_amount: Option<f64>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ExchangeRateConfig {
    pub enable_live_rates: Option<bool>,
    pub rate_lock_duration_seconds: Option<u64>,
    pub rate_variance_tolerance_percent: Option<f64>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FeeStructureConfig {
    pub corridor: String,
    pub fee_type: String,
    pub fee_value: f64,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StablecoinConfig {
    pub name: String,
    pub symbol: String,
    pub decimals: u64,
    pub reserve_currency: String,
    pub reserve_composition: Option<Vec<ReserveCompositionConfig>>,
    pub supply_caps: Option<SupplyCapsConfig>,
    pub collateral_types: Option<Vec<CollateralTypeConfig>>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReserveCompositionConfig {
    pub asset: String,
    pub target_percentage: f64,
    pub minimum_percentage: f64,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SupplyCapsConfig {
    pub maximum_supply_cap: Option<u64>,
    pub warning_threshold_percent: Option<f64>,
    pub emergency_threshold_percent: Option<f64>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CollateralTypeConfig {
    pub name: String,
    pub symbol: String,
    pub liquidation_ratio: f64,
    pub liquidation_fee_percent: Option<f64>,
    pub price_feed: Option<String>,
    pub minimum_deposit: Option<f64>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StorageConfig {
    pub instance_ttl_days: Option<u64>,
    pub session_cache_enabled: Option<bool>,
    pub persistent_ttl_days: Option<u64>,
    pub audit_log_enabled: Option<bool>,
    pub audit_log_compression: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SecurityConfig {
    pub require_signature_verification: Option<bool>,
    pub signature_algorithm: Option<String>,
    pub signature_expiry_seconds: Option<u64>,
    pub nonce_required: Option<bool>,
    pub nonce_reuse_prevention: Option<bool>,
    pub endpoint_pins: Option<Vec<EndpointPinConfig>>,
    pub rate_limits: Option<Vec<RateLimitConfig>>,
    pub multisig_requirements: Option<Vec<MultisigRequirementConfig>>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EndpointPinConfig {
    pub endpoint: String,
    pub pin_sha256: String,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RateLimitConfig {
    pub attestor: String,
    pub requests_per_minute: u64,
    pub requests_per_hour: u64,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MultisigRequirementConfig {
    pub operation: String,
    pub required_signatures: u64,
    pub signatory_attestors: Vec<String>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MonitoringConfig {
    pub enable_metrics: Option<bool>,
    pub log_all_operations: Option<bool>,
    pub alert_on_failed_attestations: Option<bool>,
    pub alert_on_replay_attempts: Option<bool>,
    pub metrics_namespace: Option<String>,
    pub alerts: Option<Vec<AlertConfig>>,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct AlertConfig {
    pub condition: String,
    pub severity: String,
    pub recipients: Vec<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

fn secure_read_config_file(path: &Path) -> Result<String, std::io::Error> {
    // Ensure the file exists
    if !path.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("file does not exist: {}", path.display()),
        ));
    }
    // Reject symlinks to avoid symlink attacks
    if let Ok(metadata) = path.metadata() {
        if metadata.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("symlink file is not allowed: {}", path.display()),
            ));
        }
    }
    // Ensure it's a regular file
    if let Ok(metadata) = path.metadata() {
        if !metadata.file_type().is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("not a regular file: {}", path.display()),
            ));
        }
    }
    // Open for reading (checks readability)
    let mut file = std::fs::File::open(path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    Ok(contents)
}

pub fn parse_runtime_config_str(input: &str, format: ConfigFormat) -> Result<RuntimeConfig, String> {
    let config = match format {
        ConfigFormat::Json => serde_json::from_str(input).map_err(|err| err.to_string())?,
        ConfigFormat::Toml => toml::from_str(input).map_err(|err| err.to_string())?,
    };
    validate_runtime_config(&config)?;
    Ok(config)
}

#[cfg(feature = "std")]
pub fn load_runtime_config_file(path: impl AsRef<Path>) -> Result<RuntimeConfig, String> {
    let path = path.as_ref();
    let input = fs::read_to_string(path).map_err(|err| err.to_string())?;
    let format = ConfigFormat::from_path(path)?;
    parse_runtime_config_str(&input, format)
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ConfigFormat {
    Json,
    Toml,
}

impl ConfigFormat {
    #[cfg(feature = "std")]
    fn from_path(path: &Path) -> Result<Self, String> {
        match path.extension().and_then(|ext| ext.to_str()) {
            Some("json") => Ok(Self::Json),
            Some("toml") => Ok(Self::Toml),
            Some(ext) => Err(format!("unsupported config extension: {ext}")),
            None => Err("config path has no extension".to_string()),
        }
    }
}

fn validate_runtime_config(config: &RuntimeConfig) -> Result<(), String> {
    if config.contract.name.is_empty() {
        return Err("contract.name cannot be empty".to_string());
    }

    if config.attestors.registry.is_empty() {
        return Err("attestors.registry cannot be empty".to_string());
    }

    let attestors: Vec<&str> = config
        .attestors
        .registry
        .iter()
        .map(|attestor| attestor.name.as_str())
        .collect();

    if let Some(operations) = &config.operations {
        if let Some(templates) = &operations.templates {
            for template in templates {
                if !attestors.contains(&template.attestor.as_str()) {
                    return Err(format!(
                        "operation '{}' references unknown attestor '{}'",
                        template.id, template.attestor
                    ));
                }
            }
        }
    }

    if let Some(security) = &config.security {
        if let Some(rate_limits) = &security.rate_limits {
            for rate_limit in rate_limits {
                if !attestors.contains(&rate_limit.attestor.as_str()) {
                    return Err(format!(
                        "rate limit references unknown attestor '{}'",
                        rate_limit.attestor
                    ));
                }
            }
        }

        if let Some(requirements) = &security.multisig_requirements {
            for requirement in requirements {
                for signatory in &requirement.signatory_attestors {
                    if !attestors.contains(&signatory.as_str()) {
                        return Err(format!(
                            "multisig requirement '{}' references unknown attestor '{}'",
                            requirement.operation, signatory
                        ));
                    }
                }
            }
        }
    }

    Ok(())
}
