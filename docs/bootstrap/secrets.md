# Bootstrap secret inputs

`[bootstrap.secrets]` declares the sensitive inputs a bootstrap configuration
needs without storing their values in mise configuration. Values come from the
environment, making secret managers such as [fnox](https://fnox.jdx.dev/) the
provider boundary rather than adding provider-specific credentials to mise.

```toml
[bootstrap.secrets]
cache_token = "MISE_CACHE_TOKEN"
database_password = {
  env = "PRODUCTION_DATABASE_PASSWORD",
  description = "Production database password",
}

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

The short declaration maps a logical name directly to an environment variable.
The table form also accepts `description` and `allow_empty = true`; empty values
are rejected by default. Mise resolves only the inputs referenced by selected
file templates; an unused declaration does not block unrelated files. Referenced
inputs are resolved and every template is rendered before any full-bootstrap
mutation starts, so a missing input cannot leave a partially rendered file or
allow earlier bootstrap steps to run.

Use fnox to inject provider-backed values into the bootstrap process:

```sh
fnox exec -- mise bootstrap --yes
fnox exec -- mise bootstrap plan
```

This is intentionally a loose integration. The machine running mise does not
need fnox when its environment has already been populated, and mise does not
know whether a value came from fnox, a CI secret, `systemd`, or a shell.

For an attended one-off run, `--prompt-secrets` securely prompts for missing
values. Prompted values remain in memory and are not exported:

```sh
mise bootstrap --prompt-secrets --yes
mise bootstrap files apply --prompt-secrets
mise bootstrap plan --prompt-secrets
```

`mise bootstrap secrets status` reports logical names, environment variable
names, and `available`, `missing`, `empty`, or `invalid_unicode`; it never prints
values. Add `--json` for machine-readable output or `--missing` to exit 1 when
an input is unavailable.

Mise redacts resolved values from its output. Plans, dry runs, status output,
and privileged-helper output contain no rendered file content. There is no
command to reveal a bootstrap secret.
