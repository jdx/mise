# Secrets

Choose how to supply secret values to the project. mise passes resolved values to
commands as environment variables; the secret provider or encryption key controls
who can resolve them.

| Approach                                           | Store in the repository                               | Required at runtime                                                            |
| -------------------------------------------------- | ----------------------------------------------------- | ------------------------------------------------------------------------------ |
| [fnox](https://github.com/jdx/fnox) (recommended)  | Secret references or encrypted values managed by fnox | fnox and access to its configured providers                                    |
| [sops](./sops.html) (experimental)                 | An encrypted JSON, YAML, or TOML file                 | A decryption identity; the SOPS CLI for providers outside built-in age support |
| [Direct age encryption](./age.html) (experimental) | Individual encrypted values inside `mise.toml`        | An age or SSH decryption identity                                              |

## Use a secret manager

With fnox configured for the project and authenticated to its providers, run:

```sh
fnox exec -- mise run deploy
```

Replace `deploy` with your task. fnox resolves secrets before starting mise, so
mise templates and tasks can read them from the inherited environment. fnox
supports remote secret storage, such as 1Password and AWS Secrets Manager, and
remote encryption, such as AWS KMS. See the [fnox documentation](https://github.com/jdx/fnox)
for provider setup.

[Bootstrap secret inputs](/bootstrap/secrets.html) give provisioning templates
stable names while fnox handles providers and authentication.

## Encrypt repository files or values

Use [sops](./sops.html) when secrets belong in a separate file, or [direct age
values](./age.html) when a few encrypted variables should live beside the rest of
`mise.toml`. Commit the ciphertext and distribute decryption identities separately.

Encryption protects stored values. [Redaction](/environments/#redactions) masks
captured task output, and [CI masking](/environments/#ci-masking) protects logs
outside mise's output capture. `mise env` intentionally exports plaintext values,
including those marked as redacted.
