# Dependency Audit

## License Policy

AnchorKit is distributed under the **MIT** license. All third-party dependencies must be compatible with this license.

### Approved Licenses

| License | Allowed | Notes |
|---------|---------|-------|
| MIT | ✅ Yes | Compatible with the project license |
| Apache-2.0 | ✅ Yes | Compatible; requires NOTICE file preservation |
| MIT OR Apache-2.0 | ✅ Yes | Most common Rust crate dual-license |
| BSD-2-Clause | ✅ Yes | Permissive, compatible |
| BSD-3-Clause | ✅ Yes | Permissive, compatible |
| ISC | ✅ Yes | Functionally equivalent to MIT |
| CC0-1.0 | ✅ Yes | Public domain dedication |
| Unicode-3.0 | ✅ Yes | Permissive, data only |
| Zlib | ✅ Yes | Permissive, compatible |
| LGPL-2.0+ / LGPL-3.0 | ⚠️ Review | Only if used as a dynamically-linked library |
| GPL-2.0 / GPL-3.0 | ❌ No | Copyleft; incompatible with MIT distribution |
| AGPL-3.0 | ❌ No | Copyleft; incompatible with MIT distribution |
| SSPL | ❌ No | Not an OSI-approved open-source license |
| Proprietary | ❌ No | Requires case-by-case legal review |

## Dependency Review Process

### Before Adding a New Dependency

1. **Necessity check**: Can the functionality be implemented without a new dependency (stdlib, minor utility code)?
2. **License check**: Run `cargo license --color never` and verify the new dependency's license is in the approved list above.
3. **Security check**: Run `cargo audit` to confirm there are no known vulnerabilities.
4. **Maintenance check**: Is the crate actively maintained? Check crates.io for last publish date and GitHub issues.
5. **Transitive dependency check**: Review what new transitive dependencies are pulled in (`cargo tree -p <crate>`).
6. **PR description**: Include a "Dependency Review" section in the PR description using the checklist below.

### Dependency Review Checklist (add to PRs that add/update dependencies)

```markdown
## Dependency Review

- [ ] License is in the approved list (see DEPENDENCY_AUDIT.md)
- [ ] `cargo audit` reports no known vulnerabilities for this crate
- [ ] The crate has been published to crates.io within the last 12 months (or is a well-known long-stable crate)
- [ ] Transitive dependencies reviewed: no new GPL/AGPL/SSPL dependencies introduced
- [ ] The new functionality cannot reasonably be implemented with the existing dependency set or stdlib
```

## Current Direct Dependencies

Generated from `Cargo.toml` as of 2026-05-30. Run `scripts/dependency-audit.sh` for an up-to-date report.

| Crate | Version | License | Purpose |
|-------|---------|---------|---------|
| soroban-sdk | 21.7.0 | Apache-2.0 | Soroban smart contract SDK |
| clap | 4.5 | MIT OR Apache-2.0 | CLI argument parsing |
| serde | 1 | MIT OR Apache-2.0 | Serialization framework |
| serde_json | 1 | MIT OR Apache-2.0 | JSON serialization |
| toml | 0.7 | MIT OR Apache-2.0 | TOML config parsing |
| reqwest | 0.12 | MIT OR Apache-2.0 | HTTP client (blocking + JSON) |
| aes-gcm | 0.10.3 | MIT OR Apache-2.0 | AES-256-GCM encryption (keystore) |
| argon2 | 0.5.3 | MIT OR Apache-2.0 | Password-based key derivation |
| rand | 0.8 | MIT OR Apache-2.0 | Cryptographic random number generation |
| base64 | 0.22 | MIT OR Apache-2.0 | Base64 encoding (keystore) |
| rpassword | 7.3 | MIT | Secure password prompt (no terminal echo) |
| zeroize | 1 | MIT OR Apache-2.0 | Secret memory zeroization on drop |

### Build Dependencies

| Crate | Version | License | Purpose |
|-------|---------|---------|---------|
| serde_json | 1 | MIT OR Apache-2.0 | Config validation in build script |
| toml | 0.7 | MIT OR Apache-2.0 | Config validation in build script |
| jsonschema | 0.17 | MIT | Config schema validation in build script |

### Dev / Test Dependencies

| Crate | Version | License | Purpose |
|-------|---------|---------|---------|
| soroban-sdk (testutils) | 21.7.0 | Apache-2.0 | Soroban test helpers |
| base64 | 0.22 | MIT OR Apache-2.0 | Test data encoding |
| ed25519-dalek | 2 | BSD-3-Clause | EdDSA signature testing |
| rand | 0.8 | MIT OR Apache-2.0 | Test randomness |
| criterion | 0.5 | MIT OR Apache-2.0 | Benchmarking |
| jsonschema | 0.17 | MIT | Schema validation tests |

## Security Vulnerability Tracking

Run `cargo audit` before each release. If a vulnerability is found:

1. Check the advisory for a patched version.
2. Update the dependency with `cargo update -p <crate> --precise <patched-version>`.
3. Re-run `cargo audit` to confirm resolution.
4. Document the advisory ID and resolution in the PR.

Known security advisories addressed: none at time of writing.

## Updating This Document

This file must be updated when:
- A new direct dependency is added or removed.
- A direct dependency's major version is bumped.
- The license policy changes (requires maintainer approval).

Run `scripts/dependency-audit.sh` to regenerate the dependency table from the current `Cargo.toml`.
