# sops <Badge type="warning" text="experimental" />

mise reads encrypted secret files and makes values available as environment variables via `env._.file`.

- **Formats**: `.env.json`, `.env.yaml`, `.env.toml`
- **Encryption**: [sops](https://getsops.io), using the built-in age support or the external `sops` CLI

<span id="example"></span>

## Choose a decryption method

The default built-in implementation handles age-encrypted files. To use AWS KMS,
GCP KMS, Azure Key Vault, Vault, or PGP, install the SOPS CLI, authenticate to the
provider, and set `sops.rops = false` as described below.

## Encrypt with sops

::: info
The default `sops.rops = true` implementation supports age-encrypted files. Set
`sops.rops = false` to use the external `sops` CLI for other key services and
methods supported by SOPS, such as AWS KMS, GCP KMS, Azure Key Vault, Vault,
and PGP.
:::

::: warning
The external `sops` CLI does not currently support TOML input/output. mise can decrypt SOPS-encrypted `.env.toml` files only with the default `sops.rops = true` setting. If you set `sops.rops = false`, mise shells out to the `sops` CLI and encrypted TOML env files fail with a configuration error. Use `.env.json` or `.env.yaml` when you need the external CLI path.
:::

1. Install tools and enable experimental features:

```sh
mise use -g sops age
mise settings set experimental=true
```

2. Reuse an existing age identity, or create one if the file does not exist:

```sh
mkdir -p ~/.config/mise
mise exec -- age-keygen -o ~/.config/mise/age.txt
# Public key: <public key>
```

3. Create `.env.json` with your values. This example uses a placeholder:

```json [.env.json]
{
  "API_TOKEN": "replace-with-your-token"
}
```

Encrypt it with the public key printed by `age-keygen`:

```sh
mise exec -- sops encrypt -i --age "<public key>" .env.json
```

::: tip
The `-i` flag replaces the plaintext file with ciphertext. Commit the encrypted
file, and keep `age.txt` outside the repository. The external SOPS CLI reads
`SOPS_AGE_KEY_FILE`; `MISE_SOPS_AGE_KEY_FILE` configures mise only. To edit the file:

```sh
SOPS_AGE_KEY_FILE="$HOME/.config/mise/age.txt" mise exec -- sops .env.json
```

:::

Age key files use the standard SOPS/age format: put one identity on each line.
Blank lines and lines beginning with `#` are ignored, and all identities are
tried when decrypting.

4. Reference it in config:

```toml
[env]
_.file = { path = ".env.json", redact = true }
```

mise now decrypts the file for `mise exec`, tasks, and shell activation.
`mise env` prints the plaintext values; `redact = true` does not hide that export.

## Environment Variables

mise supports both mise-specific environment variables and standard SOPS ones:

**mise-specific variables (highest priority):**

- `MISE_SOPS_AGE_KEY` - Age private key content directly
- `MISE_SOPS_AGE_KEY_FILE` - Path to age private key file

**Standard SOPS variables (fallback):**

- `SOPS_AGE_KEY_FILE` - Path to age private key file
- `SOPS_AGE_KEY` - Age private key content directly

**Precedence order:**

1. `MISE_SOPS_AGE_KEY` (mise setting or env var, checked first)
2. `MISE_SOPS_AGE_KEY_FILE` or `sops.age_key_file` (mise setting or env var)
3. `SOPS_AGE_KEY_FILE` (standard)
4. `SOPS_AGE_KEY` (standard, direct key content)
5. Default: `~/.config/mise/age.txt`

This allows you to override SOPS settings specifically for mise while keeping your standard SOPS configuration intact for other tools.

## Redaction

Mark secrets from files as sensitive:

```toml
[env]
_.file = { path = ".env.json", redact = true }
```

Redaction applies to captured task output. `mise env --redacted` deliberately
exports the matching secrets; it does not mask them. See [redactions](/environments/#redactions)
for output-mode limitations.

### CI masking (GitHub Actions)

See [CI masking](/environments/#ci-masking) for mise-action integration and a
manual masking example that preserves whitespace and multiline values.

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="sops" :level="2" />
