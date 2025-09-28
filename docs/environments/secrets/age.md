# Direct age Encryption <Badge type="warning" text="experimental" />

Encrypt individual environment variable values directly in `mise.toml` using [age](https://github.com/FiloSottile/age).

- **Inline storage**: values live alongside other env vars in `mise.toml`
- **Multiple recipients**: x25519 age keys and SSH recipients
- **Automatic decryption**: at runtime when identities are available

## Quick start

1. Generate an age key (if needed):

```bash
age-keygen -o ~/.config/mise/age.txt
# Note the public key output for encryption
```

2. Encrypt a value:

```bash
# Using x25519 age key
mise set SECRET_API_KEY="my-secret-value" --age-encrypt \
  --age-recipient age1ql3z7hjy54pw3hyww5ayyfg7zqgvc7w3j2elw8zmrj2kg5sfn9aqmcac8p

# Or using SSH key
mise set DB_PASSWORD="password123" --age-encrypt \
  --age-ssh-recipient ~/.ssh/id_ed25519.pub
```

3. Values are stored encrypted in `mise.toml` as an age directive:

```toml
[env]
SECRET_API_KEY = { age = { value = "<base64>", format = "raw" } }
DB_PASSWORD    = { age = { value = "<base64>", format = "zstd" } }
```

4. Decryption happens automatically:

```bash
# Using env var with raw private key or identity file content
export MISE_AGE_KEY="AGE-SECRET-KEY-1..."
# or: export MISE_AGE_KEY="$(cat ~/.config/mise/age.txt)"

mise env  # Variables are decrypted automatically
```

## CLI flags

- `--age-encrypt` — enable age encryption for the value
- `--age-recipient <KEY>` — x25519 recipient (can be set multiple times)
- `--age-ssh-recipient <PATH|KEY>` — SSH public key or path to `.pub`/private key (can be set multiple times)
- `--age-key-file <PATH>` — use recipients derived from an age identity file
- `--prompt` — prompt for the value (input is hidden when encrypting)

If no recipients are provided explicitly, mise will try defaults (see below).

## Storage format

Encrypted values are stored as base64 along with a `format` field:

- `format = "raw"` — uncompressed ciphertext (typically for small values)
- `format = "zstd"` — zstd-compressed ciphertext (used when ciphertext > 1KB)

Legacy string values with prefixes `age64:v1:` and `age64:zstd:v1:` are also supported for decryption.

## Decryption identities

mise looks for identities in this order:

1. `MISE_AGE_KEY` environment variable
   - Can contain one or more raw `AGE-SECRET-KEY-...` lines, or an age identity file payload
2. `settings.age.identity_files` (list of paths)
3. `settings.age.key_file` (single path)
4. Default `~/.config/mise/age.txt` if it exists
5. SSH identities from `settings.age.ssh_identity_files` and common defaults (`~/.ssh/id_ed25519`, `~/.ssh/id_rsa`)

Decrypted values are always marked as redacted.

If no identities are found or decryption fails, mise returns the encrypted value as-is (non-strict behavior).

## Defaults for recipients (encryption)

When `--age-encrypt` is used without explicit recipients, mise attempts to derive recipients from:

- The public keys corresponding to identities in the default key file `~/.config/mise/age.txt`
- Public keys inferred from SSH private keys if a corresponding `.pub` file exists

If none are found, the command fails with an error asking you to provide recipients or configure `settings.age.key_file`.

## Settings

```toml
[settings.age]
# Path to age identity file used for both encryption (to derive recipient) and decryption
key_file = "~/.config/mise/age.txt"

# Additional identity files to try for decryption
identity_files = ["~/.config/mise/age.txt", "~/.age/keys.txt"]

# SSH identity files to try for decryption
ssh_identity_files = ["~/.ssh/id_ed25519", "~/.ssh/id_rsa"]
```

## Notes

- Feature is experimental; flags and behavior may change.
- `mise set KEY` will print the decrypted value when possible; if decryption fails, it prints the encrypted value instead.
