---
description: "Choose how to supply secret values to the project."
---

# Secrets

Choose how to supply secret values to the project. mise passes resolved values to
commands as environment variables; the secret provider or encryption key controls
who can resolve them.

| Approach                                           | Store in the repository                               | Required at runtime                                                            |
| -------------------------------------------------- | ----------------------------------------------------- | ------------------------------------------------------------------------------ |
| [mise secrets (fnox)](./fnox.html), recommended    | Secret references or encrypted values managed by fnox | fnox and access to its configured providers                                    |
| [sops](./sops.html) (experimental)                 | An encrypted JSON, YAML, or TOML file                 | A decryption identity; the SOPS CLI for providers outside built-in age support |
| [Direct age encryption](./age.html) (experimental) | Individual encrypted values inside `mise.toml`        | An age or SSH decryption identity                                              |

## Use a secret manager

Name fnox as the project's secrets source and list the keys each task needs:

```toml [mise.toml]
[secrets.fnox]

[tasks.deploy]
secrets = ["DEPLOY_KEY", "DATABASE_URL"]
run = "./deploy.sh"
```

`mise run deploy` gives `deploy`, and only `deploy`, those two keys, and redacts them from its
output. Dependencies, hooks, shims and `mise env` get nothing. See
[mise secrets with fnox](./fnox.html). fnox supports remote secret storage, such as 1Password and
AWS Secrets Manager, and remote encryption, such as AWS KMS; see the
[fnox documentation](https://github.com/jdx/fnox) for provider setup.

[Bootstrap secret inputs](/bootstrap/secrets.html) give provisioning templates
stable names while fnox handles providers and authentication.

## Encrypt repository files or values

Use [sops](./sops.html) when secrets belong in a separate file, or [direct age
values](./age.html) when a few encrypted variables should live beside the rest of
`mise.toml`. Commit the ciphertext and distribute decryption identities separately.

sops and age values are env values: decrypted at load and visible to the shell, `mise env` and
hook-env. mise secrets reach only the processes mise starts that were granted them.

Encryption protects stored values. [Redaction](/environments/#redactions) masks
captured task output, and [CI masking](/environments/#ci-masking) protects logs
outside mise's output capture. `mise env` intentionally exports plaintext values,
including those marked as redacted.
