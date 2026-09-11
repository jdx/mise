# Daemons

::: warning Experimental
Daemon management requires `experimental = true`. It requires a pitchfork release
with `pitchfork config add` support (planned for 2.25.0).
:::

Declare custom background processes and managed databases in one section:

```toml
[daemons]
postgres = "18"
redis = "8"

[daemons.api]
run = "npm run dev"
ready_port = 3000
auto = ["start", "stop"]

[daemons.analytics]
preset = "postgres"
version = "18"
port = 5433
```

A string selects a preset matching the entry name. A table with `run` defines a
custom process. A table with `preset` and `version` selects a preset for any instance
name, and remaining fields override its pitchfork daemon definition. For presets,
`port` is an integer; custom daemons use pitchfork's structured `port` configuration.
User-provided strings retain pitchfork template syntax; mise renders only the
embedded preset templates.

```sh
mise daemons start
mise daemons ls --json
mise daemons logs api
mise daemons status api
mise daemons restart api
mise daemons stop
mise daemons tui
```

Start and restart install missing tools. Without names, start, stop, and restart
target mise-managed daemons. Listing and status do not register configuration or
start a supervisor. The TUI opens pitchfork's dashboard.

## Database presets

| Preset     | Tool                 | Default port | Environment defaults                                       |
| ---------- | -------------------- | ------------ | ---------------------------------------------------------- |
| `postgres` | `conda:postgresql`   | 5432         | `PGHOST`, `PGPORT`, `PGUSER`, `PGDATABASE`, `DATABASE_URL` |
| `redis`    | `conda:redis-server` | 6379         | `REDIS_URL`                                                |

Both bind to loopback and require their configured ports to be free; ports do not
bump automatically. PostgreSQL uses the `postgres` user with local trust authentication.
Redis enables append-only persistence. These presets are currently Unix-only.

Use `options.database` to create a different PostgreSQL database during first
initialization. Names may contain letters, numbers, and underscores. Changing this
option later does not create another database in an existing cluster.

Explicit `[env]` values override exported defaults. When multiple instances export
the same variable, the last declaration wins; use explicit `[env]` values to choose
the instance your application uses. Explicit `[tools]` declarations
must match the version requested by the preset (for example, an installed `18.1`
can satisfy `18`). Multiple instances sharing a
tool must use the same version request.

## Data and configuration

Mise generates configuration under `$MISE_STATE_DIR/daemons/<project-hash>/` and
registers it with pitchfork. Nothing is written into the project tree. Registered
files override ordinary pitchfork definitions with the same daemon ID. Edit the
source `[daemons]` declaration, not the generated file.

Data lives in `data/<daemon-name>/` beside the generated configuration. It survives
version-request changes and daemon removal. Initialization is serialized and published
only after success. Existing data is never automatically deleted. Major-version
changes require an explicit migration or reset; incompatible data fails before startup.

To reset a database, stop its daemon, locate its data directory, and explicitly remove
that instance's data. Back up anything you want to retain first. Use the database's
own migration tools to preserve data across incompatible upgrades.

Higher-precedence declarations replace a same-name daemon completely. Inherited
daemons retain their declaring project scope. One environment profile can be active
per project: stop its daemons and leave its shell sessions before switching `MISE_ENV`.
Changed definitions take effect on the next start or explicit restart.

## Automatic start and stop

Set `auto = ["start", "stop"]` on a custom or preset table and activate mise in Bash,
Zsh, or Fish. Automatic lifecycle is opt-in; shorthand database declarations do not
automatically start. Pitchfork must already be installed—hooks never install tools.

Shell hooks dispatch reconciliation in the background, so readiness waits do not
block the prompt. Leaving for an unrelated directory releases the old project
session even when no daemons exist in the new directory. Pitchfork keeps shared
processes alive while another shell session remains in the project.

Worker diagnostics are stored under `$MISE_STATE_DIR/daemons/sessions/<shell-pid>/worker.log`.
Logs are bounded, and background workers remove Unix session state older than seven
days when its shell is gone. Session commands time out after one minute; failures
remain pending for retry at a later prompt. Disabled hooks and safe mode
prevent lifecycle commands. Re-run `mise activate` after upgrading to get shell PID
tracking in your activation script.

Project sessions also apply to native pitchfork daemons configured for automatic
lifecycle management. See [pitchfork's shell sessions](https://pitchfork.jdx.dev/guides/shell-hook.html).
