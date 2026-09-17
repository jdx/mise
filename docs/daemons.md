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

## Namespaces

Each project's daemons live in a pitchfork namespace. By default that namespace is
the project directory name plus a hash of its path, which never collides between
unrelated checkouts but is not predictable enough to write down. Name it explicitly
when another project needs to refer to these daemons:

```toml
[daemons_settings]
namespace = "entiredb"
```

Namespaces follow the same rules as daemon names: letters, numbers, `.`, `_` and
`-`, no leading or trailing `-`, and no `--`. Daemon IDs are then `entiredb/<name>`.
A `namespace` key in a project's own `pitchfork.toml` still wins over the default;
`[daemons_settings]` wins over both.

`[daemons_settings]` merges key by key across configuration files, so a
`mise.local.toml` that sets only `namespace_per_worktree` keeps the `namespace`
that `mise.toml` established. This differs from `[daemons]`, where a
higher-precedence declaration replaces a same-name daemon completely.

Changing the namespace of a project whose daemons are running fails; stop them
first. Once nothing is running, mise adopts the new namespace and forgets IDs from
the old one.

### Worktrees

An explicit namespace is written in a configuration file, so every linked git
worktree of the same repository would claim the same one and the two checkouts
would fight over the same daemon IDs and state directory. In a linked worktree
mise therefore appends a worktree-specific suffix, giving `entiredb-<hash>`. The
main checkout keeps the unsuffixed name.

That trade-off is deliberate: predictable IDs in the main checkout, isolation
everywhere else. It means a qualified `depends` written against `entiredb/db`
resolves in the main checkout but not from a linked worktree. If you would rather
have every worktree share one set of daemons, and you accept that two worktrees
running them at once will collide, turn the suffix off:

```toml
[daemons_settings]
namespace = "entiredb"
namespace_per_worktree = false
```

## Daemons from another project

A daemon table with `project` pulls in a daemon that a sibling project declares,
instead of redefining it:

```toml
[daemons.pipeline]
project = "../mirror-pipeline"
name = "worker"

[daemons.api]
run = "npm run dev"
depends = ["pipeline"]
```

`project` is a directory whose mise configuration declares the daemon; a relative
path resolves against this project root. `name` is the daemon's name inside that
project and defaults to the local key. The referenced directory must be trusted,
exactly as it would be if you had changed into it.

The referenced directory is read through its whole configuration hierarchy, the
same one mise uses when it runs the daemon, so a daemon or a `[daemons_settings]`
namespace the project inherits from a parent configuration is found too.

The imported daemon keeps its own project: it runs with that directory as its
working directory, under that project's namespace, and with that project's state,
data, and `mise x` environment. `mise daemons start pipeline` from this project
starts `mirror/worker` in the sibling checkout. Because the daemon belongs to the
other project, its tool and its exported environment variables stay there —
`[daemons]` presets in a referenced project do not export `DATABASE_URL` here.

`depends` may name the imported daemon by its local key; mise rewrites it to the
qualified ID, since pitchfork resolves bare names only inside one namespace. You can
also write the qualified ID yourself.

Starting an import registers the referenced project's complete generated
configuration, because that file is rewritten as a whole and its other daemons
must survive. Only the daemons this project imported are started or stopped;
the rest stay under the referenced project's own control.

If the directory is missing, mise names the path it expected and the setting to
change, so a developer who keeps sibling checkouts somewhere else knows what to fix.

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
