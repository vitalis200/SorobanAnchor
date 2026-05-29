# Gas and Storage Cost Guide

This document describes the on-chain cost profile for major `AnchorKitContract` methods, explains which operations are expensive, and provides guidance for minimizing fees and storage footprint.

---

## Storage types and TTLs

The contract uses three Soroban storage tiers:

| Tier       | Constant          | Ledgers       | ~Days (5 s/ledger) | Used for                                      |
|------------|-------------------|---------------|--------------------|-----------------------------------------------|
| Persistent | `PERSISTENT_TTL`  | 1,555,200     | ~90 days           | Attestations, attestors, KYC, services, quotes |
| Persistent | `REPLAY_TTL`      | 120,960       | ~7 days            | Replay-protection keys                        |
| Instance   | `INSTANCE_TTL`    | 518,400       | ~30 days           | Admin, counters, config, session counters     |
| Temporary  | `SPAN_TTL`        | 17,280        | ~1 day             | Tracing spans                                 |
| Temporary  | (caller-supplied) | variable      | variable           | Metadata cache, capabilities cache, TOML cache |

Every `extend_ttl` call costs a rent fee proportional to the number of ledgers extended and the byte size of the entry. Persistent entries are the most expensive to keep alive.

---

## Cost profile by method

### High cost — avoid in hot paths

#### `route_anchors_weighted`
- **Pattern:** Two full O(n) passes over the anchor list, each issuing 2–3 persistent storage reads per anchor (meta, latest-quote ID, quote record), followed by an in-memory sort.
- **Dominant cost:** `n × 3` persistent reads in pass 1 + `n × 3` persistent reads in pass 2 = `6n` reads total.
- **Guidance:**
  - Keep the registered anchor list small (< 20 anchors) for predictable fees.
  - Pre-filter anchors off-chain and pass only candidates to the contract.
  - Use `route_anchors` (single-strategy) instead of `route_anchors_weighted` when composite scoring is not needed.
  - Cache routing results off-chain for the quote's `valid_until` window.

#### `submit_attestation` / `submit_with_request_id`
- **Pattern:** 5–7 persistent storage writes per call: attestation record, replay-protection key (`USED`), rate-limiter state, attestation counter, and (for `submit_with_request_id`) a tracing span + request context.
- **Dominant cost:** Each persistent write triggers a rent extension at `PERSISTENT_TTL` (1,555,200 ledgers) or `REPLAY_TTL` (120,960 ledgers).
- **Guidance:**
  - Batch multiple attestations into a single session using `submit_attestation_with_session` to amortize session overhead.
  - Avoid `submit_with_request_id` in production unless distributed tracing is required — it adds 2 extra persistent writes.
  - Use `submit_attestation_kyc_check` only when KYC enforcement is mandatory; it adds a KYC record read.

#### `get_session_audit_logs`
- **Pattern:** O(limit) persistent reads — one `SLOG` index read + one `AUDIT` record read per log entry.
- **Guidance:**
  - Keep `limit` small (≤ 10) for read-only queries.
  - Prefer off-chain indexing of `audit.logged` events over on-chain log retrieval in production UIs.

---

### Medium cost — use with care

#### `configure_services_versioned`
- **Pattern:** O(|services|) in-memory duplicate check + 1 persistent write + 1 `extend_ttl`.
- **Guidance:** Call once per anchor at registration time. Re-configuring is idempotent but costs a full write + rent extension each time.

#### `fetch_anchor_info`
- **Pattern:** O(|currencies|) validation loop + 1 temporary storage write + `extend_ttl`.
- **Guidance:** Minimize the `currencies` array to only assets the anchor actively supports. Temporary storage is cheaper than persistent but still incurs rent.

#### `create_session`
- **Pattern:** 2 persistent writes (session record + nonce key) + 1 instance write (session counter).
- **Guidance:** Reuse sessions across related operations. Each session supports up to `MAX_OPS_PER_SESSION = 100` operations before a new one is needed.

#### `cache_metadata` / `cache_metadata_swr`
- **Pattern:** 1 temporary write + `extend_ttl`. Cheap individually, but repeated cache refreshes accumulate rent.
- **Guidance:** Use `cache_metadata_swr` with a generous `stale_ttl_seconds` to reduce refresh frequency. The stale-while-revalidate window lets you serve cached data while refreshing in the background.

---

### Low cost — safe in hot paths

| Method                        | Storage ops | Notes                                      |
|-------------------------------|-------------|--------------------------------------------|
| `is_attestor`                 | 1 read      | Single persistent read                     |
| `get_attestor_profile`        | 1 read      | Single persistent read                     |
| `get_cached_metadata`         | 1 read      | Single temporary read                      |
| `get_anchor_toml`             | 1 read      | Single temporary read                      |
| `get_kyc_status`              | 1 read      | Single persistent read                     |
| `supports_service`            | 1 read      | Single persistent read                     |
| `get_version`                 | 1 read      | Instance storage read                      |
| `generate_request_id`         | 0 reads     | Pure computation (SHA-256 + ledger state)  |
| `verify_sep10_token`          | 1 read      | Reads stored Ed25519 key, then pure crypto |

---

## Storage minimization examples

### Minimize attestation storage

```rust
// Expensive: separate tracing span + request context (7 writes)
let id = contract.submit_with_request_id(
    request_id, issuer, subject, timestamp, payload_hash, sig
);

// Cheaper: plain attestation (5 writes, no tracing overhead)
let id = contract.submit_attestation(
    issuer, subject, timestamp, payload_hash, sig
);
```

### Batch operations under a single session

```rust
// One session creation amortizes overhead across up to 100 operations
let session_id = contract.create_session(initiator.clone());

// Each session-scoped call shares the session's persistent entry
contract.submit_attestation_with_session(
    session_id, issuer, subject, timestamp, hash1, sig1
);
contract.submit_attestation_with_session(
    session_id, issuer, subject, timestamp, hash2, sig2
);

// Close when done to free the nonce slot
contract.close_session(session_id, initiator);
```

### Use temporary storage for short-lived data

```rust
// Metadata cache uses temporary storage — cheaper rent than persistent
contract.cache_metadata(anchor, metadata, 3_600); // 1-hour TTL

// SWR variant avoids redundant refreshes
contract.cache_metadata_swr(anchor, metadata, 3_600, 300); // 1 h primary + 5 min stale
```

### Avoid redundant routing calls

```rust
// Off-chain: cache the result for the quote's valid_until window
let quotes = contract.route_anchors_weighted(
    request, min_reputation, max_results,
    fee_weight, speed_weight, reputation_weight
);
// Store quotes locally until quotes[0].valid_until; only re-query after expiry
```

---

## Upgrade and migration cost considerations

### WASM upgrade (`upgrade`)
- Calls `env.deployer().update_current_contract_wasm(...)` — a one-time syscall with fixed overhead.
- Writes 2 instance entries (version record, old WASM hash) + `extend_ttl`.
- **Cost:** Low. Run once per release.

### Schema migration (`migrate`)
- The `migrate` function is idempotent (guarded by a per-patch nonce key).
- If migration iterates over stored records (e.g. to rewrite `SCHEMA_V1` → `SCHEMA_V2`), cost scales with the number of records: `O(n)` reads + `O(n)` writes.
- **Guidance:**
  - Migrate lazily (on first read of each record) rather than eagerly iterating all records in a single transaction. Soroban's instruction limit will reject migrations that touch more than ~50–100 large persistent entries in one call.
  - Split large migrations into paginated admin calls, each processing a bounded batch.
  - After migration, old entries under the previous schema version can be pruned to reclaim rent.

### TTL renewal after upgrade
- After a WASM upgrade, instance storage TTL is extended automatically by `upgrade` (`INSTANCE_TTL = 518,400 ledgers`).
- Persistent entries (attestations, KYC records, etc.) are **not** automatically renewed. If the contract is dormant for > 90 days, persistent entries may expire. Call `extend_ttl` on critical entries as part of a maintenance routine.

### Estimating rent costs
Soroban rent is charged as:

```
rent_fee = (entry_size_bytes × ledgers_extended) × write_fee_per_byte_per_ledger
```

Use `soroban contract invoke --cost` on testnet to measure actual instruction and fee usage before deploying to mainnet. The Stellar Laboratory fee estimator can project rent costs for a given entry size and TTL.

---

## Syscall limits reference

Soroban enforces per-transaction limits. Exceeding any limit causes the transaction to fail:

| Resource                  | Approximate limit (mainnet, 2025) |
|---------------------------|-----------------------------------|
| CPU instructions          | 100,000,000                       |
| Memory (bytes)            | 40,000,000                        |
| Read ledger entries       | 40                                |
| Write ledger entries      | 25                                |
| Read bytes                | 200,000                           |
| Write bytes               | 66,000                            |
| Events (topics + data)    | 8,000 bytes total                 |

`route_anchors_weighted` with 20 anchors performs ~40 persistent reads (2 passes × 20 anchors), which approaches the read-entry limit. Keep anchor lists under 15 entries for a safe margin.

> **Note:** Limits are subject to change via Stellar Core upgrades. Always verify against the current network configuration using `soroban network status` or the Horizon `/fee_stats` endpoint.
