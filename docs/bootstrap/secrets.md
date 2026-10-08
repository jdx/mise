---
description: "Declare the secret values that bootstrap templates need without storing them in mise configuration."
---

# Secret inputs

`[bootstrap.secrets]` names the secret values your bootstrap needs, such as
tokens and passwords, without storing them in mise configuration. mise reads
each value from an environment variable at run time, so you can set it however
you like: with [fnox](https://fnox.jdx.dev/), a CI secret, or your shell.

This is separate from [`[env]` secrets](/environments/secrets/), which load
values into your shell environment.

## Example

Declare each input, then use it in a template with `secret()`:

```toml
[bootstrap.secrets]
cache_token = "MISE_CACHE_TOKEN"

[bootstrap.secrets.database_password]
env = "PRODUCTION_DATABASE_PASSWORD"
description = "Production database password"

[bootstrap.files."/etc/example/service.env"]
content = '''
CACHE_TOKEN={{ secret(name="cache_token") }}
DATABASE_PASSWORD={{ secret(name="database_password") }}
'''
template = true
owner = "root"
group = "root"
mode = "0600"
```

Then [supply the values](#supply-values) when you apply.

## Declare inputs

The short form maps a name to an environment variable, as in
`cache_token = "MISE_CACHE_TOKEN"`. The table form takes these keys:

| Key           | Default  | Meaning                                             |
| ------------- | -------- | --------------------------------------------------- |
| `env`         | Required | Environment variable that holds the value           |
| `description` | None     | Text mise shows when it prompts for the value       |
| `allow_empty` | `false`  | Accept an empty value, which mise otherwise rejects |

Names use ASCII letters, digits, `.`, `_`, and `-`. The environment variable
name must be a valid shell variable name.

## Use them in templates

These templates can call <code v-pre>{{ secret(name="...") }}</code>:

- [`[bootstrap.files]`](/bootstrap/files.html) content with `template = true`
- [`[dotfiles]`](/dotfiles.html) entries with `mode = "template"`

Block, line, and merge edits in `[dotfiles]` cannot. A `[dotfiles]` template
uses a secret the same way; for example, `dotfiles/credentials.tmpl` might
contain <code v-pre>token = {{ secret(name="cache_token") }}</code> for this
entry:

```toml
[dotfiles."~/.config/example/credentials"]
source = "dotfiles/credentials.tmpl"
mode = "template"
```

mise resolves only the inputs that the templates selected for a run use, so an
unused declaration does not block anything. Before a full `mise bootstrap`
changes anything, mise resolves those inputs and renders every template, so a
missing value stops the run before it writes a partial file or runs earlier
steps.

`secret()` inserts the value as it is. It does not quote or escape it for a
shell, JSON, TOML, or any other format, so the `.env` example above works only
for values that fit on one line without quoting. Encode the value for the format
the service reads, especially when it can contain quotes or newlines.

## Supply values

mise does not care where a value comes from. [fnox](https://fnox.jdx.dev/) can
inject values from a secret provider into the bootstrap process:

```sh
fnox exec -- mise bootstrap --yes
fnox exec -- mise bootstrap plan
```

A machine whose environment already has the variables does not need fnox, and
a value from a CI secret, a systemd credential, or your shell works the same
way.

For an attended run, `--prompt-secrets` asks for each missing value without
echoing it. Prompted values stay in memory for that run and are not exported:

```sh
mise bootstrap --prompt-secrets --yes
mise bootstrap files apply --prompt-secrets
mise dot apply --prompt-secrets
mise bootstrap plan --prompt-secrets
```

## Check what is available

```sh
mise bootstrap secrets status
```

```text
cache_token        MISE_CACHE_TOKEN              available
database_password  PRODUCTION_DATABASE_PASSWORD  missing
```

Each input is `available`, `missing`, `empty`, or `invalid_unicode`. Status
never prints values. Add `--json` for machine-readable output, or `--missing`
to exit 1 when any input is unavailable.

## How configs combine

When several config files declare the same name, the most local declaration
wins. `mise bootstrap status` includes the same secrets list as
`mise bootstrap secrets status`.

## Remote hosts

[`mise bootstrap remote`](/bootstrap/remote.html) does not copy your local
environment to the host, so values set on this machine are not available there.
Make the values available on the host, or use `--prompt-secrets` for an
attended run.

## What mise never prints

mise redacts resolved values from its output. Plans, dry runs, status output,
and the output of its privileged helper contain no rendered file content, and no
command reveals a bootstrap secret.

## See also

- [System files and directories](/bootstrap/files.html) for the files these
  templates write.
- [Bootstrap](/bootstrap.html#templates) for which bootstrap values are
  templates.
