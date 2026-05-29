# Secret File Encryption

This guide explains how to protect the persisted secret files used by
AnchorKit — keypair files, the `.anchorkit/deployments.json` record, and any
config files that contain sensitive values — using standard, widely-available
tools.

---

## Files that may contain sensitive data

| File | Sensitive content | Notes |
|------|-------------------|-------|
| `keypair.json` / plain-text keypair | Stellar secret key (`S...`) | Created by the operator; passed via `--keypair-file` |
| `~/.config/stellar/identity/<name>.toml` | Stellar secret key | Managed by the Stellar CLI keystore |
| `.anchorkit/deployments.json` | Contract IDs, network names, timestamps | No secret keys; low sensitivity but should be access-controlled |
| `configs/*.json` / `configs/*.toml` | Endpoint URLs, contact emails | No secret keys by default; treat as internal |

> **Rule of thumb:** any file whose loss or disclosure would allow an attacker
> to sign Stellar transactions on your behalf must be encrypted at rest.

---

## Option 1 — Use the Stellar CLI keystore (recommended)

The Stellar CLI ships with a built-in, passphrase-protected keystore.  This is
the safest option because the secret key never touches the filesystem in
plaintext.

```bash
# Add a key under the alias "anchor-admin"
stellar keys add anchor-admin --secret-key

# Use the alias everywhere instead of a raw key
export ANCHOR_ADMIN_SECRET=""          # leave empty
anchorkit deploy --source anchor-admin --network mainnet
```

The keystore stores keys in `~/.config/stellar/identity/` encrypted with your
passphrase.  The passphrase is prompted interactively and is never written to
disk.

---

## Option 2 — GPG symmetric encryption

Suitable for CI/CD pipelines where interactive passphrases are not practical.

### Encrypt a keypair file

```bash
# Encrypt (prompts for a passphrase)
gpg --symmetric --cipher-algo AES256 --output keypair.json.gpg keypair.json

# Securely delete the plaintext original
shred -u keypair.json          # Linux
# macOS: rm -P keypair.json
```

### Decrypt at runtime

```bash
# Decrypt to a temporary file (ensure the temp dir is on a RAM-backed fs)
gpg --decrypt --output /dev/shm/keypair.json keypair.json.gpg

anchorkit register \
  --keypair-file /dev/shm/keypair.json \
  --address G... \
  --services deposits,withdrawals \
  --contract-id C... \
  --sep10-token <TOKEN> \
  --sep10-issuer <ISSUER>

# Wipe the decrypted file immediately after use
shred -u /dev/shm/keypair.json
```

### Store the passphrase in a secrets manager

Never hard-code the GPG passphrase.  Use one of:

- **AWS Secrets Manager**: `aws secretsmanager get-secret-value --secret-id anchor/gpg-passphrase`
- **HashiCorp Vault**: `vault kv get -field=passphrase secret/anchor/gpg`
- **GitHub Actions**: `${{ secrets.GPG_PASSPHRASE }}`

Pass it to GPG non-interactively:

```bash
echo "$GPG_PASSPHRASE" | gpg --batch --passphrase-fd 0 \
  --decrypt --output /dev/shm/keypair.json keypair.json.gpg
```

---

## Option 3 — age encryption

[age](https://github.com/FiloSottile/age) is a modern, simple encryption tool
with no configuration files.

```bash
# Generate a recipient key pair (do this once, store the private key safely)
age-keygen -o age-key.txt
# Public key is printed to stdout, e.g.: age1ql3z7hjy54pw3hyww5ayyfg7zqgvc7w3j2elw8zmrj2kg5sfn9aqmcac8p

# Encrypt
age --recipient age1ql3z7hjy54pw3hyww5ayyfg7zqgvc7w3j2elw8zmrj2kg5sfn9aqmcac8p \
    --output keypair.json.age keypair.json

# Decrypt
age --decrypt --identity age-key.txt --output keypair.json keypair.json.age
```

---

## Option 4 — Full-disk or filesystem-level encryption

For production servers, encrypt the entire volume or the directory that holds
secrets:

- **Linux LUKS**: `cryptsetup luksFormat /dev/sdX`
- **macOS FileVault**: System Preferences → Security & Privacy → FileVault
- **AWS EBS**: Enable encryption at volume creation time
- **Kubernetes**: Use encrypted etcd and Kubernetes Secrets with a KMS provider

This protects all files on the volume, including `.anchorkit/` and any keypair
files, without requiring per-file tooling.

---

## Protecting `.anchorkit/deployments.json`

`deployments.json` does not contain secret keys, but it does record contract
IDs and network names.  Restrict access with filesystem permissions:

```bash
chmod 600 .anchorkit/deployments.json
chmod 700 .anchorkit/
```

If you store this file in version control, ensure the repository is private and
consider encrypting it with `git-crypt` or `sops`.

---

## Environment variable hygiene

Prefer environment variables over `--secret-key` flags because flags appear in
the process list (`ps aux`) and shell history.

```bash
# Set the secret in the current shell only (not exported to child processes
# beyond what is needed)
export ANCHOR_ADMIN_SECRET="$(gpg --decrypt keypair.json.gpg)"

# Run the command
anchorkit attest --subject G... --payload-hash <HASH> --contract-id C...

# Unset immediately after use
unset ANCHOR_ADMIN_SECRET
```

On Linux, you can also use a RAM-backed tmpfs for any intermediate files:

```bash
mount -t tmpfs -o size=1m tmpfs /mnt/secrets
```

---

## CI/CD checklist

- [ ] Secret keys are stored in the CI secrets store (GitHub Actions Secrets,
      GitLab CI Variables, AWS Secrets Manager, etc.), never in the repository.
- [ ] Keypair files are encrypted at rest and decrypted only for the duration
      of the job.
- [ ] Decrypted files are written to a RAM-backed path (`/dev/shm`, tmpfs) and
      deleted immediately after use.
- [ ] `ANCHOR_ADMIN_SECRET` is masked in CI logs (most platforms do this
      automatically for secrets).
- [ ] `--secret-key` flag is **not** used in CI scripts; use the environment
      variable or `--keypair-file` instead.
- [ ] `.anchorkit/deployments.json` is not committed to version control, or is
      encrypted with `git-crypt` / `sops` if it must be.

---

## Further reading

- [Stellar CLI key management](https://developers.stellar.org/docs/tools/developer-tools/cli/stellar-cli)
- [age encryption tool](https://github.com/FiloSottile/age)
- [Mozilla SOPS](https://github.com/mozilla/sops) — secrets management for config files
- [git-crypt](https://github.com/AGWA/git-crypt) — transparent encryption for Git repositories
