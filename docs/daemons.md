---
description: Manage project daemons and persistent PostgreSQL and Redis databases with pitchfork.
---

# Daemons

::: warning Experimental
Daemon management requires `experimental = true`. It requires
[pitchfork 2.25.0](https://github.com/jdx/pitchfork/releases/tag/v2.25.0) or later
for external configuration support.
:::

Use daemons for processes that keep running between task invocations, such as a
database, message broker, or development server. Declare them in `mise.toml`;
mise provides the project configuration and tool environment, while
[pitchfork](https://pitchfork.jdx.dev/) manages the processes and readiness checks.

## Quick start

This example gives a Node.js project a persistent PostgreSQL database. Add it to
the project's `mise.toml`:

```toml
[settings]
experimental = true

[tools]
node = "24"

[daemons]
postgres = "18"

[tasks.dev]
daemons = "postgres"
run = "npm run dev"
```

Run `mise run dev` to install missing tools, start PostgreSQL, wait for it to be
ready, and then run your application's `dev` script. The preset supplies connection
variables, including `DATABASE_URL`, and keeps database data between runs.

PostgreSQL stays running when the task exits. Later invocations reuse it.
Run `mise daemons stop postgres` when you no longer need it, or configure
[automatic start and stop](#automatic-start-and-stop) for shell sessions.

## Declare a daemon

Choose a declaration based on what you want to run:

| Declaration                                | Use                                                      |
| ------------------------------------------ | -------------------------------------------------------- |
| `postgres = "18"` under `[daemons]`        | A built-in database preset whose name matches the entry. |
| `preset = "postgres"` and `version = "18"` | A named instance of a preset, with optional overrides.   |
| `run = "exec npm run dev"`                 | A custom shell command.                                  |
| `task = "dev:core"`                        | An existing mise task, with optional `args`.             |

For example, declare a development server and a second PostgreSQL instance:

```toml
[daemons.api]
run = "exec npm run dev"
ready_port = 3000

[daemons.analytics]
preset = "postgres"
version = "18"
port = 5433
```

Custom commands use the project's mise tool environment by default. Declare their
tools in `[tools]`; presets add their own required tools. Use `exec` for the final
long-running command so it receives stop signals directly.

Fields such as `ready_port`, `ready_cmd`, and `auto` configure pitchfork's daemon
behavior. Set a readiness check that reflects when your service can accept work;
the example above waits for port 3000. For presets, `port` is an integer.
Custom daemons also accept pitchfork's structured `port` configuration.
User-provided strings retain pitchfork template syntax; mise renders only the
embedded preset templates.

## Tasks that require daemons

Add `daemons` to a task to start its services before any task body runs:

```toml
[daemons]
postgres = "18"
redis = "8"

[tasks.test]
daemons = ["postgres", "redis"]
run = "npm test"
```

`mise run test` starts the requested daemons and waits for pitchfork to report them
ready. Already-running daemons are reused. This replaces prerequisite tasks that
launch background processes and poll for readiness.

Use a string for one daemon, a list for several, or `true` for every daemon in the
task's project configuration. Each name must match a `[daemons]` entry.
In a monorepo, each task resolves daemon names in its own project's configuration
hierarchy, including inherited declarations.

Daemon startup is part of dependency handling: `--skip-deps` and the
`task.skip_depends` setting skip it. `--dry-run` validates daemon names and the
experimental setting, and reports what would start without starting anything.
Safe mode blocks task daemon startup.

See the [`daemons` task option](/tasks/task-configuration.html#daemons) for all
accepted values. Use `mise tasks info <task>` to inspect a task's daemon requirements.

## Daemons that run a task

Use `task` when the long-running command is already defined as a mise task:

```toml
[tasks."dev:core"]
run = "cargo run --bin core --"

[daemons.core]
task = "dev:core"
args = ["--verbose"]
ready_port = 8080
```

Start it with `mise daemons start core`. The `args` array passes arguments to the
task; in this example, Cargo forwards `--verbose` to the `core` application.
The daemon's readiness check is configured on `[daemons.core]`, not on the task.

A daemon's `task` cannot be combined with `run` or `preset`, and `args` requires
`task`. The referenced task must exist when daemons are registered.

::: warning Task dependencies are skipped
A daemon invokes its task with `mise run --skip-deps`. This prevents recursive
startup, but also skips that task's `depends` tasks and `daemons` requirements.
Put setup commands in [`init`](#setup-before-the-process-starts), and arrange
required services through pitchfork's daemon `depends` configuration or start
them separately.
:::

## Setup before the process starts

Use `init` for setup that must finish before the daemon starts. It accepts one
command or an ordered list, and works with `run`, `task`, and database presets:

```toml
[daemons.api]
init = ["npm ci", "npm run migrate"]
run = "exec npm start"
ready_port = 3000
```

Each command must succeed before the next runs. The setup commands and the
long-running command share a shell, so an exported variable or directory change
carries through to subsequent commands. For task daemons, the task still applies
its own environment and working-directory configuration. By default, setup runs
in the project's mise tool environment, including for task daemons. Setting
`mise = false` on the daemon disables that environment wrapper for both setup and
the long-running command.

**Write setup commands that are safe to repeat.** `init` runs on every start and
restart, including automatic restarts. Use commands such as `npm ci` or a migration
tool that can handle an already-initialized project.

Readiness checks apply after setup, so tasks and other daemons waiting for this
daemon also wait for `init`. For a database preset, the preset's database
initialization runs before your `init` commands. A preset may also override `run`;
both initialization steps still precede that command.

## Manage running daemons

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

Lifecycle commands accept project daemon names. Use pitchfork directly for group
operations: mise rejects `--group` because pitchfork groups can include daemons
outside the project.

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
