---
description: "Choose how to supply secret values to a project, and mask sensitive values in task output and CI logs."
---

# Secrets

Keep secret values out of plaintext config. mise can fetch them from a secret
manager through fnox, or decrypt files and values that you commit, and it masks
values you mark as sensitive in the output it captures.

| Approach                                                            | You commit                                                     | Who receives the values                                         | Needs                                                    |
| ------------------------------------------------------------------- | -------------------------------------------------------------- | --------------------------------------------------------------- | -------------------------------------------------------- |
| [fnox](/environments/secrets/fnox.html) (experimental, recommended) | `fnox.toml` with secret references or encrypted values         | Only tasks that list the key, and commands run with `--secrets` | fnox 1.39.0 or newer, and access to its providers        |
| [SOPS file](/environments/secrets/sops.html)                        | An encrypted `.env.json`, `.env.yaml` or `.env.toml`           | Everything that loads `[env]`: your shell, tasks, `mise env`    | An age identity, or the `sops` CLI for KMS, Vault or PGP |
| [age values](/environments/secrets/age.html) (experimental)         | Encrypted values in `mise.toml`                                | Everything that loads `[env]`                                   | An age identity or an SSH key                            |
| Untracked file                                                      | Nothing; `mise.local.toml` or `.env` is listed in `.gitignore` | Everything that loads `[env]`                                   | Nothing                                                  |

## Use a secret manager {#use-a-secret-manager}

mise secrets is experimental. Enable it and add fnox to the project:

```sh
mise settings experimental=true
mise use fnox
```

Then name fnox as the project's secrets source and list the keys each task
needs:

```mise-toml [mise.toml]
min_version = "2026.10.4"

[secrets.fnox]

[tasks.deploy]
secrets = ["DEPLOY_KEY", "DATABASE_URL"]
run = "./deploy.sh"
```

`mise run deploy` gives those two keys to `deploy` only and redacts them from
its output. Its dependencies, your shell, hooks and `mise env` get nothing;
programs that `deploy` starts inherit the values like any environment variable.
fnox reads secrets from providers such as 1Password, AWS Secrets Manager and AWS
KMS. See [fnox](/environments/secrets/fnox.html) for the mise side and the
[fnox documentation](https://fnox.jdx.dev) for provider setup.

To declare secrets that `mise bootstrap` writes into managed files, such as a
service's credentials, see [Bootstrap secret inputs](/bootstrap/secrets.html).

## Encrypt repository files or values {#encrypt-repository-files-or-values}

Use a [SOPS file](/environments/secrets/sops.html) when the secrets belong in a
separate file, or [age values](/environments/secrets/age.html) when a few
encrypted variables should live in `mise.toml`. Commit the ciphertext and share
decryption identities outside the repository.

SOPS and age values are ordinary `[env]` values. mise decrypts them whenever it
loads the config, so your activated shell, tasks and `mise env` see the
plaintext. Mark SOPS values with `redact = true` to keep them out of task output;
age values are redacted by default.

## Redaction and CI masking {#redaction}

Redaction replaces sensitive values with `[redacted]` in output that mise
captures. It does not encrypt anything, and the programs mise starts still
receive the real values.

### Mark values as sensitive {#mark-values-as-sensitive}

Set `redact = true` on a variable, or on a directive to mark every value it
loads:

```toml [mise.toml]
[env]
SECRET = { value = "my_secret", redact = true }
_.file = { path = ".env.json", redact = true }
```

Or list variable names, with `*` wildcards, in the top-level `redactions` array:

```toml [mise.toml]
redactions = ["SECRET_*", "*_TOKEN", "PASSWORD"]

[env]
API_TOKEN = "token_123"
```

Set `redact = false` on a variable to exempt it from `redactions` patterns,
including patterns from the global config:

```toml [mise.toml]
[env]
TEST_TOKEN = { value = "not-sensitive", redact = false }
```

`mise env --redacted` leaves out a variable that sets `redact = false`, so
[CI masking](#ci-masking) that reads that list does not mask it either.

mise also redacts a value the caller supplies for a `required` variable, or for
a `default` the caller overrides, when the entry sets `redact = true` or a
`redactions` pattern matches its name:

```mise-toml [mise.toml]
[tasks.deploy]
env = { ASC_KEY_ID = { required = true, redact = true } }
run = "./deploy.sh"
```

mise matches values, not names: once a value is marked, mise replaces that text
wherever it appears in the output it processes.

### What redaction covers {#what-redaction-covers}

mise redacts:

- the output of tasks run with `mise run`, and the command lines it prints for
  them
- its own log messages
- the value column of `mise set` (pass `--no-redact` to see the values)

mise does not redact:

- tasks with [`raw = true`](/tasks/task-configuration.html#raw) or
  [`interactive = true`](/tasks/task-configuration.html#interactive), or runs
  with `--raw` or the `raw` setting, because their output goes straight to the
  terminal. mise prints a hint when redactions are configured.
- the output of `mise exec`, because mise hands the process over to the command
- `mise env`, which prints real values on purpose (see
  [Export sensitive values](#export-redacted-values))

The [`task.output`](/configuration/settings.html#task.output) modes `replacing`
and `timed` do not print every line. In CI, use `prefix` or `interleave`, for
example `MISE_TASK_OUTPUT=prefix mise run build`, to keep complete, redacted
logs.

### Export sensitive values {#export-redacted-values}

`mise env` prints real values, including values marked as sensitive.
`--redacted` selects only the sensitive variables and still prints their values.
Use these flags to hand secrets to another program on purpose:

```sh
mise env --redacted            # only sensitive variables, as shell code
mise env --values              # only values, one per line
mise env --redacted --values   # only the values of sensitive variables
```

### CI masking {#ci-masking}

[mise-action](https://github.com/jdx/mise-action) registers GitHub Actions masks
for values marked with `redact = true` or matching `redactions`. When you resolve
secrets outside that action, register masks before running commands that might
print them.

In a custom GitHub Actions step with Bash and `jq`, export JSON so whitespace is
preserved, and escape each value the way workflow commands require:

```sh
set -o pipefail
mise env --redacted --json | jq -r '
  .[] | select(length > 0) |
  "::add-mask::" + (gsub("%"; "%25") | gsub("\r"; "%0D") | gsub("\n"; "%0A"))
'
```

This follows the
[workflow-command escaping in the Actions toolkit](https://github.com/actions/toolkit/blob/main/packages/core/src/command.ts).
Do not loop over values with word splitting, such as `for value in $(...)`,
which breaks values that contain spaces or newlines.
