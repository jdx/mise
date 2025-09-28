# Secrets <Badge type="warning" text="experimental" />

Because env vars in mise.toml can store sensitive information, mise has built-in support for reading
encrypted secrets from files. Currently, this is done with a [sops](https://getsops.io) implementation
however other secret backends could be added in the future.

Secrets are `.env.(json|yaml|toml)` files with a simple structure, for example:

```json
{
  "AWS_ACCESS_KEY_ID": "AKIAIOSFODNN7EXAMPLE",
  "AWS_SECRET_ACCESS_KEY": "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"
}
```

Env vars from this can be imported into a mise config with the following:

```toml
[env]
_.file = ".env.json"
```

mise will automatically use a secret backend like sops if the file is encrypted.

## sops

mise uses the rust [rops](https://github.com/gibbz00/rops) library to interact with [sops](https://getsops.io) files.
If you encrypt a sops file, mise will automatically decrypt it when reading the file. sops files can
be in json, yaml, or toml format—however if you want to use toml you'll need to use the rops cli instead
of sops. Otherwise, either sops or rops will work fine.

::: info
Currently age is the only sops encryption method supported.
:::

In order to encrypt a file with sops, you'll first need to install it (`mise use -g sops`). You'll
also need to install [age](https://github.com/FiloSottile/age) (`mise use -g age`) to generate a keypair for sops to use
if you have not already done so.

To generate a keypair with age run the following and note the public key that is output to use
in the next command to `sops`:

```sh
$ age-keygen -o ~/.config/mise/age.txt
Public key: <public key>
```

Assuming we have a `.env.json` file like at the top of this doc, we can now encrypt it with sops:

```sh
sops encrypt -i --age "<public key>" .env.json
```

::: tip
The `-i` here overwrites the file with an encrypted version. This encrypted version is safe to commit
into your repo as without the private key (`~/.config/mise/age.txt` in this case) the file is useless.

You can later decrypt the file with `sops decrypt -i .env.json` or edit it in EDITOR with `sops edit .env.json`.
However, you'll first need to set SOPS_AGE_KEY_FILE to `~/.config/mise/age.txt` to decrypt the file.
:::

Lastly, we need to add the file to our mise config which can be done with `mise set _.file=.env.json`.

Now when you run `mise env` you should see the env vars from the file:

```sh
$ mise env
export AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE
export AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY
```

## Working with Encrypted Secrets

Since encrypted files typically contain sensitive information, you should mark them as redacted:

```toml
[env]
_.file = { path = ".env.json", redact = true }
```

This will mark all environment variables from that file as sensitive. You can then use the `mise env` flags to work with these secrets:

```bash
# View only the redacted/sensitive variables
mise env --redacted

# Get just the values (useful for scripts)
mise env --redacted --values
```

:::danger
mise, or other tools, may log secrets in CI systems. You'll want to configure your CI system to redact/mask these values.

### GitHub Actions Integration

When using secrets in GitHub Actions, you must mask them to prevent exposure in logs:

```yaml
- name: Mask secrets
  run: |
    for value in $(mise env --redacted --values); do
      echo "::add-mask::$value"
    done

- name: Use secrets safely
  run: |
    # Now the secrets are masked in the logs
    mise exec -- ./deploy.sh
```

If you're using [mise-action](https://github.com/jdx/mise-action), it automatically handles masking for variables marked with `redact = true`.
:::

### `sops` Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="sops" :level="4" />

## Direct age Encryption <Badge type="warning" text="experimental" />

In addition to sops, mise provides experimental built-in support for encrypting individual environment variables directly using [age](https://github.com/FiloSottile/age) encryption. This allows you to encrypt sensitive values right in your `mise.toml` file without needing external encrypted files.

### Quick Start

1. **Generate an age key pair** (if you haven't already):

```bash
age-keygen -o ~/.config/mise/age.txt
# Note the public key output for encryption
```

2. **Encrypt a value**:

```bash
# Using x25519 age key
mise set SECRET_API_KEY="my-secret-value" --age-encrypt \
  --age-recipient age1ql3z7hjy54pw3hyww5ayyfg7zqgvc7w3j2elw8zmrj2kg5sfn9aqmcac8p

# Or using SSH key
mise set DB_PASSWORD="password123" --age-encrypt \
  --age-ssh-recipient ~/.ssh/id_ed25519.pub
```

3. **Values are stored encrypted** in `mise.toml`:

```toml
[env]
SECRET_API_KEY = "age64:v1:YWdlLWVuY3J5cHRpb24ub3JnL3YxCi0+IFgyNTUxOSB3d0..."
DB_PASSWORD = "age64:v1:YWdlLWVuY3J5cHRpb24ub3JnL3YxCi0+IHNzaC1lZDI1NTE5..."
```

4. **Automatic decryption** at runtime:

```bash
# Decryption happens automatically when the identity is available
export MISE_AGE_KEY="AGE-SECRET-KEY-1..."  # Or use ~/.config/mise/age.txt
mise env  # Variables are decrypted automatically
```

### Encryption Options

The `mise set` command supports several age-related flags:

- `--age-encrypt` - Enable age encryption for the value
- `--age-recipient <KEY>` - Specify age x25519 recipient (can be used multiple times)
- `--age-ssh-recipient <PATH|KEY>` - Specify SSH recipient as file path or public key (can be used multiple times)
- `--age-key-file <PATH>` - Use recipients from an age identity file

### Storage Format

Encrypted values are stored with a prefix indicating the encryption method:

- `age64:v1:` - Uncompressed encrypted values (< 1KB)
- `age64:zstd:v1:` - Compressed encrypted values (≥ 1KB, using zstd compression)

### Decryption

mise automatically decrypts values when:

1. **Environment variable**: `MISE_AGE_KEY` contains the secret key
2. **Identity file**: `~/.config/mise/age.txt` exists (default location)
3. **SSH keys**: Standard SSH private keys (`~/.ssh/id_ed25519`, `~/.ssh/id_rsa`)

Decrypted values are automatically marked as redacted for security.

### Configuration

You can configure age encryption behavior in settings:

```toml
[settings.age]
# Path to age identity file (default: ~/.config/mise/age.txt)
key_file = "~/.config/mise/age.txt"

# Additional age identity files for decryption
identity_files = ["~/.config/mise/age.txt", "~/.age/keys.txt"]

# SSH identity files for decryption
ssh_identity_files = ["~/.ssh/id_ed25519", "~/.ssh/id_rsa"]

# Strict mode - fail if decryption fails (default: false)
# In non-strict mode, encrypted values are returned as-is if decryption fails
strict = false
```

### Use Cases

Direct age encryption is useful for:

- **API keys and tokens**: Encrypt sensitive credentials directly in config
- **Database passwords**: Keep connection strings secure
- **Small secrets**: Avoid separate encrypted files for individual values
- **Team sharing**: Encrypt with multiple recipients for team access

### Security Considerations

- Encrypted values are safe to commit to version control
- Always use `.gitignore` for identity files (`age.txt`, SSH private keys)
- In CI/CD, provide identities via secure environment variables
- Decrypted values are automatically redacted in logs when possible

### Example: Team Collaboration

```bash
# Team members generate their age keys
age-keygen -o ~/.config/mise/age.txt

# Encrypt secrets for multiple team members
mise set API_KEY="secret" --age-encrypt \
  --age-recipient age1alice... \
  --age-recipient age1bob... \
  --age-recipient age1carol...

# Each team member can decrypt with their own key
export MISE_AGE_KEY="AGE-SECRET-KEY-1..."
mise env  # Decrypts automatically
```

### Differences from sops

| Feature            | sops                        | Direct age Encryption     |
| ------------------ | --------------------------- | ------------------------- |
| Encryption scope   | Entire file                 | Individual values         |
| File formats       | JSON, YAML, TOML            | N/A (stored in mise.toml) |
| Encryption methods | age, PGP, KMS, etc.         | age only                  |
| Key rotation       | Requires re-encrypting file | Per-value re-encryption   |
| Storage            | Separate encrypted files    | Inline in mise.toml       |

Choose sops for encrypting entire configuration files with multiple secrets. Use direct age encryption for individual sensitive values that need to be mixed with non-sensitive configuration.
