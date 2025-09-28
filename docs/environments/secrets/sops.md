# sops <Badge type="warning" text="experimental" />

mise reads encrypted secret files and makes values available as environment variables via `env._.file`.

- **Formats**: `.env.json`, `.env.yaml`, `.env.toml`
- **Encryption**: [sops](https://getsops.io) backed by [age](https://github.com/FiloSottile/age)

## Example

```json
{
  "AWS_ACCESS_KEY_ID": "AKIAIOSFODNN7EXAMPLE",
  "AWS_SECRET_ACCESS_KEY": "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"
}
```

```toml [mise.toml]
[env]
_.file = ".env.json"
```

mise will automatically decrypt the file if it is sops-encrypted.

## Encrypt with sops

:::: info
Currently age is the only sops encryption method supported.
::::

1. Install tools: `mise use -g sops age`

2. Generate an age key and note the public key:

```sh
age-keygen -o ~/.config/mise/age.txt
# Public key: <public key>
```

3. Encrypt the file:

```sh
sops encrypt -i --age "<public key>" .env.json
```

:::: tip
The `-i` overwrites the file. The encrypted file is safe to commit. Set `SOPS_AGE_KEY_FILE=~/.config/mise/age.txt` to decrypt/edit with sops.
::::

4. Reference it in config:

```toml
[env]
_.file = ".env.json"
```

Now `mise env` exposes the values.

## Redaction

Mark secrets from files as sensitive:

```toml
[env]
_.file = { path = ".env.json", redact = true }
```

Work with redacted values:

```bash
mise env --redacted
mise env --redacted --values
```

### CI masking (GitHub Actions)

```yaml
- name: Mask secrets
  run: |
    for value in $(mise env --redacted --values); do
      echo "::add-mask::$value"
    done
- name: Use secrets safely
  run: |
    mise exec -- ./deploy.sh
```

If you use [mise-action](https://github.com/jdx/mise-action), values marked `redact = true` are masked automatically.

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="sops" :level="2" />
