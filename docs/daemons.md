---
description: Manage project daemons and persistent PostgreSQL and Redis databases with pitchfork.
---

# Daemons

::: warning Experimental
Daemon management requires `experimental = true`. It requires
[pitchfork 2.25.0](https://github.com/jdx/pitchfork/releases/tag/v2.25.0) or later
for external configuration support.
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
custom process. A table with `task` runs a mise task as the daemon. A table with `preset` and `version` selects a preset for any instance
name, and remaining fields override its pitchfork daemon definition. For presets,
`port` is an integer. Custom daemons accept the same integer shorthand or pitchfork's structured `port` configuration.
User-provided strings retain pitchfork template syntax; mise renders only the
embedded preset templates.

## Daemons that run a task

A daemon can run a mise task instead of a shell command:

```toml
[tasks."dev:core"]
run = "cargo run --bin core"

[daemons.core]
task = "dev:core"
args = ["--verbose"]
ready_port = 8080
```

`task` and `run` are mutually exclusive, and neither combines with `preset`. mise
runs the task with `mise run --skip-deps`, so the task's own dependencies and
daemons are not started again from inside the daemon; declare anything the task
needs as a separate daemon or start it before the task. `args` are passed to the
task after `--`. Readiness fields such as `ready_port` and `ready_cmd` come from
the daemon table as usual. The task must exist when daemons are registered; an
unknown name fails before anything starts.

## Setup before the process starts

`init` runs one or more setup commands before the long-running process, in the
order given:

```toml
[daemons.api]
init = ["npm ci", "npm run migrate"]
run = "exec npm start"
ready_port = 3000
```

Each step must succeed before the next one runs, and the daemon is not considered
ready until the process itself is. The steps and the long-running process share one
shell, so a step can export a variable or change directory for the ones after it.
A task-backed daemon with `init` runs that shell inside the project's tool
environment, so setup commands see mise-installed tools. **`init` must be idempotent**: it runs on every
start and restart, including automatic ones. Prefer commands that converge on the
desired state, such as `npm ci` or a migration tool, over commands that fail or
duplicate work when the state is already correct.

Anything that waits on a daemon's readiness also waits for its `init` to finish,
because readiness is only checked against the process that `init` leads to. That
applies to `mise daemons start`, to tasks that declare `daemons`, and to pitchfork
`depends` between daemons. A preset's own database initialization always runs
first, before any `init` step.

## Tasks that require daemons

A task can declare the daemons it needs:

```toml
[tasks.dev]
daemons = ["postgres", "nats"]
run = "npm run dev"
```

`mise run dev` starts those daemons, waits until pitchfork reports them ready, and
then runs the task. Use `daemons = true` to require every daemon in the project.
Already-running daemons are left alone, so repeated runs are cheap. Daemons are
part of the dependency phase: `--skip-deps` and the `task.skip_depends` setting
skip them, and `--dry-run` starts nothing. See
[task configuration](/tasks/task-configuration.html#daemons).

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
Lifecycle commands accept project daemon names; `--group` is rejected because
pitchfork groups can include daemons outside the project. Use pitchfork directly
for group operations.

## Database presets

| Preset     | Tool       | Default port | Environment defaults                                       |
| ---------- | ---------- | ------------ | ---------------------------------------------------------- |
| `postgres` | `postgres` | 5432         | `PGHOST`, `PGPORT`, `PGUSER`, `PGDATABASE`, `DATABASE_URL` |
| `redis`    | `redis`    | 6379         | `REDIS_URL`                                                |

Both bind to loopback and require their configured ports to be free; ports do not
bump automatically. PostgreSQL uses the `postgres` user with local trust authentication. Any process
that can reach its loopback port can connect without a password. These presets
are for development on a trusted local machine; use a custom daemon with
authentication for shared or untrusted environments.
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

A preset `run` override still runs after database initialization. It follows
pitchfork shell-command semantics: use `exec` for the final long-running process
(for example, `setup-command && exec server`) so it receives stop signals directly. A
`run` that needs setup first can use `init` instead of chaining commands by hand.

## Automatic start and stop

Set `auto = ["start", "stop"]` on a custom or preset table and activate mise in Bash,
Zsh, or Fish. Automatic lifecycle is opt-in; shorthand database declarations do not
automatically start. Pitchfork must already be installed—hooks never install tools.

Shell hooks register changed configuration and emit background pitchfork session
commands, so readiness waits do not block the prompt. Pitchfork owns session
liveness and automatic stopping; mise keeps no per-PID session files or workers.
Leaving for an unrelated directory releases the old project session even when no
daemons exist in the new directory. Shared processes stay alive while another
shell session remains in the project.

Failures report a diagnostic without disabling the prompt fast path. Directory or
configuration changes retry session updates; you can also run `mise hook-env --force`
through your shell's eval. A forced hook or `mise daemons start` restores a generated
configuration that was detached through pitchfork.
Disabled hooks and safe mode prevent lifecycle commands. Re-run `mise activate`
after upgrading to get shell PID tracking; old activation scripts display a hint.

Project sessions also apply to native pitchfork daemons configured for automatic
lifecycle management. See [pitchfork's shell sessions](https://pitchfork.jdx.dev/guides/shell-hook.html).
