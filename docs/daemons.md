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
custom process. A table with `preset` and `version` selects a preset for any instance
name, and remaining fields override its pitchfork daemon definition. For presets,
`port` is an integer. Custom daemons accept the same integer shorthand or pitchfork's structured `port` configuration.
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
Lifecycle commands accept project daemon names; `--group` is rejected because
pitchfork groups can include daemons outside the project. Use pitchfork directly
for group operations.

## Daemons from another project

Use `project` to run a daemon defined in another checkout. This lets an application
start a shared service without copying its daemon configuration.

For example, define a worker in `../mirror-pipeline/mise.toml`:

```toml
[daemons.worker]
run = "npm run worker"
```

Then reference it from your application's `mise.toml`:

```toml
[daemons.pipeline]
project = "../mirror-pipeline"
name = "worker"

[daemons.api]
run = "npm run dev"
depends = ["pipeline"]
```

Run `mise daemons start pipeline` to start the worker, or `mise daemons start api`
to start the API with the worker as a dependency. Use the local name `pipeline`
in lifecycle commands and `depends`; mise resolves it to the worker's full daemon
ID. A fixed namespace is optional when using `project` references.

`project` accepts an absolute path, or a path relative to the project root of the
configuration file that declares it. That is the declaring file's own project, so
a `project` inherited from a parent configuration resolves against the parent's
root, not the directory you are in. For a nested file such as
`.config/mise/config.toml` it is the project root, not the file's directory. `name` selects the daemon in the referenced
project; omit it when the local and remote names match. Mise reads the referenced
project's parent configuration files too, so inherited daemon declarations and
settings are available.

The referenced project must already be trusted. Run `mise trust <dir>` after
reviewing it. Mise will not trust it for you, even from commands such as `mise run`
that implicitly trust the configuration they are running, because `project` would
otherwise grant a directory lasting trust without asking.

The table accepts only `project` and `name`. A daemon runs under one registration
in its own project, so it cannot be given per-importer `env` or other overrides;
if the referenced daemon needs values from your project, it has to read them from
its own configuration or environment.

### Environment and lifecycle

The worker runs in its owning project, with that project's tools, environment,
namespace, and data directories. Importing a database preset does not add its
exported variables, such as `DATABASE_URL`, to the application's environment.
Configure the application's connection separately.

Lifecycle commands select the daemons you imported and start their dependencies
as needed. Other daemons in the referenced project remain registered and can
continue running; importing one daemon does not give your project control over
all of them.

Automatic lifecycle does not cross projects. Shell hooks register and start only
this project's own daemons, because registering another project's configuration
from here would rewrite it with just the daemons you imported. Use
`mise daemons start` for an imported daemon.

If a checkout is missing or untrusted, `mise daemons` reports the expected
directory and the `project` setting to update. Every other command keeps working:
the imported daemon is dropped, and your own daemons, tools, and environment are
unaffected. A teammate without that checkout can still run `mise run` and `mise x`.

## Namespaces

A daemon's full ID is `<namespace>/<name>`. By default, mise derives the namespace
from the project directory name and a hash of its path to separate checkouts.
Set `namespace` when you need a stable ID for a qualified dependency:

```toml
[daemons_settings]
namespace = "services"

[daemons.db]
preset = "postgres"
version = "18"
```

The database's ID is `services/db`. Another daemon can use
`depends = ["services/db"]` once that database is registered with pitchfork.
Use a [`project` reference](#daemons-from-another-project) when mise should also
load and register the other project's configuration.

Namespace names accept ASCII letters, numbers, `.`, `_`, and `-`. They cannot be
empty, equal `.`, start or end with `-`, or contain `..` or `--`. An explicit
`[daemons_settings].namespace` takes precedence over `namespace` in the project's
`pitchfork.toml`, which in turn takes precedence over the generated default.

Stop the project's daemons before changing its namespace. The next start adopts
the new namespace and removes the old IDs from mise's state.

### Configuration inheritance

`[daemons_settings]` merges individual keys across configuration files. For example,
setting only `namespace_per_worktree` in `mise.local.toml` preserves `namespace`
from `mise.toml`. In contrast, a higher-precedence `[daemons.<name>]` declaration
replaces that daemon's entire definition.

Child projects inherit daemon settings from parent configuration files. If several
projects inherit one namespace, give their daemons distinct names or override the
namespace in each project. Mise rejects duplicate IDs among the projects it loads;
it cannot detect collisions with projects outside that configuration hierarchy or
its imports. Global and system configuration cannot set `[daemons_settings]`;
mise ignores those tables with a warning.

### Git worktrees

Linked Git worktrees get a path-specific suffix on an explicit namespace. With
`namespace = "services"`, the main checkout uses `services` and a linked worktree
uses `services-<hash>`, where the hash is 16 hex characters, so IDs in a worktree
are noticeably longer. Each checkout therefore has separate daemon IDs and state.

A literal dependency on `services/db` always refers to that exact ID; it does not
follow the current worktree's suffix. Prefer a local daemon name or a `project`
reference when the dependency should resolve to a particular checkout.

To use the same namespace across worktrees, disable the suffix:

```toml
[daemons_settings]
namespace = "services"
namespace_per_worktree = false
```

Only use this when you intend to share daemon IDs. Starting daemons from multiple
worktrees at once can cause collisions.

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
(for example, `setup-command && exec server`) so it receives stop signals directly.

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
