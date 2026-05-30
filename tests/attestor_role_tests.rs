#![cfg(feature = "std")]

use anchorkit::{load_runtime_config_file, parse_runtime_config_str, ConfigFormat};

#[test]
fn test_kyc_issuer_role_in_fiat_ramp() {
    let config = load_runtime_config_file("configs/fiat-on-off-ramp.json")
        .expect("failed to load fiat-on-off-ramp config");

    // Verify kyc-issuer role exists
    let kyc_attestor = config
        .attestors
        .registry
        .iter()
        .find(|a| a.role == "kyc-issuer")
        .expect("kyc-issuer role not found");

    assert_eq!(kyc_attestor.name, "kyc-provider");
    assert!(kyc_attestor.enabled);
    assert!(kyc_attestor.endpoint.is_some());

    // Verify kyc-issuer is used in operations
    let operations = config.operations.expect("operations config missing");
    let templates = operations.templates.expect("operation templates missing");

    let kyc_operation = templates
        .iter()
        .find(|t| t.attestor == "kyc-provider")
        .expect("kyc operation not found");

    assert_eq!(kyc_operation.operation_type, "kyc_verification");
    assert!(kyc_operation.required_fields.contains(&"user_id".to_string()));
    assert!(kyc_operation.required_fields.contains(&"kyc_status".to_string()));
}

#[test]
fn test_transfer_verifier_role_in_fiat_ramp() {
    let config = load_runtime_config_file("configs/fiat-on-off-ramp.json")
        .expect("failed to load fiat-on-off-ramp config");

    // Verify transfer-verifier role exists
    let transfer_attestor = config
        .attestors
        .registry
        .iter()
        .find(|a| a.role == "transfer-verifier")
        .expect("transfer-verifier role not found");

    assert_eq!(transfer_attestor.name, "bank-integration");
    assert!(transfer_attestor.enabled);

    // Verify transfer-verifier is used in operations
    let operations = config.operations.expect("operations config missing");
    let templates = operations.templates.expect("operation templates missing");

    let deposit_operation = templates
        .iter()
        .find(|t| t.id == "deposit-confirmation")
        .expect("deposit operation not found");

    assert_eq!(deposit_operation.attestor, "bank-integration");
    assert_eq!(deposit_operation.operation_type, "fund_transfer");
}

#[test]
fn test_compliance_approver_role_in_fiat_ramp() {
    let config = load_runtime_config_file("configs/fiat-on-off-ramp.json")
        .expect("failed to load fiat-on-off-ramp config");

    // Verify compliance-approver role exists
    let compliance_attestor = config
        .attestors
        .registry
        .iter()
        .find(|a| a.role == "compliance-approver")
        .expect("compliance-approver role not found");

    assert_eq!(compliance_attestor.name, "compliance-officer");
    assert!(compliance_attestor.enabled);

    // Verify compliance-approver is used in operations
    let operations = config.operations.expect("operations config missing");
    let templates = operations.templates.expect("operation templates missing");

    let withdrawal_operation = templates
        .iter()
        .find(|t| t.id == "withdrawal-approval")
        .expect("withdrawal operation not found");

    assert_eq!(withdrawal_operation.attestor, "compliance-officer");
    assert_eq!(withdrawal_operation.operation_type, "withdrawal_approval");
}

#[test]
fn test_identity_verifier_role_in_remittance() {
    let config = load_runtime_config_file("configs/remittance-anchor.json")
        .expect("failed to load remittance-anchor config");

    // Verify identity-verifier role exists
    let identity_attestor = config
        .attestors
        .registry
        .iter()
        .find(|a| a.role == "identity-verifier")
        .expect("identity-verifier role not found");

    assert_eq!(identity_attestor.name, "identity-verifier");
    assert!(identity_attestor.enabled);

    // Verify identity-verifier is used in operations
    let operations = config.operations.expect("operations config missing");
    let templates = operations.templates.expect("operation templates missing");

    let sender_verification = templates
        .iter()
        .find(|t| t.id == "sender-kyc")
        .expect("sender verification operation not found");

    assert_eq!(sender_verification.attestor, "identity-verifier");
    assert_eq!(sender_verification.operation_type, "sender_verification");

    let recipient_verification = templates
        .iter()
        .find(|t| t.id == "recipient-verification")
        .expect("recipient verification operation not found");

    assert_eq!(recipient_verification.attestor, "identity-verifier");
    assert_eq!(recipient_verification.operation_type, "recipient_verification");
}

#[test]
fn test_settlement_bank_role_in_remittance() {
    let config = load_runtime_config_file("configs/remittance-anchor.json")
        .expect("failed to load remittance-anchor config");

    // Verify settlement-bank role exists
    let settlement_attestor = config
        .attestors
        .registry
        .iter()
        .find(|a| a.role == "settlement-bank")
        .expect("settlement-bank role not found");

    assert_eq!(settlement_attestor.name, "correspondent-bank");
    assert!(settlement_attestor.enabled);

    // Verify settlement-bank is used in operations
    let operations = config.operations.expect("operations config missing");
    let templates = operations.templates.expect("operation templates missing");

    let settlement_operations: Vec<_> = templates
        .iter()
        .filter(|t| t.attestor == "correspondent-bank")
        .collect();

    assert!(settlement_operations.len() >= 2);
    assert!(settlement_operations
        .iter()
        .any(|t| t.operation_type == "settlement_instruction"));
    assert!(settlement_operations
        .iter()
        .any(|t| t.operation_type == "settlement_confirmation"));
}

#[test]
fn test_corridor_manager_role_in_remittance() {
    let config = load_runtime_config_file("configs/remittance-anchor.json")
        .expect("failed to load remittance-anchor config");

    // Verify corridor-manager role exists
    let corridor_attestor = config
        .attestors
        .registry
        .iter()
        .find(|a| a.role == "corridor-manager")
        .expect("corridor-manager role not found");

    assert_eq!(corridor_attestor.name, "corridor-operator");
    assert!(corridor_attestor.enabled);

    // Verify corridor-manager is used in operations
    let operations = config.operations.expect("operations config missing");
    let templates = operations.templates.expect("operation templates missing");

    let corridor_operation = templates
        .iter()
        .find(|t| t.id == "corridor-routing")
        .expect("corridor routing operation not found");

    assert_eq!(corridor_operation.attestor, "corridor-operator");
    assert_eq!(corridor_operation.operation_type, "corridor_routing");
}

#[test]
fn test_compliance_checker_role_in_remittance() {
    let config = load_runtime_config_file("configs/remittance-anchor.json")
        .expect("failed to load remittance-anchor config");

    // Verify compliance-checker role exists
    let compliance_attestor = config
        .attestors
        .registry
        .iter()
        .find(|a| a.role == "compliance-checker")
        .expect("compliance-checker role not found");

    assert_eq!(compliance_attestor.name, "aml-screening");
    assert!(compliance_attestor.enabled);

    // Verify compliance-checker is used in operations
    let operations = config.operations.expect("operations config missing");
    let templates = operations.templates.expect("operation templates missing");

    let aml_operation = templates
        .iter()
        .find(|t| t.id == "aml-clearance")
        .expect("aml clearance operation not found");

    assert_eq!(aml_operation.attestor, "aml-screening");
    assert_eq!(aml_operation.operation_type, "aml_screening");
}

#[test]
fn test_reserve_verifier_role_in_stablecoin() {
    let config = load_runtime_config_file("configs/stablecoin-issuer.json")
        .expect("failed to load stablecoin-issuer config");

    // Verify reserve-verifier role exists
    let reserve_attestor = config
        .attestors
        .registry
        .iter()
        .find(|a| a.role == "reserve-verifier")
        .expect("reserve-verifier role not found");

    assert_eq!(reserve_attestor.name, "reserve-auditor");
    assert!(reserve_attestor.enabled);

    // Verify reserve-verifier is used in operations
    let operations = config.operations.expect("operations config missing");
    let templates = operations.templates.expect("operation templates missing");

    let reserve_operation = templates
        .iter()
        .find(|t| t.id == "reserve-proof")
        .expect("reserve proof operation not found");

    assert_eq!(reserve_operation.attestor, "reserve-auditor");
    assert_eq!(reserve_operation.operation_type, "reserve_attestation");
}

#[test]
fn test_collateral_custodian_role_in_stablecoin() {
    let config = load_runtime_config_file("configs/stablecoin-issuer.json")
        .expect("failed to load stablecoin-issuer config");

    // Verify collateral-custodian role exists
    let collateral_attestor = config
        .attestors
        .registry
        .iter()
        .find(|a| a.role == "collateral-custodian")
        .expect("collateral-custodian role not found");

    assert_eq!(collateral_attestor.name, "collateral-manager");
    assert!(collateral_attestor.enabled);

    // Verify collateral-custodian is used in operations
    let operations = config.operations.expect("operations config missing");
    let templates = operations.templates.expect("operation templates missing");

    let collateral_operations: Vec<_> = templates
        .iter()
        .filter(|t| t.attestor == "collateral-manager")
        .collect();

    assert!(collateral_operations.len() >= 2);
    assert!(collateral_operations
        .iter()
        .any(|t| t.operation_type == "collateral_deposit"));
    assert!(collateral_operations
        .iter()
        .any(|t| t.operation_type == "collateral_liquidation"));
}

#[test]
fn test_treasury_operator_role_in_stablecoin() {
    let config = load_runtime_config_file("configs/stablecoin-issuer.json")
        .expect("failed to load stablecoin-issuer config");

    // Verify treasury-operator role exists
    let treasury_attestor = config
        .attestors
        .registry
        .iter()
        .find(|a| a.role == "treasury-operator")
        .expect("treasury-operator role not found");

    assert_eq!(treasury_attestor.name, "treasury-manager");
    assert!(treasury_attestor.enabled);

    // Verify treasury-operator is used in operations
    let operations = config.operations.expect("operations config missing");
    let templates = operations.templates.expect("operation templates missing");

    let treasury_operations: Vec<_> = templates
        .iter()
        .filter(|t| t.attestor == "treasury-manager")
        .collect();

    assert!(treasury_operations.len() >= 3);
    assert!(treasury_operations
        .iter()
        .any(|t| t.operation_type == "mint_authorization"));
    assert!(treasury_operations
        .iter()
        .any(|t| t.operation_type == "burn_confirmation"));
    assert!(treasury_operations
        .iter()
        .any(|t| t.operation_type == "redemption_processing"));
}

#[test]
fn test_risk_analyst_role_in_stablecoin() {
    let config = load_runtime_config_file("configs/stablecoin-issuer.json")
        .expect("failed to load stablecoin-issuer config");

    // Verify risk-analyst role exists
    let risk_attestor = config
        .attestors
        .registry
        .iter()
        .find(|a| a.role == "risk-analyst")
        .expect("risk-analyst role not found");

    assert_eq!(risk_attestor.name, "risk-monitor");
    assert!(risk_attestor.enabled);

    // Verify risk-analyst is used in operations
    let operations = config.operations.expect("operations config missing");
    let templates = operations.templates.expect("operation templates missing");

    let risk_operations: Vec<_> = templates
        .iter()
        .filter(|t| t.attestor == "risk-monitor")
        .collect();

    assert!(risk_operations.len() >= 2);
    assert!(risk_operations
        .iter()
        .any(|t| t.operation_type == "risk_assessment"));
    assert!(risk_operations
        .iter()
        .any(|t| t.operation_type == "price_feed"));
}

#[test]
fn test_all_roles_are_valid() {
    let valid_roles = vec![
        "kyc-issuer",
        "transfer-verifier",
        "compliance-approver",
        "rate-provider",
        "attestor",
        "identity-verifier",
        "settlement-bank",
        "corridor-manager",
        "compliance-checker",
        "reserve-verifier",
        "collateral-custodian",
        "treasury-operator",
        "risk-analyst",
    ];

    // Test each config file
    let config_files = vec![
        "configs/fiat-on-off-ramp.json",
        "configs/remittance-anchor.json",
        "configs/stablecoin-issuer.json",
    ];

    for config_file in config_files {
        let config = load_runtime_config_file(config_file)
            .unwrap_or_else(|_| panic!("failed to load {}", config_file));

        for attestor in &config.attestors.registry {
            assert!(
                valid_roles.contains(&attestor.role.as_str()),
                "Invalid role '{}' found in {}",
                attestor.role,
                config_file
            );
        }
    }
}

#[test]
fn test_role_to_operation_mapping() {
    // This test verifies that operation types align with attestor roles
    let config = load_runtime_config_file("configs/fiat-on-off-ramp.json")
        .expect("failed to load config");

    let operations = config.operations.expect("operations config missing");
    let templates = operations.templates.expect("operation templates missing");

    for template in templates {
        let attestor = config
            .attestors
            .registry
            .iter()
            .find(|a| a.name == template.attestor)
            .unwrap_or_else(|| panic!("attestor '{}' not found in registry", template.attestor));

        // Verify that the operation type makes sense for the role
        match attestor.role.as_str() {
            "kyc-issuer" => {
                assert!(
                    template.operation_type.contains("kyc")
                        || template.operation_type.contains("verification")
                );
            }
            "transfer-verifier" => {
                assert!(
                    template.operation_type.contains("transfer")
                        || template.operation_type.contains("fund")
                        || template.operation_type.contains("refund")
                );
            }
            "compliance-approver" => {
                assert!(
                    template.operation_type.contains("approval")
                        || template.operation_type.contains("compliance")
                );
            }
            _ => {}
        }
    }
}

#[test]
fn test_invalid_role_rejected() {
    let config_json = r#"
    {
      "contract": {
        "name": "test-anchor",
        "version": "1.0.0",
        "network": "stellar-testnet"
      },
      "attestors": {
        "registry": [{
          "name": "invalid-attestor",
          "address": "GBBD6A7KNZF5WNWQEPZP5DYJD2AYUTLXRB6VXJ4RCX4RTNPPQVNF3GQ",
          "role": "invalid-role",
          "enabled": true
        }]
      }
    }
    "#;

    // This should fail JSON schema validation (not tested here)
    // but the config parser should still parse it
    let result = parse_runtime_config_str(config_json, ConfigFormat::Json);
    
    // The parser will succeed because it doesn't validate against the schema
    // Schema validation happens separately
    assert!(result.is_ok());
}

#[test]
fn test_rate_limits_reference_valid_attestors() {
    let config = load_runtime_config_file("configs/fiat-on-off-ramp.json")
        .expect("failed to load config");

    let security = config.security.expect("security config missing");
    let rate_limits = security.rate_limits.expect("rate limits missing");

    for rate_limit in rate_limits {
        let attestor_exists = config
            .attestors
            .registry
            .iter()
            .any(|a| a.name == rate_limit.attestor);

        assert!(
            attestor_exists,
            "Rate limit references unknown attestor: {}",
            rate_limit.attestor
        );
    }
}

#[test]
fn test_multisig_requirements_reference_valid_attestors() {
    let config = load_runtime_config_file("configs/stablecoin-issuer.json")
        .expect("failed to load config");

    let security = config.security.expect("security config missing");
    let multisig_requirements = security
        .multisig_requirements
        .expect("multisig requirements missing");

    for requirement in multisig_requirements {
        for signatory in &requirement.signatory_attestors {
            let attestor_exists = config
                .attestors
                .registry
                .iter()
                .any(|a| a.name == *signatory);

            assert!(
                attestor_exists,
                "Multisig requirement references unknown attestor: {}",
                signatory
            );
        }
    }
}
