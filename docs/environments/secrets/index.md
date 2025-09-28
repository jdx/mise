# Secrets

Use mise to manage sensitive environment variables securely. There are two supported approaches:

- [sops](/environments/secrets/sops) <Badge type="warning" text="experimental" /> — Encrypt entire files and load them via `env._.file`
- [Direct age encryption](/environments/secrets/age) <Badge type="warning" text="experimental" /> — Encrypt individual env vars inline in `mise.toml`

Both methods integrate with `mise env` and redactions. Pick sops for whole-file workflows; use direct age for per-variable encryption stored directly in `mise.toml`.
