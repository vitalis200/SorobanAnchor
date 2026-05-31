#!/usr/bin/env bash
# KYC Workflow Example
# Demonstrates a complete KYC lifecycle: submission, approval/rejection,
# attestation gated by KYC status, and status checks.
#
# Prerequisites:
#   - anchorkit binary in PATH  (cargo build --release && export PATH="$PWD/target/release:$PATH")
#   - ANCHOR_CONTRACT_ID env var set to a deployed contract
#   - STELLAR_NETWORK env var set (default: testnet)
#
# Run:
#   bash examples/kyc_workflow.sh

set -e

# ── Configuration ─────────────────────────────────────────────────────────────
CONTRACT_ID="${ANCHOR_CONTRACT_ID:-CBXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX}"
NETWORK="${STELLAR_NETWORK:-testnet}"

# Addresses (replace with real Stellar public keys in production)
ADMIN_ACCOUNT="anchor-admin"                                    # Stellar CLI keystore alias
ATTESTOR_ADDRESS="GBBD6A7KNZF5WNWQEPZP5DYJD2AYUTLXRB6VXJ4RCX4RTNPPQVNF3GQ"
USER_ADDRESS="GABC123XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX"

GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

echo "=== AnchorKit KYC Workflow Example ==="
echo "Network:     $NETWORK"
echo "Contract ID: $CONTRACT_ID"
echo ""

# ── Step 1: Register a KYC-capable attestor ───────────────────────────────────
echo -e "${BLUE}Step 1: Register KYC attestor${NC}"
echo "--------------------------------------"
echo "Registering attestor with KYC service..."
echo ""
echo "  Command:"
echo "    anchorkit register \\"
echo "      --address $ATTESTOR_ADDRESS \\"
echo "      --services deposits,withdrawals,kyc \\"
echo "      --contract-id $CONTRACT_ID \\"
echo "      --network $NETWORK \\"
echo "      --source $ADMIN_ACCOUNT \\"
echo "      --sep10-token \$SEP10_JWT \\"
echo "      --sep10-issuer \$SEP10_ISSUER"
echo ""

# Simulate (remove the 'echo' prefix and uncomment to run live):
# anchorkit register \
#   --address "$ATTESTOR_ADDRESS" \
#   --services deposits,withdrawals,kyc \
#   --contract-id "$CONTRACT_ID" \
#   --network "$NETWORK" \
#   --source "$ADMIN_ACCOUNT" \
#   --sep10-token "$SEP10_JWT" \
#   --sep10-issuer "$SEP10_ISSUER"

echo -e "  ${GREEN}✓ Attestor registered with KYC capability${NC}"
echo ""

# ── Step 2: Submit KYC data for a user ────────────────────────────────────────
echo -e "${BLUE}Step 2: Submit KYC data${NC}"
echo "--------------------------------------"
echo "User $USER_ADDRESS submits KYC documents."
echo ""
echo "  The contract call (Rust API):"
cat <<'RUST'
    // Compute a deterministic hash of the KYC payload
    let kyc_payload = serde_json::json!({
        "full_name": "[name]",
        "date_of_birth": "[dob]",
        "document_type": "passport",
        "document_number": "[doc_number]",
        "country": "US"
    });
    let payload_hash = contract.generate_request_id(
        env,
        user_address.clone(),
        ledger_timestamp,
    );

    // Submit KYC — status becomes Pending
    contract.submit_kyc(
        env,
        user_address.clone(),
        payload_hash,
        attestor_address.clone(),
    );
RUST
echo ""
echo -e "  ${GREEN}✓ KYC submitted — status: Pending (error code 20 if checked now)${NC}"
echo ""

# ── Step 3: Check KYC status (pending) ────────────────────────────────────────
echo -e "${BLUE}Step 3: Check KYC status (pending)${NC}"
echo "--------------------------------------"
echo "  Rust API:"
cat <<'RUST'
    let status = contract.get_kyc_status(env, user_address.clone());
    // Returns: KycStatus::Pending
    // Attempting submit_attestation_kyc_check here returns ErrorCode::KycPending (20)
RUST
echo ""
echo -e "  ${YELLOW}⏳ Status: Pending — attestations blocked until approved${NC}"
echo ""

# ── Step 4a: Approve KYC ──────────────────────────────────────────────────────
echo -e "${BLUE}Step 4a: Approve KYC (happy path)${NC}"
echo "--------------------------------------"
echo "  Rust API (called by the attestor after off-chain review):"
cat <<'RUST'
    contract.approve_kyc(
        env,
        user_address.clone(),
        attestor_address.clone(),   // must be a registered attestor
    );
    // KYC status is now Approved
RUST
echo ""
echo -e "  ${GREEN}✓ KYC approved — user may now submit KYC-gated attestations${NC}"
echo ""

# ── Step 4b: Reject KYC (alternative path) ────────────────────────────────────
echo -e "${BLUE}Step 4b: Reject KYC (rejection path)${NC}"
echo "--------------------------------------"
echo "  Rust API:"
cat <<'RUST'
    contract.reject_kyc(
        env,
        user_address.clone(),
        attestor_address.clone(),
        "Document expired".to_string(),
    );
    // KYC status is now Rejected
    // submit_attestation_kyc_check returns ErrorCode::KycRejected (21)
RUST
echo ""
echo -e "  ${RED}✗ KYC rejected — user must resubmit with valid documents${NC}"
echo ""

# ── Step 5: Submit a KYC-gated attestation ────────────────────────────────────
echo -e "${BLUE}Step 5: Submit KYC-gated attestation (after approval)${NC}"
echo "--------------------------------------"
echo "  CLI:"
echo ""
echo "    anchorkit attest \\"
echo "      --subject $USER_ADDRESS \\"
echo "      --payload-hash \$(echo -n 'deposit:usdc:1000' | sha256sum | awk '{print \$1}') \\"
echo "      --contract-id $CONTRACT_ID \\"
echo "      --network $NETWORK \\"
echo "      --credential-name kyc-attestor-key"
echo ""
echo "  Rust API (KYC-enforced variant):"
cat <<'RUST'
    // This call checks KYC status before writing the attestation.
    // Returns ErrorCode::KycPending (20) or KycRejected (21) if not approved.
    let attestation_id = contract.submit_attestation_kyc_check(
        env,
        attestor_address.clone(),
        user_address.clone(),
        ledger_timestamp,
        payload_hash,
        signature,
    )?;
    println!("Attestation ID: {}", attestation_id);
RUST
echo ""
echo -e "  ${GREEN}✓ Attestation submitted — ID recorded on-chain${NC}"
echo ""

# ── Step 6: Retrieve the attestation ──────────────────────────────────────────
echo -e "${BLUE}Step 6: Retrieve attestation${NC}"
echo "--------------------------------------"
echo "  Rust API:"
cat <<'RUST'
    let record = contract.get_attestation(env, attestation_id)?;
    println!("Issuer:       {}", record.issuer);
    println!("Subject:      {}", record.subject);
    println!("Timestamp:    {}", record.timestamp);
    println!("Payload hash: {}", record.payload_hash);
RUST
echo ""
echo -e "  ${GREEN}✓ Attestation verified on-chain${NC}"
echo ""

# ── Step 7: Session-batched attestations (efficiency) ─────────────────────────
echo -e "${BLUE}Step 7: Batch multiple attestations in one session${NC}"
echo "--------------------------------------"
echo "  Use sessions to amortize per-call overhead when submitting many attestations:"
echo ""
cat <<'RUST'
    // Create a session (2 persistent writes)
    let session_id = contract.create_session(
        env,
        attestor_address.clone(),
        "kyc-batch-2024-05".to_string(),
    )?;

    // Submit up to 100 attestations under the same session
    for (subject, hash, sig) in batch {
        contract.submit_attestation_with_session(
            env,
            session_id,
            attestor_address.clone(),
            subject,
            timestamp,
            hash,
            sig,
        )?;
    }

    // Close the session to finalize audit log
    contract.close_session(env, session_id, attestor_address.clone())?;
RUST
echo ""
echo -e "  ${GREEN}✓ Batch complete — session audit log written${NC}"
echo ""

# ── Summary ───────────────────────────────────────────────────────────────────
echo "=== KYC Workflow Summary ==="
echo ""
echo "  KYC lifecycle:"
echo "    submit_kyc()        → status: Pending"
echo "    approve_kyc()       → status: Approved  (attestations allowed)"
echo "    reject_kyc()        → status: Rejected  (resubmission required)"
echo ""
echo "  Relevant error codes:"
echo "    19  KycNotFound     — approve/reject called before submit_kyc"
echo "    20  KycPending      — attestation attempted while KYC is pending"
echo "    21  KycRejected     — attestation attempted after KYC rejection"
echo ""
echo "  Further reading:"
echo "    docs/error-codes.md"
echo "    docs/gas-and-storage-costs.md"
echo "    examples/attestation_workflow.sh"
