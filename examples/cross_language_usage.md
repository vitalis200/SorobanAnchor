# AnchorKit Cross-Language Usage Guide

AnchorKit ships as two interfaces: a **Rust library** (on-chain WASM + off-chain native) and a **CLI binary** (`anchorkit`). This guide shows how to integrate from Rust, shell scripts, and any language that can invoke a subprocess or call a Soroban RPC endpoint.

---

## 1. Rust (native / off-chain)

Add to `Cargo.toml`:

```toml
[dependencies]
anchorkit = { path = "." }   # or publish to crates.io and use version = "..."
```

### SEP-6 deposit normalization

```rust
use anchorkit::sep6::{initiate_deposit, RawDepositResponse};

let raw = RawDepositResponse {
    transaction_id: "txn-001".into(),
    how: "Send to bank account 1234".into(),
    min_amount: Some(10),
    max_amount: Some(10_000),
    fee_fixed: Some(1),
    status: Some("pending_external".into()),
    ..Default::default()
};
let deposit = initiate_deposit(raw)?;
println!("tx: {}, status: {:?}", deposit.transaction_id, deposit.status);
```

### SEP-24 interactive deposit

```rust
use anchorkit::sep24::{initiate_interactive_deposit, RawInteractiveDepositResponse};

let raw = RawInteractiveDepositResponse {
    url: "https://anchor.example.com/interactive/deposit".into(),
    id: "txn-002".into(),
};
let resp = initiate_interactive_deposit(raw)?;
// Redirect the user's browser to resp.url
println!("Redirect to: {}", resp.url);
```

### Retry with exponential backoff

```rust
use anchorkit::retry::{retry_with_backoff, RetryConfig};

let config = RetryConfig::default(); // 3 attempts, 200 ms base delay
let result = retry_with_backoff(
    &config,
    |attempt| fetch_transaction_status("txn-001", attempt),
    |err| matches!(err, TransientError),
    |delay_ms| std::thread::sleep(std::time::Duration::from_millis(delay_ms)),
)?;
```

### Domain validation

```rust
use anchorkit::validate_anchor_domain;

validate_anchor_domain("https://anchor.example.com")?;  // OK
validate_anchor_domain("http://anchor.example.com")?;   // Err: HTTP not allowed
```

---

## 2. Shell / CLI

The `anchorkit` binary exposes all major workflows. Build it once:

```bash
cargo build --release
export PATH="$PWD/target/release:$PATH"
```

### Registration

```bash
anchorkit register \
  --address GATTESTOR_ADDRESS \
  --services deposits,withdrawals,kyc \
  --contract-id "$ANCHOR_CONTRACT_ID" \
  --network testnet \
  --source anchor-admin \
  --sep10-token "$SEP10_JWT" \
  --sep10-issuer "$SEP10_ISSUER"
```

### Attestation submission

```bash
PAYLOAD_HASH=$(echo -n "deposit:usdc:500:$(date +%s)" | sha256sum | awk '{print $1}')

anchorkit attest \
  --subject GUSER_ADDRESS \
  --payload-hash "$PAYLOAD_HASH" \
  --contract-id "$ANCHOR_CONTRACT_ID" \
  --network testnet \
  --credential-name kyc-attestor-key
```

### Quote retrieval

```bash
anchorkit quote \
  --from USD \
  --to USDC \
  --amount 1000 \
  --contract-id "$ANCHOR_CONTRACT_ID" \
  --network testnet \
  --credential-name anchor-admin-key
```

### Contract deployment

```bash
# Testnet
anchorkit deploy \
  --network testnet \
  --source anchor-admin \
  --admin GADMIN_ADDRESS

# Mainnet upgrade
anchorkit deploy \
  --upgrade \
  --contract-id "$ANCHOR_CONTRACT_ID" \
  --network mainnet \
  --source anchor-admin
```

### Environment check

```bash
anchorkit doctor
# Use --fix to auto-resolve common issues
anchorkit doctor --fix
```

---

## 3. Python (via subprocess)

Any language that can run a subprocess can drive the CLI:

```python
import subprocess, json, os

def attest(subject: str, payload_hash: str) -> str:
    result = subprocess.run(
        [
            "anchorkit", "attest",
            "--subject", subject,
            "--payload-hash", payload_hash,
            "--contract-id", os.environ["ANCHOR_CONTRACT_ID"],
            "--network", os.environ.get("STELLAR_NETWORK", "testnet"),
            "--credential-name", "kyc-attestor-key",
        ],
        capture_output=True,
        text=True,
        check=True,
    )
    # CLI prints the attestation ID on stdout
    return result.stdout.strip()

attestation_id = attest("GUSER_ADDRESS", "abc123deadbeef...")
print(f"Attestation ID: {attestation_id}")
```

---

## 4. JavaScript / TypeScript (via Soroban RPC)

Call the deployed contract directly using the Stellar SDK:

```typescript
import { Contract, SorobanRpc, TransactionBuilder, Networks, Keypair } from "@stellar/stellar-sdk";

const server = new SorobanRpc.Server("https://soroban-testnet.stellar.org");
const contract = new Contract(process.env.ANCHOR_CONTRACT_ID!);
const keypair = Keypair.fromSecret(process.env.ANCHOR_ADMIN_SECRET!);

// Build a submit_attestation transaction
const account = await server.getAccount(keypair.publicKey());
const tx = new TransactionBuilder(account, {
  fee: "100",
  networkPassphrase: Networks.TESTNET,
})
  .addOperation(
    contract.call(
      "submit_attestation",
      // issuer, subject, timestamp, payload_hash, signature
      // (encode as Soroban XDR ScVal types)
    )
  )
  .setTimeout(30)
  .build();

const prepared = await server.prepareTransaction(tx);
prepared.sign(keypair);
const result = await server.sendTransaction(prepared);
console.log("Attestation submitted:", result.hash);
```

> See the [Stellar JS SDK docs](https://stellar.github.io/js-stellar-sdk/) for full ScVal encoding details.

---

## 5. Scenario quick-reference

| Scenario | Shell example | Rust API |
|----------|--------------|----------|
| Register attestor | `anchorkit register ...` | `contract.register_attestor()` |
| Submit attestation | `anchorkit attest ...` | `contract.submit_attestation()` |
| KYC submission | see `examples/kyc_workflow.sh` | `contract.submit_kyc()` |
| KYC approval | — | `contract.approve_kyc()` |
| Get quote | `anchorkit quote ...` | `contract.route_anchors()` |
| SEP-6 deposit | — | `initiate_deposit(raw)` |
| SEP-24 interactive | — | `initiate_interactive_deposit(raw)` |
| Deploy contract | `anchorkit deploy ...` | — |
| Revoke attestor | `anchorkit revoke ...` | `contract.revoke_attestor()` |

---

## Further reading

- `docs/error-codes.md` — full error code reference
- `docs/gas-and-storage-costs.md` — on-chain cost guide
- `docs/secret-file-encryption.md` — secret management
- `examples/kyc_workflow.sh` — end-to-end KYC lifecycle
- `examples/attestation_workflow.sh` — attestation submission and verification
- `examples/anchor_routing_example.sh` — multi-anchor routing strategies
