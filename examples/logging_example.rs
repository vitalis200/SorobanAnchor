/// Example demonstrating AnchorKit transaction state tracking and domain validation.
///
/// This replaces the previous logging example which referenced types that no longer
/// exist (LoggingConfig, Logger, RequestId). Use Soroban events for on-chain logging.
use anchorkit::{
    validate_anchor_domain,
    sep6::{initiate_deposit, RawDepositResponse},
    retry::{retry_with_backoff, RetryConfig},
};

fn main() {
    println!("🚀 AnchorKit Example");
    println!("====================");

    // 1. Domain validation
    println!("\n📋 Step 1: Domain validation");
    let domains = [
        "https://anchor.example.com",
        "https://api.stellar.org/sep6",
        "http://insecure.example.com",   // rejected: HTTP
        "https://192.168.1.1",           // rejected: IP address
    ];
    for domain in &domains {
        match validate_anchor_domain(domain) {
            Ok(()) => println!("  ✅ Valid:   {}", domain),
            Err(e) => println!("  ❌ Invalid: {} — {:?}", domain, e.code),
        }
    }

    // 2. SEP-6 deposit normalisation
    println!("\n📋 Step 2: SEP-6 deposit normalisation");
    let raw = RawDepositResponse {
        transaction_id: "txn-001".into(),
        how: "Send to bank account 1234".into(),
        extra_info: None,
        min_amount: Some(10),
        max_amount: Some(10_000),
        fee_fixed: Some(1),
        status: Some("pending_external".into()),
        clawback_enabled: None,
        stellar_memo: None,
        stellar_memo_type: None,
        asset_code: None,
    };
    match initiate_deposit(raw) {
        Ok(deposit) => println!("  ✅ Deposit txn: {}", deposit.transaction_id),
        Err(e) => println!("  ❌ Error: {:?}", e.code),
    }

    // 3. Retry with backoff
    println!("\n📋 Step 3: Retry with backoff");
    let config = RetryConfig::default();
    let mut attempts = 0u32;
    let result = retry_with_backoff(
        &config,
        |_attempt| -> Result<&str, u32> {
            attempts += 1;
            Ok("success")
        },
        |_err| false,
        |_ms| {},
    );
    println!("  ✅ Result: {:?} after {} attempt(s)", result, attempts);

    println!("\n🎉 Example completed!");
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_logging_example_runs() {
        super::main();
    }
}
