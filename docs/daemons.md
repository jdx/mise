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
`port` is an integer or `"auto"`. Custom daemons accept the same values, or pitchfork's
structured `port` configuration.
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

## Database presets

| Preset     | Tool       | Default port | Environment defaults                                       |
| ---------- | ---------- | ------------ | ---------------------------------------------------------- |
| `postgres` | `postgres` | 5432         | `PGHOST`, `PGPORT`, `PGUSER`, `PGDATABASE`, `DATABASE_URL` |
| `redis`    | `redis`    | 6379         | `REDIS_URL`                                                |

Both bind to loopback and require their configured ports to be free. A fixed port
never bumps; use [`port = "auto"`](#ports-across-git-worktrees) to give each worktree
its own. PostgreSQL uses the `postgres` user with local trust authentication. Any process
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

## Ports across git worktrees

Mise renders a daemon's port into its command line and its `[env]` exports while
configuration loads, so `mise env` and `mise x` see the endpoint without a discovery
step. That happens long before pitchfork could pick a port, which is why pitchfork's
own `bump` cannot separate two checkouts of the same project. Running the same
daemons from a primary checkout and several linked worktrees at once would otherwise
fail on the second `mise daemons start`.

Set `port = "auto"` to derive the port from the project root instead:

```toml
[daemons.postgres]
preset = "postgres"
version = "18"
port = "auto"

[daemons.api]
run = "npm run dev"
port = { auto = true, base = 3000 }
```

The primary checkout keeps the base port, so a single-checkout project is unchanged:
`PGPORT` stays `5432` and the API stays on `3000`. Each linked git worktree gets a
stable offset derived from its path, so the same worktree always resolves to the same
port and two worktrees do not collide.

The enclosing checkout decides, not the directory holding `mise.toml`, so a config
nested in a monorepo such as `packages/api/mise.toml` still follows its worktree.
Sibling projects within one worktree keep separate ports. Only a checkout created by
`git worktree add` is offset; a submodule, a `git clone --separate-git-dir`, and a
project outside git are each the single copy of their project and keep the base port.

`base` sets the port the primary checkout uses. It defaults to the preset's port and
is required for custom daemons, which have no default to offset. `stride` sets the
distance between consecutive worktree slots and defaults to `1`; raise it for a daemon
that binds a contiguous range so neighbouring worktrees cannot overlap. A large `base`
or `stride` can push a high-numbered slot past 65535, which fails at config load with a
message naming the daemon.

There are 512 slots, one for the primary checkout and 511 for worktrees. That keeps an
allocated port recognisably near its base: Postgres stays within 5432 to 5943 and Redis
within 6379 to 6890, so the two preset ranges cannot reach each other.

The resolved port is recorded in the generated `state.json` alongside the `base` and
`stride` it came from, and `mise daemons ls --json` reports it:

```sh
mise daemons ls --json
```

```json
[
  {
    "id": "mise-3f0a/postgres",
    "name": "postgres",
    "port": 5679,
    "port_auto": true
  }
]
```

Because the allocation is persisted, a future change to how offsets are derived cannot
move a daemon that is already running. Editing `base` or `stride` does re-derive it.
Starting a daemon fails when another project root on this machine is _running_ a
daemon on the same port, naming that root. Liveness is what matters, and it is checked
for the daemon holding the port rather than the project around it, so an unrelated
daemon or an open shell session in that project does not make it look busy. A stopped
project never reserves a port, so two projects can still take turns on a default port
such as 5432 exactly as before. The check only probes a root whose port actually
matches, and it leaves the port available when that project's supervisor cannot be
reached. It is a diagnostic rather than a reservation: two projects starting at the same
instant can still both see a port as free, and binding remains the final arbiter.

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
