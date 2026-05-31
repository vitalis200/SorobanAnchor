#!/usr/bin/env bash
# Attestation Workflow Example
# Covers: attestor registration, attestation submission (plain, session-batched,
# and request-ID-traced), replay protection, and status verification.
#
# Prerequisites:
#   - anchorkit binary in PATH
#   - ANCHOR_CONTRACT_ID env var set
#   - STELLAR_NETWORK env var set (default: testnet)
#
# Run:
#   bash examples/attestation_workflow.sh

set -e

# ── Configuration ─────────────────────────────────────────────────────────────
CONTRACT_ID="${ANCHOR_CONTRACT_ID:-CBXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX}"
NETWORK="${STELLAR_NETWORK:-testnet}"
ADMIN_ACCOUNT="anchor-admin"
ATTESTOR_ADDRESS="GBBD6A7KNZF5WNWQEPZP5DYJD2AYUTLXRB6VXJ4RCX4RTNPPQVNF3GQ"
SUBJECT_ADDRESS="GABC123XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX"

GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo "=== AnchorKit Attestation Workflow Example ==="
echo "Network:     $NETWORK"
echo "Contract ID: $CONTRACT_ID"
echo ""

# ── Step 1: Register an attestor ──────────────────────────────────────────────
echo -e "${BLUE}Step 1: Register attestor${NC}"
echo "--------------------------------------"
echo "  Command:"
echo "    anchorkit register \\"
echo "      --address $ATTESTOR_ADDRESS \\"
echo "      --services deposits,withdrawals \\"
echo "      --contract-id $CONTRACT_ID \\"
echo "      --network $NETWORK \\"
echo "      --source $ADMIN_ACCOUNT \\"
echo "      --sep10-token \$SEP10_JWT \\"
echo "      --sep10-issuer \$SEP10_ISSUER"
echo ""
echo "  Rust API:"
cat <<'RUST'
    contract.register_attestor(
        env,
        attestor_address.clone(),
        vec![ServiceType::Deposits, ServiceType::Withdrawals],
        sep10_token,
        sep10_issuer,
    )?;
RUST
echo ""
echo -e "  ${GREEN}✓ Attestor registered${NC}"
echo ""

# ── Step 2: Verify attestor is active ─────────────────────────────────────────
echo -e "${BLUE}Step 2: Verify attestor is active${NC}"
echo "--------------------------------------"
cat <<'RUST'
    assert!(contract.is_attestor(env, attestor_address.clone()));
    let profile = contract.get_attestor_profile(env, attestor_address.clone())?;
    println!("Services: {:?}", profile.services);
RUST
echo ""
echo -e "  ${GREEN}✓ Attestor confirmed active${NC}"
echo ""

# ── Step 3: Submit a plain attestation ────────────────────────────────────────
echo -e "${BLUE}Step 3: Submit attestation (CLI)${NC}"
echo "--------------------------------------"
echo "  Compute a payload hash from your operation data:"
echo ""
echo "    PAYLOAD_HASH=\$(echo -n 'deposit:usdc:500:2024-05-30' | sha256sum | awk '{print \$1}')"
echo ""
echo "  Submit:"
echo "    anchorkit attest \\"
echo "      --subject $SUBJECT_ADDRESS \\"
echo "      --payload-hash \$PAYLOAD_HASH \\"
echo "      --contract-id $CONTRACT_ID \\"
echo "      --network $NETWORK \\"
echo "      --credential-name kyc-attestor-key"
echo ""
echo "  Rust API:"
cat <<'RUST'
    let attestation_id = contract.submit_attestation(
        env,
        attestor_address.clone(),   // issuer
        subject_address.clone(),
        ledger_timestamp,
        payload_hash,               // bytes32 SHA-256 of your operation data
        signature,                  // Ed25519 signature over (issuer || subject || timestamp || hash)
    )?;
    println!("Attestation ID: {}", attestation_id);
RUST
echo ""
echo -e "  ${GREEN}✓ Attestation written on-chain${NC}"
echo ""

# ── Step 4: Replay protection ─────────────────────────────────────────────────
echo -e "${BLUE}Step 4: Replay protection${NC}"
echo "--------------------------------------"
echo "  Submitting the same (issuer, payload_hash) pair a second time returns:"
echo "    ErrorCode::ReplayAttack (6)"
echo ""
echo "  The replay key is stored with a 7-day TTL. After expiry the slot is"
echo "  reclaimed, so payload hashes must be unique per operation — not per day."
echo ""
echo "  Use generate_request_id() to derive unique hashes:"
cat <<'RUST'
    // Deterministic: SHA-256(issuer || subject || ledger_sequence)
    let unique_hash = contract.generate_request_id(
        env,
        attestor_address.clone(),
        ledger_sequence,
    );
RUST
echo ""
echo -e "  ${YELLOW}⚠  Never reuse a payload hash across different operations${NC}"
echo ""

# ── Step 5: Attestation with distributed tracing ──────────────────────────────
echo -e "${BLUE}Step 5: Attestation with request tracing${NC}"
echo "--------------------------------------"
echo "  Use submit_with_request_id when you need end-to-end tracing across"
echo "  off-chain and on-chain systems. Adds 2 extra persistent writes."
echo ""
cat <<'RUST'
    let request_id = contract.generate_request_id(env, attestor_address.clone(), seq);

    let attestation_id = contract.submit_with_request_id(
        env,
        request_id.clone(),
        attestor_address.clone(),
        subject_address.clone(),
        ledger_timestamp,
        payload_hash,
        signature,
    )?;

    // Retrieve the tracing span later
    let span = contract.get_request_context(env, request_id)?;
    println!("Span ledger: {}", span.ledger_sequence);
RUST
echo ""
echo -e "  ${GREEN}✓ Attestation with tracing span recorded${NC}"
echo ""

# ── Step 6: Session-batched attestations ──────────────────────────────────────
echo -e "${BLUE}Step 6: Batch attestations in a session${NC}"
echo "--------------------------------------"
echo "  Sessions amortize per-call overhead. One session supports up to 100 ops."
echo ""
cat <<'RUST'
    let session_id = contract.create_session(
        env,
        attestor_address.clone(),
        "batch-deposit-2024-05".to_string(),
    )?;

    for (subject, hash, sig) in operations {
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

    contract.close_session(env, session_id, attestor_address.clone())?;
RUST
echo ""
echo -e "  ${GREEN}✓ Batch complete — audit log finalized${NC}"
echo ""

# ── Step 7: Retrieve and verify an attestation ────────────────────────────────
echo -e "${BLUE}Step 7: Retrieve attestation${NC}"
echo "--------------------------------------"
cat <<'RUST'
    let record = contract.get_attestation(env, attestation_id)?;
    // Verify fields match what was submitted
    assert_eq!(record.issuer,       attestor_address);
    assert_eq!(record.subject,      subject_address);
    assert_eq!(record.payload_hash, payload_hash);
    println!("Verified attestation at ledger {}", record.timestamp);
RUST
echo ""
echo -e "  ${GREEN}✓ Attestation verified${NC}"
echo ""

# ── Step 8: Revoke an attestor ────────────────────────────────────────────────
echo -e "${BLUE}Step 8: Revoke attestor (when key is compromised)${NC}"
echo "--------------------------------------"
echo "  CLI:"
echo "    anchorkit revoke \\"
echo "      --address $ATTESTOR_ADDRESS \\"
echo "      --contract-id $CONTRACT_ID \\"
echo "      --network $NETWORK \\"
echo "      --source $ADMIN_ACCOUNT"
echo ""
cat <<'RUST'
    contract.revoke_attestor(env, attestor_address.clone(), admin_address.clone())?;
    // is_attestor() now returns false; future submissions are rejected
RUST
echo ""
echo -e "  ${GREEN}✓ Attestor revoked — no further attestations accepted${NC}"
echo ""

# ── Summary ───────────────────────────────────────────────────────────────────
echo "=== Attestation Workflow Summary ==="
echo ""
echo "  Sequence:"
echo "    register_attestor()              → attestor active"
echo "    submit_attestation()             → attestation ID returned"
echo "    get_attestation(id)              → verify on-chain record"
echo "    revoke_attestor()                → attestor deactivated"
echo ""
echo "  Cost tips:"
echo "    - Use submit_attestation_with_session() for batches (amortizes overhead)"
echo "    - Avoid submit_with_request_id() unless distributed tracing is required"
echo "    - Keep registered anchor list small for route_anchors_weighted()"
echo ""
echo "  Relevant error codes:"
echo "    4   UnauthorizedAttestor  — signature mismatch"
echo "    5   InvalidTimestamp      — timestamp too old or in the future"
echo "    6   ReplayAttack          — duplicate (issuer, payload_hash)"
echo "    16  RateLimitExceeded     — per-attestor rate cap hit"
echo ""
echo "  Further reading:"
echo "    docs/error-codes.md"
echo "    docs/gas-and-storage-costs.md"
echo "    examples/kyc_workflow.sh"
