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

## Groups

Name a set of project daemons in `[daemon_groups]` and use that name wherever a
daemon name is accepted:

```toml
[daemon_groups]
default = ["postgres", "nats", "core", "node0", "node1"]
two-cluster = ["default", "core2", "c2-node0"]
```

A member is another daemon in the same project or another group in that same project,
which expands in place. Members are validated when configuration loads, so a group can
never select a daemon outside the project, including a same-named daemon in a parent or
child project. An equivalent table form matching pitchfork's own
syntax also works:

```toml
[daemon_groups.two-cluster]
daemons = ["default", "core2", "c2-node0"]
```

```sh
mise daemons start two-cluster
mise daemons stop --group two-cluster
mise daemons logs default
```

A positional name is resolved by each project in its own terms: it selects that
project's group of that name, or failing that a daemon of that name. So if one
project declares a group `web` and another declares a daemon `web`, the one name
selects the group in the first and the daemon in the second. `--group` only ever
selects a group, and never a daemon that happens to share its name.

When a nearer project redefines a daemon, the group that named it keeps its member,
but selection stays per project, so the group no longer reaches that daemon. With a
parent declaring `default = ["postgres", "api"]` and a child redefining `postgres`,
`mise daemons start default` from the child starts `api` alone. The redefined
`postgres` belongs to the child, which starts it through its own daemons rather
than the parent's group. Declare the group in the project that owns the daemons if
you want one command to cover both.

A group is an alias in the configuration rather than persisted state, so removing
one leaves nothing to expand. Daemons it started are still tracked by name: `mise
daemons ls` lists them, and `mise daemons stop` without names stops every daemon
the project is tracking.

A group name may not repeat a daemon name in the same project, and a group must
have at least one member. `--group` is accepted only for groups declared in
`[daemon_groups]`; a pitchfork group defined elsewhere is rejected because it can
include daemons outside the project. Use pitchfork directly for those.

`mise daemons start` with no names starts the `default` group when the project
declares one, and otherwise starts every project daemon. `restart` does the same,
since it starts daemons too. `stop` without names still covers every project daemon. Each project resolves this
on its own: with inherited daemons, a `default` group in one project does not limit
what another project starts. Group names are likewise project scoped, so nested
projects may each declare their own `default`.

Groups are also written to the generated pitchfork configuration with fully
qualified daemon IDs, so `pitchfork start --group two-cluster` works natively.
Pitchfork group names are global to its configuration, so choose distinct names
across projects if you invoke pitchfork directly.

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
