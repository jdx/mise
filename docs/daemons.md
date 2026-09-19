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
tools in `[tools]`; presets add their own required tools. A daemon declared with
`task` is the exception: mise is the entry point there, so it is not wrapped again
unless it also has [`init`](#setup-before-the-process-starts). The task still gets
the tool environment from mise itself. Use `exec` for the final
long-running command so it receives stop signals directly.

Fields such as `ready_port`, `ready_cmd`, and `auto` configure pitchfork's daemon
behavior. Set a readiness check that reflects when your service can accept work;
the example above waits for port 3000. Use an integer `port` for a fixed port or
[automatic ports](#ports-across-git-worktrees) to run services across worktrees.
Custom daemons also accept pitchfork's structured `port` table; presets accept
only an integer or mise's automatic port syntax.
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

A task can name a daemon imported from another project, by the name this project
gave it or by its full ID. `true` covers only this project's own daemons, so a
task asking for everything never reaches into a referenced project.

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

::: warning Subtasks do not start daemons
A task requirement is honored for the tasks a run resolves up front, including
their `depends`. A subtask reached through a `run = [{ task = "..." }]` entry is
resolved once the run is already executing, and its own `daemons` are not started.
Declare the requirement on the task you invoke.
:::

A daemon invokes its task with `mise run`, so that task's `depends` tasks run
before it, as they would on the command line. Its own `daemons` requirements are
the one exception: starting those would start this daemon again, so mise skips
them. Arrange services a supervised task needs through pitchfork's daemon
`depends` configuration, or start them separately.

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

A nearer declaration replaces a same-name daemon completely, so the name belongs to
the project that declared it last. A group in an outer project keeps naming it, but
the daemon is started by the project that now owns it. With a parent declaring
`default = ["postgres", "api"]` and a child redefining `postgres`, starting from the
child runs one `postgres`, the child's, together with the parent's `api`. One service
means one process, whichever project ends up owning it.

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
since it starts daemons too. `stop` without names still covers every project daemon.

Each project resolves that on its own. With inherited daemons, a `default` group in
one project does not limit what another project starts. Group names are project
scoped in the same way, so nested projects may each declare their own `default`.

Groups are also written to the generated pitchfork configuration with fully
qualified daemon IDs, so `pitchfork start --group two-cluster` works natively.
Pitchfork group names are global to its configuration, so choose distinct names
across projects if you invoke pitchfork directly.

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

After reviewing the referenced project's configuration, trust it and start the
worker from your application:

```sh
mise trust ../mirror-pipeline
mise daemons start pipeline
```

Run `mise daemons start api` to start the API and its worker dependency: the
referenced project is registered and started too, even though nothing named it.
Use the local name `pipeline` in commands and in `depends`; mise resolves it to
the worker's full daemon ID. A `[daemon_groups]` member cannot name it: a group
becomes a pitchfork group in this project's configuration and covers the daemons
this project declares. You do
not need to configure a namespace to use a project reference.

Starting resolves dependencies across projects, so a daemon here can depend on
one there. Stopping and logs do not: they act on the daemons this project named,
in the projects that own them.

### Paths and configuration

`project` accepts an absolute path or a path relative to the declaring
configuration's project root. Inherited references resolve against their parent
project's root. A reference in `.config/mise/config.toml` also resolves against the
project root, rather than the configuration file's directory.

`name` selects the daemon in the referenced project and defaults to the local
name. The referenced project can inherit daemon declarations and settings from
its parent configuration files. Reference the project that defines the daemon;
a reference cannot point to another reference.

All referenced configuration must already be trusted, including inherited files.
If additional trust is needed, mise reports the path and the `mise trust` command
to use after reviewing it. Running `mise run` or `mise daemons start` does not
implicitly trust another project's configuration.

A reference table accepts only `project` and `name`. Configure environment
variables and other daemon options in the project that defines the daemon.

### Environment and lifecycle

The worker runs in its own project with that project's tools, environment,
namespace, and data directories. Importing a database preset does not add its
exported variables, such as `DATABASE_URL`, to your application's environment.
Configure the application's database connection separately.

Start and restart install missing tools in each project and start the selected
daemons and their dependencies. Other daemons in the referenced project remain
registered and can continue running independently.

Automatic start and stop apply only to the current project's own daemons. Use
`mise daemons start` to start an imported daemon.

If a referenced checkout is missing or untrusted, the import is dropped and the
rest of your configuration is unaffected, so `mise run` and `mise x` keep working
and teammates can work without checking out every service.

Naming the unavailable daemon fails and explains why, so `mise daemons start
pipeline` reports the expected directory and the setting to update. Other daemon
commands warn and continue, so you can still list and stop your own daemons. A
`depends` entry pointing at the unavailable daemon is dropped rather than
registered, because there is no daemon ID to point it at. Starting a daemon that
declared that dependency fails and names the unavailable import, rather than
running it without something it said it needs.

An untrusted checkout is reported separately from a missing one, and mise never
trusts it for you. Run `mise trust` on the path it names after reviewing it.

## Namespaces

A daemon's full ID is `<namespace>/<name>`. By default, mise derives the namespace
from the project directory name and a hash of its path to separate checkouts.
Set `namespace` to give daemons predictable IDs that other projects can use in
`depends`:

```toml
[daemons_settings]
namespace = "services"

[daemons.db]
preset = "postgres"
version = "18"
```

In the main checkout, the database's ID is `services/db`. Another daemon can use
`depends = ["services/db"]` once that database is registered with pitchfork.
Use a [`project` reference](#daemons-from-another-project) when mise should also
load and register the other project's configuration.

Namespace names can contain ASCII letters, numbers, `.`, `_`, and `-`. They cannot
be empty, equal `.`, start or end with `-`, or contain `..` or `--`.

Mise chooses the namespace in this order:

1. `namespace` in `[daemons_settings]`
2. `namespace` in the project's `pitchfork.toml`
3. The generated default

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
uses `services-<hash>`, where `<hash>` is a 16-character hexadecimal hash of the
worktree path. Each checkout has separate daemon IDs and state.

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

Both bind to loopback and require their configured ports to be free. Use
[`port = "auto"`](#ports-across-git-worktrees) to derive ports for linked worktrees.
PostgreSQL uses the `postgres` user with local trust authentication. Any process
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

Use `port = "auto"` to run the same database preset in your primary checkout and
linked Git worktrees without assigning ports by hand:

```toml
[daemons.postgres]
preset = "postgres"
version = "18"
port = "auto"
```

The primary checkout uses PostgreSQL's default port, `5432`. In a linked worktree,
mise derives an offset from the project root's path. Connection variables such as
`PGPORT` and `DATABASE_URL` follow the resolved port, so applications can use the
same configuration in each checkout.

Automatic ports require the same `experimental = true` setting as other daemon
features. They are resolved when configuration loads, so `mise env` and `mise x`
can expose them before a daemon starts. Mise does not search for a free port or
change the port when it is occupied; see [port conflicts](#port-conflicts).

### Configure the base port and spacing

Use the table form to choose a base port. Custom daemons require `base` because
they have no preset default:

```toml
[daemons.api]
run = "exec npm run dev -- --port $API_PORT"
port = { auto = true, base = 3000 }
```

This example assumes the application's `dev` script accepts `--port`. The primary
checkout uses `3000`; linked worktrees use ports from `3001` through `3511`.
Configure the application to listen on the exported port. A fixed `ready_port`
does not follow an automatic port, so omit it or use a readiness check that reads
the resolved port.

Both presets and custom daemons accept these options:

| Option   | Meaning                                            | Default                                            |
| -------- | -------------------------------------------------- | -------------------------------------------------- |
| `auto`   | Enables automatic port allocation. Must be `true`. | Required in the table form.                        |
| `base`   | Port used by the primary checkout.                 | The preset's default; required for custom daemons. |
| `stride` | Spacing between allocation slots.                  | `1`                                                |

For a service that uses several consecutive ports, set `stride` to the size of
that range, for example `port = { auto = true, base = 3000, stride = 10 }`.
This separates different slots by ten ports; it does not prevent two projects
from receiving the same slot. Configuration loading fails if the resolved port
would exceed `65535`.

There are 511 possible worktree offsets. With the default base and stride,
PostgreSQL uses `5433`–`5943` in linked worktrees and Redis uses `6380`–`6890`.

The variable is a convenience, so a name that cannot produce a usable one costs only
the variable and never the daemon. Two daemons whose names differ only by punctuation,
such as `web-ui` and `web_ui`, would claim the same variable, so neither exports it and
mise warns. A name beginning with a digit cannot be a shell variable at all, so it goes
without one and mise warns. Both daemons run normally in either case, and their ports
still reach pitchfork, which injects `$PORT` into the process it starts regardless. The
same name decides `<NAME>_URL`, described in
[Stable URLs per worktree](#stable-urls-per-worktree), so a name that withholds one
withholds both.

Two daemons in one project cannot share a port, and mise says so when the configuration
loads rather than letting the second fail to bind. Two instances of one preset are the
usual way to reach this, since they share a base port: give the second its own `port`,
or its own `base` when both use `port = "auto"`.

```toml
[daemons]
postgres = "18"

[daemons.analytics]
preset = "postgres"
version = "18"
port = { auto = true, base = 5500 }
```

### Project layout and port stability

Mise detects the enclosing checkout even when `mise.toml` is nested in a directory
such as `packages/api`. Each project root inside a linked worktree is hashed
separately. Submodules follow their enclosing checkout: they receive an offset
inside a linked worktree and keep the base port inside a primary checkout.

Independent clones, including `git clone --separate-git-dir`, and projects outside
Git keep the base port. Worktrees of a bare repository all receive offsets because
there is no primary checkout. To give one checkout a fixed port, set an integer
`port` in a checkout-specific configuration such as a gitignored `mise.local.toml`.

Mise saves resolved ports in the project's generated `state.json` during daemon
registration and reuses them on later loads. This preserves existing assignments
if the allocation algorithm changes. Changing `base` or `stride` causes mise to
resolve the port again; restart the affected daemon after making that change.

Use `mise daemons ls --json` to inspect assignments. Each listed daemon includes
`port` for its resolved port and `port_auto` to indicate automatic allocation.

### Port environment variables

Presets export their usual connection variables with the resolved port, including
`PGPORT` and `DATABASE_URL` for PostgreSQL and `REDIS_URL` for Redis.

Custom daemons with an integer or automatic `port` export `<NAME>_PORT`. Mise
uppercases the daemon name and replaces punctuation with underscores:
`[daemons.api]` exports `API_PORT`, and `[daemons.web-ui]` exports `WEB_UI_PORT`.
These variables are available through `mise env`, `mise x`, and the daemon's mise
environment. Explicit `[env]` values take precedence over daemon exports.

If a name starts with a digit, or two names map to the same variable (such as
`web-ui` and `web_ui`), mise warns and omits the affected exports. The daemons can
still run. Pitchfork also provides `$PORT` to the process it starts.

### Port conflicts

Automatic ports are derived from paths, so different projects can receive the
same port. When another mise-managed project has a running daemon on that port,
startup fails with an error identifying the daemon and its project root. Change
one project's `base` or stop the other daemon before starting again.

Stopped daemons do not reserve ports. Conflict checks cover only the daemons being
started: `mise daemons start redis` is not blocked by a conflict on this project's
PostgreSQL port. The checks apply to fixed integer ports as well as automatic ports.

These checks do not reserve ports or detect every listener. An unmanaged process,
an unreachable supervisor, or two projects starting simultaneously can still
cause an ordinary bind failure. Mise keeps the selected port rather than trying
another one, so existing shells retain the same connection settings.

## Stable URLs per worktree

Ports separate concurrent checkouts, but they also mean every service has to be told
which port its neighbours ended up on. For anything that speaks HTTP, pitchfork's
reverse proxy removes that step: it routes a stable hostname to whatever port the
daemon actually bound, and mise derives the same hostname while configuration loads.

Every proxied daemon is reachable at a hostname. A daemon is proxied when it
configures a `port` and has not opted out with `proxy = false`:

```
<daemon>.<project>.<tld>              in the primary checkout
<daemon>.<worktree>.<project>.<tld>   in a linked git worktree
```

The daemon component is the daemon's own name, the project component comes from the
project's pitchfork namespace, and a linked worktree adds a component of its own. A
project with `namespace = "shop"` checked out at `~/src/shop` therefore serves its
`api` daemon at `https://api.shop.localhost`, and a linked worktree at
`~/src/shop-pr-42` serves the same daemon at `https://api.shop-pr-42.shop.localhost`.
Both can run at once, and neither URL changes when a port moves.

Mise exports that URL as `<NAME>_URL` next to `<NAME>_PORT`, using the same naming
rules, so another service can be pointed at it without any port arithmetic:

```toml
[daemons.api]
run = "npm run dev"
port = "auto"

[env]
APP_BASE_URL = "{{ env.API_URL }}"
```

Every HTTP service in the stack can reference its neighbours this way, which is what
lets several worktrees of one project run concurrently without a per-worktree port
table. Databases keep `port = "auto"` instead: the proxy speaks HTTP, and a Postgres
or Redis client does not, so the `postgres` and `redis` presets opt out of it and keep
exporting `PGPORT`, `DATABASE_URL`, and `REDIS_URL`.

A daemon without a `port` is never routed and gets no URL, and neither is one that
opted out. `mise daemons urls` still lists both, with their ports.

### Per-daemon proxy settings

A daemon can take a different hostname label, or opt out of the proxy entirely:

```toml
# https://front.shop.localhost, not https://web.…
[daemons.web]
run = "npm run dev"
port = 5173
proxy = "front"

# No hostname and no WORKER_URL; reachable only on its port.
[daemons.worker]
run = "npm run worker"
port = 9000
proxy = false
```

**Set `proxy = false` on any daemon that does not speak HTTP.** The proxy serves
HTTP, so a custom Redis or Postgres daemon would otherwise be given an `https://`
hostname and a `REDIS_URL` or `DATABASE_URL` pointing at it, which is not what a
client of that database expects. The database presets already do this for you.

`proxy = true` turns routing back on for a daemon that a preset opted out of, using
the daemon's own name as the label.

`proxy_tls` chooses what the proxy does with TLS for that daemon. The default,
`"terminate"`, means the proxy serves HTTPS and forwards plain HTTP to the daemon.
Use `"passthrough"` when the daemon serves TLS itself and the connection should reach
it unbroken:

```toml
[daemons.api]
run = "npm run dev:https"
port = 3000
proxy_tls = "passthrough"
```

Both keys are forwarded to pitchfork unchanged. Hostname routing needs pitchfork
2.26.0 or later; an older supervisor starts the daemons normally but does not serve
the hostnames.

### Naming the project and the worktree

The project component is the explicit `[daemons_settings] namespace`, before any
per-worktree suffix, so `namespace_per_worktree` keeps separating daemon IDs while
the worktree component does the separating in hostnames. Without an explicit
namespace, both mise and pitchfork name the project after the primary checkout's
directory.

A bare repository has no primary checkout, so each worktree beside it names itself
and gets no worktree component: a daemon in `shop/main` is `api.main.localhost`, not
`api.main.shop.localhost`. The directory holding a bare repository often holds
unrelated ones too, and naming the project after it would put them on one label.

Because there is no worktree component, an explicit `namespace` in a file every such
worktree shares gives them all one hostname, and `worktree_label` cannot separate
them: it applies only to the worktree component. Give each checkout a namespace of
its own instead, in a gitignored `mise.local.toml`:

```toml
# mise.local.toml, in one worktree of the bare repository
[daemons_settings]
namespace = "shop-pr-42"
```

The worktree component is the linked worktree's directory name. To name it yourself,
set `worktree_label` in a pitchfork configuration file inside that worktree, which is
where pitchfork reads it:

```toml
# pitchfork.local.toml, in the worktree
worktree_label = "pr-42"
```

Mise reads the key from the same place, so the URL it exports and the hostname the
proxy serves stay the same. Use `pitchfork.local.toml` and gitignore it: a tracked
file is shared by every checkout, and a worktree label has to differ between them.

Labels are folded to lowercase letters, digits and `-`. Two daemons, worktrees or
projects whose names fold to one label collide, and pitchfork routes neither; mise
withholds both URLs for the same pair and warns, rather than exporting an endpoint
the proxy refuses. The daemons still run on their ports.

### Seeing the URLs

`mise daemons urls` prints every daemon's hostname next to the port it binds, grouped
by project root, with the pages pitchfork serves for the whole stack:

```sh
mise daemons urls
```

```
~/src/shop-pr-42
Daemon         URL                                 Port  Proxy        Status
shop/api       https://api.pr-42.shop.localhost    3117  terminate    running
shop/web       https://front.pr-42.shop.localhost  5173  passthrough  running
shop/postgres  -                                   5679  off          running
  stack:   https://pr-42.shop.localhost
  project: https://shop.localhost
```

The proxy column is the daemon's `proxy_tls` mode, or `off` when it has no hostname.
Such a daemon is listed with its port alone rather than omitted, so a database is
visible here too. `mise daemons ls --json` carries the same information in its `host`,
`url`, and `proxy` fields. The primary checkout has no stack page of its own; its
stack is the project.

### Where the scheme and port come from

Mise derives the URL the way pitchfork does: the scheme follows `proxy.https`, the TLD
follows `proxy.tld`, and the port appears only when it is not the standard one for that
scheme. It reads those from `/etc/pitchfork/config.toml` and
`~/.config/pitchfork/config.toml`, with `PITCHFORK_PROXY_*` environment variables
taking precedence, which are the settings layers that apply wherever the daemon is
started from. A project-level `[settings.proxy]` is deliberately not consulted, because
it would make a URL depend on the directory the supervisor happened to start in.

With no pitchfork configuration at all, mise assumes pitchfork's defaults: HTTPS on
port 443 under `.localhost`, so a URL is exported before the proxy is switched on. See
pitchfork's [port management guide](https://pitchfork.jdx.dev/guides/port-management)
for enabling the proxy and trusting its certificate.

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
