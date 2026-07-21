# Secrets

Use mise to manage sensitive environment variables securely. There are multiple supported approaches:

- **[fnox](https://github.com/jdx/fnox)** <Badge type="tip" text="recommended" /> — Full-featured secret manager with remote secret storage (e.g.: 1Password, AWS Secrets Manager) and remote encryption (e.g.: AWS KMS). This is a separate project by @jdx that works well alongside mise. There's no direct integration with mise and fnox, you set it up separately.
- [sops](/environments/secrets/sops) <Badge type="warning" text="experimental" /> — Encrypt entire files and load them via `env._.file`
- [Direct age encryption](/environments/secrets/age) <Badge type="warning" text="experimental" /> — Encrypt individual env vars inline in `mise.toml`
