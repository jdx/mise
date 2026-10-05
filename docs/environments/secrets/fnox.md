# mise secrets with fnox

<Badge type="warning" text="experimental" />

[fnox](https://fnox.jdx.dev) can be the secrets source for a project. mise reads the list of
keys fnox knows about and shows it with `mise secrets ls`. Values are never shown, and nothing
receives secrets yet.

mise secrets is experimental. Enable it with:

```sh
mise settings experimental=true
```

## Quick start

Add fnox to the project and name it as the secrets source:

```toml [mise.toml]
[tools]
fnox = "latest"

[secrets.fnox]          # this project's secrets come from fnox, discovered from this directory
profile = "dev"         # optional; passed as fnox -P
```

```sh
mise use fnox
mise secrets ls
```

```
fnox · profile dev · ~/src/app (mise.toml) · fnox 1.39.0
KEY                ENV    FILE  DESCRIPTION
AWS_ACCESS_KEY_ID  -      no    (lease aws)
DATABASE_URL       true   no    app database
DEPLOY_KEY         exec   no
GCP_SA_JSON        exec   yes
SIGNING_KEY        false  no    release signing key
```

The header goes to stderr and the table to stdout. mise needs fnox 1.39.0 or newer.

## Where the source is declared

`[secrets.fnox]` is read from project `mise.toml` files only.

- The nearest file wins per field. `mise.local.toml` and `mise.<env>.toml` can override
  `profile`. fnox runs in the directory of the nearest file that declares `[secrets.fnox]`, and
  finds its `fnox.toml` from there.
- Global config, system config, and files in or above your home directory are ignored, because
  they apply to every project. `mise secrets ls` and `mise doctor` name any ignored file.
- The file must be [trusted](/cli/trust.html), as with the rest of project config.
- Safe mode (`MISE_SAFE=1`) refuses to use secrets sources.

## `mise secrets ls`

| Column      | Meaning                                                              |
| ----------- | -------------------------------------------------------------------- |
| KEY         | The name of the secret or lease key                                  |
| ENV         | fnox's `env` setting: `true`, `exec`, `false`, or `-` for lease keys |
| FILE        | `yes` if fnox provides the value as a file                           |
| DESCRIPTION | fnox's description, or `(lease <name>)`                              |

`-J`/`--json` prints the same information as JSON:

```json
{
  "source": {
    "kind": "fnox",
    "root": "/home/me/src/app",
    "declared_in": ["/home/me/src/app/mise.toml"],
    "profile": "dev",
    "tool": {
      "path": "/home/me/.local/share/mise/installs/fnox/1.39.0/fnox",
      "version": "1.39.0"
    }
  },
  "keys": [
    {
      "key": "DEPLOY_KEY",
      "kind": "secret",
      "env": "exec",
      "as_file": false,
      "lease": null,
      "description": null
    }
  ],
  "dynamic_leases": [],
  "ignored": []
}
```

`env` is `true`, `"exec"`, `false`, or `null` for lease keys.

mise looks for the fnox CLI in the project's tools first, then on `PATH`.

## Migrating from mise-env-fnox

Remove `_.fnox-env` and the `mise-env-fnox` plugin from `mise.toml`, then add `[secrets.fnox]`.
`mise doctor` warns while the plugin is still configured.
