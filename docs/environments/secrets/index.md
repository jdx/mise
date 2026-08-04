# Secrets

Use mise to manage sensitive environment variables securely. There are multiple supported approaches:

- **[fnox](https://github.com/jdx/fnox)** <Badge type="tip" text="recommended" /> — Full-featured secret manager with remote secret storage (e.g.: 1Password, AWS Secrets Manager) and remote encryption (e.g.: AWS KMS). Use `fnox exec -- mise ...` to populate mise's environment. [Bootstrap secret inputs](/bootstrap/secrets.html) give provisioning templates stable logical names while fnox remains responsible for providers and authentication.
- [sops](/environments/secrets/sops) <Badge type="warning" text="experimental" /> — Encrypt entire files and load them via `env._.file`
- [Direct age encryption](/environments/secrets/age) <Badge type="warning" text="experimental" /> — Encrypt individual env vars inline in `mise.toml`
