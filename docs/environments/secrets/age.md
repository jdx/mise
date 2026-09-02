# Direct age Encryption <Badge type="warning" text="experimental" />

Encrypt individual environment variable values directly in `mise.toml` using [age](https://github.com/FiloSottile/age) encryption. The age tool is not required—support is built into mise.

This is a simple way to store encrypted environment variables directly in `mise.toml`. Run `mise set --age-encrypt <key>=<value>` to use it. By default, mise uses your SSH key (`~/.ssh/id_ed25519` or `~/.ssh/id_rsa`) if one exists.

- **Inline storage**: values live alongside other env vars in `mise.toml`
- **Multiple recipients**: x25519 age keys and SSH recipients
- **Automatic decryption**: at runtime when identities are available

## Quick start

1. Enable experimental features:

```bash
mise settings set experimental=true
```

2. (Optional) Generate an age key if you don't want to use your SSH key:

```bash
age-keygen -o ~/.config/mise/age.txt
# Note the public key output for encryption
```

3. Encrypt a value:

```bash
mise set --age-encrypt --prompt DB_PASSWORD
# Enter value for DB_PASSWORD: [hidden input]
```

::: warning
Use `--prompt` to avoid accidentally exposing the value in your shell history. It is optional, though: you can also run `mise set --age-encrypt DB_PASSWORD="password123"`.
:::

4. Values are stored encrypted in `mise.toml` as an age directive:

```toml
[env]
DB_PASSWORD = { age = { value = "<base64>" } }
```

5. Decryption happens automatically:

```bash
mise env  # Variables are decrypted automatically
```

## CLI flags

- `--age-encrypt` — enable age encryption for the value
- `--age-recipient <KEY>` — x25519 recipient (can be set multiple times)
- `--age-ssh-recipient <PATH|KEY>` — SSH public key or path to `.pub`/private key (can be set multiple times)
- `--age-key-file <PATH>` — use recipients derived from an age identity file
- `--prompt` — prompt for the value to avoid accidentally exposing it to your shell history

If no recipients are provided explicitly, mise tries the defaults (see below).

## Storage format

Encrypted values are stored as base64 along with a `format` field:

- `format = "raw"` — uncompressed ciphertext (typically for small values)
- `format = "zstd"` — zstd-compressed ciphertext (used when ciphertext > 1KB)

## Decryption identities

mise looks for identities in this order:

1. `MISE_AGE_KEY` environment variable
   - Can contain one or more raw `AGE-SECRET-KEY-...` lines, or an age identity file payload
2. `settings.age.identity_files` (list of paths)
3. `settings.age.key_file` (single path)
4. Default `~/.config/mise/age.txt` if it exists
5. SSH identities from `settings.age.ssh_identity_files` and common defaults (`~/.ssh/id_ed25519`, `~/.ssh/id_rsa`)

Paths configured in `settings.age.key_file`, `settings.age.identity_files`, and
`settings.age.ssh_identity_files` are resolved relative to the config root of
the file that declares them. They also support Tera templates, including
<span v-pre>`{{ config_root }}`</span> and values from `env`. Absolute paths and paths beginning
with `~` keep their existing meaning.

Decrypted values are always marked as redacted.

Age decryption is strict by default. If no identities are found, no available identity can decrypt the value, or the age payload is invalid, mise fails instead of continuing with a partially resolved environment.

To allow commands and tasks to continue when an age value cannot be decrypted, disable strict mode:

```bash
mise settings set age.strict=false
```

In non-strict mode, mise skips values that cannot be decrypted and continues resolving the rest of the environment.

## Defaults for recipients (encryption)

When `--age-encrypt` is used without explicit recipients, mise attempts to derive recipients from:

- The public keys corresponding to identities in the default key file `~/.config/mise/age.txt`
- Public keys inferred from SSH private keys if a corresponding `.pub` file exists

If none are found, the command fails with an error asking you to provide recipients or configure `settings.age.key_file`.

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="age" :level="2" />

## Notes

- This feature is experimental; flags and behavior may change.
- `mise set KEY` prints the decrypted value.
