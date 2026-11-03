---
description: Set up a development stack with project daemons, a stable URL for each Git worktree, and a supervisor that starts at login.
socialDescription: Project daemons, a URL per Git worktree, and a supervisor at login.
---

# Set up a development stack <Badge type="warning" text="experimental" />

This guide sets up a Node.js API and its PostgreSQL database so that opening
`https://api.shop.localhost` starts both, each Git worktree gets its own
database and URL, and idle stacks stop on their own. One pitchfork supervisor,
started at login, runs the daemons of every repository on your machine, and
each repository keeps its own definitions in `mise.toml`. For every option, see
the [daemons overview](/daemons.html).

::: warning Experimental
Daemons are experimental. The config below enables them with
`experimental = true` in the project's `[settings]`.
:::

## Before you start

You need:

- A Git repository with a dev server that accepts `--host` and `--port` and
  serves a `/health` endpoint. The examples use a Node.js application in
  `~/src/shop`.
- `curl`, which the example's readiness check uses.
- pitchfork on your `PATH` for its `proxy` and `supervisor` commands:
  `mise use -g pitchfork`. Hostnames need pitchfork 2.26.0 or later and idle
  shutdown needs 2.27.0; see [Requirements](/daemons.html#requirements).

Set up pitchfork's local HTTPS proxy once per machine. Turn it on in
`~/.config/pitchfork/config.toml`:

```toml
[settings.proxy]
enable = true
```

Then point the machine's DNS, certificate trust, and HTTPS port at the proxy,
and check the result:

```sh
pitchfork proxy setup
pitchfork proxy doctor
```

`pitchfork proxy setup` prints a plan and asks before it changes anything. It
uses sudo for the system changes it needs, so the supervisor itself can run as
your normal user. See pitchfork's
[local proxy setup](https://pitchfork.jdx.dev/guides/port-management#local-proxy-setup)
for platform details. The supervisor reads proxy settings when it starts, so
restart it if it is already running:

```sh
pitchfork supervisor stop
pitchfork supervisor start
```

## Define one application

Presets run databases and brokers, `run` runs application servers, and
`depends` lists the services a server needs. For a Node.js application in
`~/src/shop`, start with this `mise.toml`:

```toml
[settings]
experimental = true

[tools]
node = "24"

[daemons_settings]
namespace = "shop"

[daemons.db]
preset = "postgres"
version = "18"
port = "auto"

[daemons.api]
run = "exec npm run dev -- --host 127.0.0.1 --port $API_PORT"
port = { auto = true, base = 3000 }
ready_cmd = "curl -fsS http://127.0.0.1:$API_PORT/health"
depends = ["db"]

[tasks.test]
daemons = ["db"]
run = "npm test"
```

The database preset installs PostgreSQL, checks that it is ready, keeps its
data, and exports `DATABASE_URL`. The API starts after the database is ready.
Use the exported connection variables in your application instead of copying
localhost ports into `.env` files.

Adapt the server command and the `/health` endpoint to your application. The
server must listen on `API_PORT`: a port in mise config cannot move a port the
application hardcodes.

Trust the config, register the project, and list its URLs:

```sh
cd ~/src/shop
mise trust
mise daemons register
mise daemons urls
```

`mise daemons register` installs missing tools, checks the definitions and
their dependencies, and registers the project with pitchfork without starting
anything. Run it again after you change a daemon's declaration.
`mise daemons urls` lists `https://api.shop.localhost` for the API and port
`5432` for the database. Listing URLs does not register a project or install
its tools.

`mise run test` starts the database before it runs the tests. To start the API
yourself, run `mise daemons start api`, and read its output with
`mise daemons logs api`.

## Open it by URL

Open `https://api.shop.localhost`. The request starts the database, waits until
it is ready, starts the API, and then reaches it. The supervisor and its proxy
must already be running. A DNS lookup by itself does not start a daemon, and
visiting an unknown hostname cannot register a checkout.

Daemons started this way keep running until you stop them. To stop them when
the stack sits idle, add an idle timeout to `~/.config/pitchfork/config.toml`
and restart the supervisor:

```toml
[settings.proxy]
enable = true
idle_timeout = "15m"
```

After 15 minutes without activity, pitchfork stops the proxy-started API and
then its unused dependencies. An open streaming response or WebSocket keeps the
API active, and a shared dependency stays running while another running
consumer needs it. The next request starts the stack again; preset data stays
on disk.

To set the timeout for one daemon instead of the whole machine, add
`proxy_idle_timeout = "15m"` to its `[daemons.<name>]` table, or `false` to
keep it running. See [Stop idle daemons](/daemons/worktrees.html#stop-idle-daemons).

Explicit starts are exempt: `mise daemons start` keeps a daemon and its
dependencies running until you stop them. If you started the API that way
earlier, stop the stack once with `mise daemons stop api db` before you try the
on-demand workflow.

Shell sessions can also keep proxy-started daemons running.
[Start and stop with your shell](/daemons.html#automatic-start-and-stop) tracks
shells entering and leaving projects; browser-driven startup does not need it.

## Add a worktree

Because the database and the API use automatic ports, a linked worktree gets
its own daemon IDs, ports, and database with no extra configuration. A custom
daemon needs an explicit `base`, as the API has; a preset uses its default port
as the base.

```sh
git worktree add ../shop-feature -b feature
cd ../shop-feature
mise trust
mise daemons register
```

The primary checkout's API is at `https://api.shop.localhost`, and the
worktree's is at `https://api.shop-feature.shop.localhost`. Use `API_URL` for
HTTP clients and `DATABASE_URL` for the database. The proxy carries HTTP only,
not PostgreSQL or Redis traffic.

A preset's extra listeners, such as CockroachDB's HTTP port, move with
`port = "auto"` too. A connection URI you write into a preset option, such as
SpiceDB's `datastore_uri`, is not rewritten. See
[Named ports](/daemons/worktrees.html#named-ports).

## Share a service

Put infrastructure that several applications share in its own repository, for
example `~/src/services`, and declare each service there once. Applications
reference that project instead of copying its declaration or turning off
worktree isolation.

In `~/src/services/mise.toml`:

```toml
[settings]
experimental = true

[daemons_settings]
namespace = "services"

[daemons.events]
preset = "nats"
version = "2"
```

In `~/src/shop/mise.toml`, add the reference and add `events` to the API's
`depends`:

```toml
[daemons.events]
project = "../services"

[daemons.api]
run = "exec npm run dev -- --host 127.0.0.1 --port $API_PORT"
port = { auto = true, base = 3000 }
ready_cmd = "curl -fsS http://127.0.0.1:$API_PORT/health"
depends = ["db", "events"]

[env]
NATS_URL = "nats://127.0.0.1:4222"
```

Review and trust `../services`, then run `mise daemons register` again. mise
registers the referenced daemon too, and pitchfork starts `events` before the
API when the API is requested. `project` paths are relative to this project's
root, so keep the sibling layout or use an absolute path.

A reference shares the process, not its environment variables. `NATS_URL` above
connects to the shared service's fixed port, because the preset's exports stay
in the services project. Stop shared infrastructure from its own project, and
only when no other consumer needs it.

To share one PostgreSQL server across all your checkouts, with a separate
database for each, use a
[shared server provider](/daemons/sharing.html#shared-server-providers) instead
of a database per worktree. For an authorization and messaging stack, see the
[CockroachDB, SpiceDB, and NATS example](/daemons/presets.html#example-cockroachdb-spicedb-and-nats).

## Start the supervisor at login {#keep-the-supervisor-available-at-login}

On a machine you manage with [mise bootstrap](/bootstrap.html), declare a user
service in your global config. Use the permanent absolute path to your mise
binary in both places below. `type -P mise` in Bash, `whence -p mise` in Zsh,
or `command -s mise` in Fish prints it, for example `/opt/homebrew/bin/mise` on
macOS with Homebrew, or a path under `~/.local/bin` on Linux. In a Bash or Zsh
shell where mise is activated, `command -v mise` prints only `mise`, the name
of the shell function.
`PITCHFORK_MISE_BIN` makes daemon commands use that same binary, even when
several mise installations exist.

```toml
[tools]
pitchfork = "latest"

[bootstrap.services.pitchfork]
scope = "user"
command = "/opt/homebrew/bin/mise exec -- pitchfork supervisor run --boot"
environment = { PITCHFORK_MISE_BIN = "/opt/homebrew/bin/mise" }
working_directory = "~"
requires_tools = true
restart = "on-failure"
```

Pin a tested pitchfork version in your machine config when you need a
reproducible setup. `requires_tools` makes a full `mise bootstrap` install the
tool before it starts the service. The foreground `supervisor run --boot`
process is what launchd or systemd supervises; `supervisor start` would detach
from it.

Preview with `mise bootstrap --dry-run`, apply with `mise bootstrap`, and check
the result with `mise bootstrap services status`. On macOS the service is a
user LaunchAgent, `~/Library/LaunchAgents/dev.mise.pitchfork.plist`; on Linux
it is a systemd user unit, `~/.config/systemd/user/dev.mise.pitchfork.service`.
Either one starts at login, not before a user logs in. See
[User services](/bootstrap/services.html#user-services). `--boot` starts daemons
marked
[`boot_start = true`](https://pitchfork.jdx.dev/reference/configuration#boot-start);
leave that unset on applications you want to start only on demand.

To restart the supervisor after you change its settings:

```sh
systemctl --user restart dev.mise.pitchfork.service       # Linux
launchctl kickstart -k "gui/$(id -u)/dev.mise.pitchfork"  # macOS
```

Choose one owner for login startup. Before you apply the bootstrap service,
stop a supervisor you started by hand, such as the one from
[Before you start](#before-you-start), with `pitchfork supervisor stop`. If you
previously used `pitchfork boot enable`, also run `pitchfork boot disable`.
Stopping the supervisor interrupts its running daemons, so do it when the stack
can stop. Do not register the same supervisor both ways.

To keep CLI commands from starting an unmanaged replacement supervisor, set
this in `~/.config/pitchfork/config.toml` once the managed service works:

```toml
[settings.supervisor]
auto_start = false
```

Without mise bootstrap, use
[`pitchfork boot enable`](https://pitchfork.jdx.dev/guides/boot-start) instead.
Either way, the project definitions stay in their repositories.

## Next steps

- Stop the stack with `mise daemons stop api db`.
- After `git worktree remove`, run
  [`mise daemons prune`](/daemons/data.html#clean-up-deleted-projects) to delete
  that worktree's database.
- Run migrations before the API starts with
  [`init`](/daemons.html#setup-before-the-process-starts), for example
  `init = "npm run migrate"`. It runs after the dependencies are ready, on
  every start, so make it safe to repeat.
- Keep short-lived build and test commands as tasks. Run a task as the server
  only when its command already needs a mise task; see
  [Run a task as a daemon](/daemons.html#daemons-that-run-a-task).
- Add a [`default` group](/daemons.html#groups) so `mise daemons start` starts
  only the daemons you use every day.
- See [Service presets](/daemons/presets.html) for Redis, CockroachDB, NATS, and
  SpiceDB.
