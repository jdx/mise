---
description: Set up a development stack with project daemons, shared services, and separate URLs for each worktree.
---

# Set up a development stack

Keep each application's tools, environment, and daemons in its `mise.toml`.
Use presets for databases and brokers, `run` for application servers, and
`depends` for the services a server needs. One Pitchfork supervisor manages
all the repositories on your machine.

This is the recommended starting point for a stack spread across `~/src`.
Start with explicit commands, verify the stack, then add browser-triggered
startup and login persistence. The [daemon reference](/daemons.html) covers
other declaration forms and all the configuration options.

::: warning Experimental
Daemon management requires `experimental = true` and an installed Pitchfork.
:::

## Define one application

For a Node.js application in `~/src/shop`, start with:

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

Adapt the server command and `/health` endpoint to your application. The server
must listen on `API_PORT`; assigning a port in configuration cannot change a
hardcoded application port. This example also requires `curl`.

The database preset installs PostgreSQL, checks readiness, preserves its data,
and exports `DATABASE_URL`. The API starts after the database is ready.
Use the exported connection variables in your application instead of repeating
localhost ports in `.env` files.

```sh
cd ~/src/shop
mise trust
mise daemons start api
mise daemons logs api
mise daemons urls
mise run test
```

`mise daemons start` installs missing tools and registers the project's generated
Pitchfork configuration. Repeat it after changing daemon definitions. Listing
URLs alone does not register a project or install its tools.

For setup such as migrations, add `init = "npm run migrate"` to the API daemon.
It runs after dependencies are ready, on every start; make it safe to repeat.
Keep short-lived build and test commands as tasks. Use `task = "dev:api"` only
when the server command already needs the structure of a mise task.

## Separate worktrees by default

Keep the default namespace isolation and use automatic ports for each checkout's
services. A linked worktree gets its own daemon IDs, database data, and ports.
Custom daemons need an explicit base, as the API above does; presets know their
default base port.

```sh
git worktree add ../shop-feature -b feature
cd ../shop-feature
mise trust
mise daemons start api
mise daemons urls
```

With the proxy configured below, the primary API has the hostname
`api.shop.localhost`; the linked checkout has `api.shop-feature.shop.localhost`.
Use `API_URL` for HTTP clients and `DATABASE_URL` for the database. An HTTP proxy
does not carry PostgreSQL or Redis traffic.

Presets with several listeners also need separate
[additional ports](/daemons.html#ports) across worktrees. If a preset option
contains a literal connection URI, mise does not rewrite its port automatically.

## Share a service deliberately

Put infrastructure shared by several applications in a separate repository,
for example `~/src/services`. Declare each service there once. Applications
reference that project rather than copy its configuration or disable worktree
isolation.

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

In `~/src/shop/mise.toml`, add the import and include it in the API's dependencies:

```toml
[daemons.events]
project = "../services"
name = "events"

[daemons.api]
run = "exec npm run dev -- --host 127.0.0.1 --port $API_PORT"
port = { auto = true, base = 3000 }
ready_cmd = "curl -fsS http://127.0.0.1:$API_PORT/health"
depends = ["db", "events"]

[env]
NATS_URL = "nats://127.0.0.1:4222"
```

Review and trust `../services`, then run `mise daemons start api` again. Mise
registers the imported service and Pitchfork starts it before the API. Relative
project paths resolve from the declaring configuration: sibling checkouts must
keep the expected layout, or use an absolute path for an intentionally shared
checkout.

Imports share process ownership, not environment variables. `NATS_URL` above is
an explicit connection to the shared service's fixed port; the imported preset's
exports remain in the services project. Stop shared infrastructure from its own
project when every consumer is finished with it.

See the [CockroachDB, SpiceDB, and NATS example](/daemons.html#example-cockroachdb-spicedb-and-nats)
for an authorization and messaging stack.

## Open a hostname to start the application

After explicitly starting each checkout once, configure Pitchfork's
[local HTTPS proxy](https://pitchfork.jdx.dev/guides/port-management#hostname-resolution).
That setup covers wildcard hostname resolution, certificate trust, and the
standard HTTPS port. Keep the supervisor running as your normal user; proxy
setup handles the privileged networking changes separately. Check it with
`pitchfork proxy doctor`.

Then stop the application stack:

```sh
mise daemons stop api db
mise daemons urls
```

Open the API URL. The HTTP request starts
its stopped dependencies, waits for readiness, and then reaches the API.
The supervisor and proxy must already be running. A DNS lookup by itself does
not start a daemon, and an unknown checkout cannot be configured by visiting
its hostname: register it with mise first.

Closing the browser does not stop the stack. Stop it explicitly with
`mise daemons stop`, or separately opt into
[shell-session lifecycle](/daemons.html#automatic-start-and-stop).
Shell sessions track shells entering and leaving projects, not browser activity.
Do not enable shell auto-stop for services you expect to remain available to a
browser after leaving the directory.

## Keep the supervisor available at login

For a machine managed through mise bootstrap, declare one user service in your
global mise configuration. Use the permanent absolute path to your installed
mise binary; replace `/opt/homebrew/bin/mise` below if it is installed elsewhere.

```toml
[tools]
pitchfork = "latest"

[bootstrap.services.pitchfork]
scope = "user"
command = "/opt/homebrew/bin/mise x -- pitchfork supervisor run --boot"
working_directory = "~"
requires_tools = true
restart = "on-failure"
```

Pin a tested Pitchfork version in your machine configuration when you need
reproducible setup. `requires_tools` makes a full bootstrap install the tool
before starting the service. The foreground `supervisor run --boot` process is
what launchd or systemd supervises; `supervisor start` would detach from it.

Preview with `mise bootstrap --dry-run`, then apply with `mise bootstrap` and
check `mise bootstrap services status`. On macOS this is a user LaunchAgent,
starting at login, not before a user logs in. `--boot` starts daemons marked
`boot_start = true`; leave that
unset on applications you want to start only on demand.

Choose one owner for login startup. If you previously used
`pitchfork boot enable`, disable that registration with `pitchfork boot disable`
and stop the existing supervisor before applying the bootstrap service. This
interrupts its running daemons, so do it when the stack can be stopped.
Do not register the same supervisor with both commands.

To prevent CLI commands from starting an unmanaged replacement, set this in
`~/.config/pitchfork/config.toml` after the managed service works:

```toml
[settings.supervisor]
auto_start = false
```

Without mise bootstrap, use
[`pitchfork boot enable`](https://pitchfork.jdx.dev/guides/boot-start) instead.
Both approaches keep the supervisor available; project definitions still belong
in their repositories.
