---
description: Run the same daemons in several Git worktrees at once, with separate daemon IDs, ports, and stable HTTPS hostnames.
socialDescription: Separate daemon IDs, ports, and HTTPS hostnames for each Git worktree.
---

# Ports, URLs, and worktrees <Badge type="warning" text="experimental" />

Run the same daemons in several Git worktrees of one project at once. Each
checkout gets its own daemon IDs, ports, and data, and pitchfork's proxy gives
each HTTP daemon a hostname that stays the same when its port changes.

::: warning Experimental
Daemons are experimental. Enable them with `experimental = true` under
`[settings]`; see [Requirements](/daemons.html#requirements).
:::

With `namespace = "shop"` and `port = { auto = true, base = 3000 }`, an `api`
daemon looks like this in the primary checkout and in a linked worktree:

|                | Primary checkout `~/src/shop` | Linked worktree `~/src/shop-pr-42`      |
| -------------- | ----------------------------- | --------------------------------------- |
| Daemon ID      | `shop/api`                    | `shop-<hash>/api`                       |
| Port           | `3000`                        | One of `3001` to `3511`, from its path  |
| URL            | `https://api.shop.localhost`  | `https://api.shop-pr-42.shop.localhost` |
| State and data | Its own directory             | Its own directory                       |

## Namespaces

A daemon's full ID is `<namespace>/<name>`. Unless the project sets a namespace
or has its own pitchfork configuration file, mise derives the namespace from
the project directory name and a hash of its path to separate checkouts (see
the order below). Set `namespace` to give daemons predictable IDs that other
projects can use in `depends`:

```toml
[daemons_settings]
namespace = "services"

[daemons.db]
preset = "postgres"
version = "18"
```

In the primary checkout, the database's ID is `services/db`. A daemon in
another project can use `depends = ["services/db"]` once that database is
registered with pitchfork. Use a
[`project` reference](/daemons/sharing.html#use-a-daemon-from-another-project) when
mise should also load and register the other project's config.

Namespaces follow the same rules as daemon names: ASCII letters, numbers, `.`,
`_`, and `-`, not empty or `.`, with no leading or trailing `-`, and no `..` or
`--`.

mise chooses the namespace in this order:

1. `namespace` in `[daemons_settings]`, with a
   [suffix](#namespaces-in-git-worktrees) in a linked worktree unless
   `namespace_per_worktree = false`.
2. `namespace` in the project's own pitchfork configuration, checked in
   `pitchfork.local.toml`, `pitchfork.toml`, `.config/pitchfork.local.toml`,
   then `.config/pitchfork.toml`.
3. The project directory's name, when one of those files exists without a
   `namespace`.
4. Otherwise, `<directory>-<hash>`.

Stop the project's daemons before you change its namespace. The next start
adopts the new namespace and removes the old IDs from mise's state.

### Namespace inheritance

`[daemons_settings]` merges key by key across config files, so setting only
`namespace_per_worktree` in `mise.local.toml` keeps the `namespace` from
`mise.toml`. Child projects inherit daemon settings from their parent's config.
If several projects inherit one namespace, give their daemons different names
or set a namespace in each project. mise rejects duplicate IDs among the
projects it loads, but it cannot detect a collision with a project outside that
hierarchy and its references.

Global and system config cannot set `[daemons_settings]`; mise ignores the
table there with a warning.

### Namespaces in Git worktrees

A linked Git worktree adds a suffix to a `[daemons_settings]` namespace; a
namespace read from a pitchfork file is used unchanged in every worktree. With
`namespace = "services"`, the primary checkout uses `services` and a linked
worktree uses `services-<hash>`, where `<hash>` is a hexadecimal hash (up to 16
characters) of the project root's path in that worktree. Each checkout has
separate daemon IDs and state.

A literal dependency on `services/db` always means that exact ID; it does not
follow the current worktree's suffix. Use a local daemon name or a `project`
reference when the dependency should resolve within a particular checkout.

To share one namespace across worktrees, turn the suffix off:

```toml
[daemons_settings]
namespace = "services"
namespace_per_worktree = false
```

Do this only when you mean to share daemon IDs. Starting the same daemons from
several worktrees at once then makes them collide.

## Ports

### Fixed ports

Set `port` to an integer for a port that is the same in every checkout. mise
does not search for a free port or pick another one, so every port you
configure must be free when the daemon starts. A custom daemon also accepts
pitchfork's structured `port` table, which mise passes on unchanged; a preset
accepts only an integer or an [automatic port](#automatic-ports).

### Named ports

Presets with more than one listener have named ports, listed in the
[preset table](/daemons/presets.html). Override one with `ports.<name>`,
alongside the primary port if you like:

```toml
[daemons.crdb]
preset = "cockroachdb"
version = "26"
port = 26258
ports.http_port = 8081
```

A daemon's primary port and named ports must all differ. With `port = "auto"`,
the named ports move with the primary port, so each checkout keeps a complete
set. A named port you set yourself is used exactly as written.

mise exports each named port as `<NAME>_<PORT_NAME>`, with the daemon's name
folded as for [port variables](#port-variables): a `crdb` daemon from the
`cockroachdb` preset exports `CRDB_HTTP_PORT`, and a `spicedb` daemon named
`authz` exports `AUTHZ_HTTP_PORT` and `AUTHZ_METRICS_PORT`. The value is the
port the daemon uses, so read it instead of adding an offset yourself:

```toml
[daemons.crdb]
preset = "cockroachdb"
version = "26"
port = "auto"

[env]
COCKROACH_CONSOLE = "http://127.0.0.1:{{ env.CRDB_HTTP_PORT }}"
```

### Automatic ports

Use `port = "auto"` to run the same daemon in the primary checkout and in
linked worktrees without assigning ports by hand:

```toml
[daemons.postgres]
preset = "postgres"
version = "18"
port = "auto"
```

The primary checkout uses the preset's default port, `5432`. In a linked
worktree, mise adds an offset derived from the project root's path. Connection
variables such as `PGPORT` and `DATABASE_URL` follow the resolved port, so the
application's config works unchanged in each checkout.

mise resolves automatic ports when it loads the config, so `mise env` and
`mise exec` show them before a daemon starts. It does not search for a free
port or move the port when it is taken; see [Port conflicts](#port-conflicts).

A custom daemon has no default port, so use the table form and give it a
`base`:

```toml
[daemons.api]
run = "exec npm run dev -- --port $API_PORT"
port = { auto = true, base = 3000 }
```

mise exports the resolved port as `API_PORT`; see
[Port variables](#port-variables). This example assumes the application's `dev`
script accepts `--port`. The primary checkout uses `3000`, and linked worktrees
use ports from `3001` through `3511`. A fixed `ready_port` does not follow an
automatic port, so leave it out or use a readiness check that reads the
resolved port.

| Option   | Meaning                                   | Default                                                |
| -------- | ----------------------------------------- | ------------------------------------------------------ |
| `auto`   | Turns on automatic ports. Must be `true`. | Required in the table form                             |
| `base`   | Port for the primary checkout             | The preset's default port; required for custom daemons |
| `stride` | Distance between worktree slots           | `1`                                                    |

There are 511 worktree slots. With the default stride, PostgreSQL uses `5433`
to `5943` in linked worktrees and Redis uses `6380` to `6890`. For a service
that binds a range of consecutive ports, set `stride` to the size of the range,
for example `port = { auto = true, base = 3000, stride = 10 }`. That keeps
different slots ten ports apart; it does not stop two projects from landing on
the same slot. Loading the config fails if a resolved port would exceed
`65535`.

### Port variables

A custom daemon with an integer or automatic `port` exports `<NAME>_PORT`.
mise uppercases the daemon name and replaces punctuation with underscores:
`[daemons.api]` exports `API_PORT`, and `[daemons.web-ui]` exports
`WEB_UI_PORT`. A preset exports its own connection variables with the resolved
port instead, such as `PGPORT` and `REDIS_URL`. The variables are available
through `mise env`, `mise exec`, and the daemon's own environment. Explicit
`[env]` values take precedence over them.

If a daemon name starts with a digit, or two names fold to the same variable
(`web-ui` and `web_ui`), mise warns and exports no `<NAME>_PORT` or
`<NAME>_URL` for them. The daemons still run, and pitchfork still sets `$PORT`
in the process. A preset's own variables win over a derived one, so a custom
daemon whose `<NAME>_PORT` matches a preset's named-port variable gives way to
the preset.

### Project layout and port stability

mise finds the enclosing checkout even when `mise.toml` is nested in a
directory such as `packages/api`. Each project root inside a linked worktree
gets its own slot. Submodules follow their enclosing checkout: they get an
offset inside a linked worktree and keep the base port inside a primary
checkout.

Independent clones, including `git clone --separate-git-dir`, and projects
outside Git keep the base port. Worktrees of a bare repository all get offsets,
because there is no primary checkout. To give one checkout a fixed port,
[redeclare the daemon](/daemons.html#override-a-daemon) with an integer `port`
in a gitignored `mise.local.toml`.

Once a daemon is registered, it keeps its automatic port across mise upgrades.
Changing `base` or `stride` assigns a new port; restart the daemon afterward.
`mise daemons ls --json` shows each daemon's `port`, and `port_auto` is `true`
for an automatic one.

### Port conflicts

Two daemons in one project cannot share a port. mise reports it when it loads
the config, instead of letting the second fail to bind. Two instances of one
preset usually cause this, because they share a base port. Give the second its
own `port`, or its own `base` when both use `port = "auto"`:

```toml
[daemons]
postgres = "18"

[daemons.analytics]
preset = "postgres"
version = "18"
port = { auto = true, base = 5500 }
```

Automatic ports come from paths, so different projects can get the same port.
When another mise project has a running daemon on that port, the start fails
with an error that names the daemon and its project root. Change one project's
`base`, or stop the other daemon, and start again.

Stopped daemons do not hold their ports. The check covers only the daemons
being started, so `mise daemons start redis` is not blocked by a conflict on
this project's PostgreSQL port. It applies to fixed ports as well as automatic
ones.

The check does not reserve ports or see every listener. An unmanaged process,
an unreachable supervisor, or two projects starting at the same moment can
still cause an ordinary bind failure. mise keeps the selected port rather than
trying another, so open shells keep the same connection settings.

If the port is already taken, pitchfork fails to start the daemon. For an
automatic port, mise adds a warning that names the daemon and the port and says
whether the port is the base or a worktree offset. To use another port in this
checkout, redeclare the daemon in a gitignored `mise.local.toml` with a fixed
`port` or a different `base`, repeating its other keys:

```toml
# mise.local.toml
[daemons.api]
run = "exec npm run dev -- --port $API_PORT"
port = 3100
```

## Stable URLs per worktree

Separate ports mean each service has to be told where its neighbors are.
pitchfork's reverse proxy removes that step for HTTP services: it routes a
stable hostname to whatever port the daemon bound, and mise derives the same
hostname when it loads the config. The proxy must be enabled in pitchfork; see
[Before you start](/daemons/development-stack.html#before-you-start) in the
development stack guide.

A daemon gets a hostname when it has a `port` and has not set `proxy = false`:

```text
<daemon>.<project>.<tld>              in the primary checkout
<daemon>.<worktree>.<project>.<tld>   in a linked Git worktree
```

The daemon component is the daemon's name. The project component is the
`[daemons_settings] namespace` when set, otherwise the primary checkout's
directory name (see [Choose hostname labels](#choose-hostname-labels)). A
linked worktree adds a component of its own. A project with `namespace = "shop"`
checked out at `~/src/shop` serves its `api` daemon at
`https://api.shop.localhost`, and a linked worktree at `~/src/shop-pr-42`
serves it at `https://api.shop-pr-42.shop.localhost`. Both can run at once, and
neither URL changes when a port moves.

mise exports the URL as `<NAME>_URL`, next to `<NAME>_PORT` and with the same
naming rules, so another service can use it without knowing the port:

```toml
[daemons.api]
run = "exec npm run dev -- --port $API_PORT"
port = { auto = true, base = 3000 }

[env]
APP_BASE_URL = "{{ env.API_URL }}"
```

When every HTTP service reaches its neighbors this way, several worktrees of
one project can run at once without a table of ports. Databases keep
`port = "auto"` instead: the proxy speaks HTTP and database clients do not, so
every preset opts out of the proxy and keeps exporting its own connection
variables, such as `PGPORT`, `DATABASE_URL`, and `REDIS_URL`.

A daemon without a `port`, or with `proxy = false`, gets no hostname and no
`<NAME>_URL`. `mise daemons urls` still lists it, without a URL, and shows its
port when it has one.

### Per-daemon proxy settings

A daemon can use a different hostname label, or have no hostname:

```toml
# https://front.shop.localhost, not https://web.shop.localhost
[daemons.web]
run = "exec npm run dev"
port = 5173
proxy = "front"

# No hostname and no WORKER_URL; reachable only on its port.
[daemons.worker]
run = "exec npm run worker"
port = 9000
proxy = false
```

Set `proxy = false` on a custom daemon that does not speak HTTP. The proxy
serves HTTP, so a custom Redis daemon named `redis` would otherwise get an
`https://` hostname and a `REDIS_URL` pointing at it, which is not what a Redis
client expects.

`proxy = true` gives a preset a hostname, using the daemon's name as the label.
The preset still exports no `<NAME>_URL`.

`proxy_tls` sets what the proxy does with TLS. The default, `"terminate"`,
serves HTTPS at the proxy and forwards plain HTTP to the daemon. Use
`"passthrough"` when the daemon serves TLS itself and the connection should
reach it unbroken:

```toml
[daemons.api]
run = "exec npm run dev:https"
port = 3000
proxy_tls = "passthrough"
```

Both keys go to pitchfork unchanged and need pitchfork 2.26.0 or later. An
older supervisor starts the daemons but does not serve their hostnames.

### Choose hostname labels

The project label is the `[daemons_settings] namespace` without its worktree
suffix, or the primary checkout's directory name if you set none. A namespace
read from a pitchfork file is not used. mise passes the project label to
pitchfork when it registers the project. If the installed pitchfork cannot take
a label, set `namespace` explicitly, or the URLs mise prints do not route.

The worktree label is the linked worktree's directory name. To choose it
yourself, set `worktree_label` in a pitchfork config file inside that worktree,
which is where pitchfork reads it:

```toml
# pitchfork.local.toml, in the worktree
worktree_label = "pr-42"
```

mise reads the key from the same place, so the URL it exports and the hostname
the proxy serves agree. Use `pitchfork.local.toml` and gitignore it: a tracked
file is shared by every checkout, and each worktree needs a different label.

If the project sets no `[daemons_settings] namespace` and has no other
pitchfork file, this file also changes the worktree's namespace from
`<directory>-<hash>` to its directory name, as step 3 of
[the namespace order](#namespaces) describes. Stop the worktree's daemons before
you add it.

Labels are folded to lowercase letters, digits, and `-`. When two daemons,
worktrees, or projects fold to the same hostname, pitchfork routes none of
them, so mise exports no URL for them and warns. The daemons still run on their
ports. If exactly one of the claimants is a daemon this project reaches through
a `project` reference, its own project registers it, so pitchfork serves that
one and the local daemons get no URL; the warning names it.

Worktrees of a bare repository have no primary checkout, so each one is
labeled by its own directory name and has no worktree component: a daemon in
`shop/main` is `api.main.localhost`, not `api.main.shop.localhost`. Do not put
`namespace` in a config file these worktrees share, or they all get the same
hostname, which `worktree_label` cannot separate; mise warns when it sees this.
Set a namespace per worktree in a gitignored `mise.local.toml` instead:

```toml
# mise.local.toml, in one worktree of the bare repository
[daemons_settings]
namespace = "shop-pr-42"
```

### Register for on-demand startup

Run [`mise daemons register`](/cli/daemons/register.html) in each checkout to
tell pitchfork about its daemons without starting them. It installs missing
tools, checks the definitions and their dependencies, and includes daemons
outside the `default` group and the daemons from other projects they depend on.
Daemons that are already running keep running; registering does not restart
them or initialize database data.

With the supervisor running and its proxy enabled, the first request to a
registered hostname starts that daemon and its dependencies. Listing URLs does
not register the project. The
[development stack guide](/daemons/development-stack.html) walks through the
whole setup.

### List URLs

[`mise daemons urls`](/cli/daemons/urls.html) prints each daemon's hostname
next to the port it binds, grouped by project root, followed by the pages
pitchfork serves for the whole stack. Take this config, checked out in a linked
worktree at `~/src/shop-pr-42` with the `worktree_label = "pr-42"` from above:

```toml
[daemons_settings]
namespace = "shop"

[daemons.api]
run = "exec npm run dev -- --port $API_PORT"
port = { auto = true, base = 3000 }
proxy_tls = "passthrough"

[daemons.web]
run = "exec npm run dev"
port = { auto = true, base = 5173 }
proxy = "front"

[daemons.postgres]
preset = "postgres"
version = "18"
port = "auto"
```

```text
~/src/shop-pr-42
Daemon                          URL                                 Port  Proxy        Status
shop-33110356f24e9076/api       https://api.pr-42.shop.localhost    3187  passthrough  running
shop-33110356f24e9076/web       https://front.pr-42.shop.localhost  5360  terminate    running
shop-33110356f24e9076/postgres  -                                   5619  off          running
  stack:   https://pr-42.shop.localhost
  project: https://shop.localhost
```

The ID suffix is the worktree's
[namespace hash](#namespaces-in-git-worktrees), and all three daemons use the
same worktree slot, 187. The Proxy column shows the daemon's `proxy_tls` mode,
or `off` when it has no hostname; a database still appears, with its port. The
primary checkout prints no stack page, because its stack is the project.
`mise daemons ls --json` has the same information in its `host`, `url`, and
`proxy` fields.

### Stop idle daemons

Opening a daemon's URL starts it, and its `depends`, if it is not running. By
default nothing stops it again, because pitchfork's idle shutdown is off. Set
`proxy_idle_timeout` to have pitchfork stop the daemon after that long without
proxy traffic:

```toml
[daemons.web]
run = "exec npm run dev"
port = 5173
depends = ["db"]
proxy_idle_timeout = "30m"

[daemons.db]
preset = "postgres"
version = "18"
```

Visiting `https://web.shop.localhost` then starts `db` and `web`. Thirty
minutes after the last request, `web` stops, then `db` if nothing else needs
it, and the next visit starts both again.

| `proxy_idle_timeout`     | Effect                                                                                                                     |
| ------------------------ | -------------------------------------------------------------------------------------------------------------------------- |
| Not set                  | Uses the timeout of the daemon whose request started this one, otherwise pitchfork's `proxy.idle_timeout` (off unless set) |
| `"30m"`, `"1h"`, `"90s"` | Stops the daemon after that long without proxy traffic                                                                     |
| `false` or `"0"`         | Never stops the daemon for inactivity, even as a dependency of a daemon with a timeout or when `proxy.idle_timeout` is set |

To turn on idle shutdown for every proxy-started daemon, set pitchfork's
`proxy.idle_timeout`, for example with `PITCHFORK_PROXY_IDLE_TIMEOUT=15m` or
`idle_timeout = "15m"` under `[settings.proxy]` in pitchfork's user config. A
dependency that is already running keeps the timeout it started with.

Only daemons the proxy started stop this way. A daemon you start with
`mise daemons start`, the shell hook, or `boot_start`, and its dependencies,
keep running however long they sit idle. A request counts as activity until
its response finishes, and an open WebSocket, streaming response, or TLS
passthrough connection keeps the daemon active, so a browser tab holding a
hot-reload socket keeps a dev server up. Traffic sent straight to the daemon's
port does not count, so set `false` on a daemon that clients reach directly.

pitchfork checks for idle daemons every `general.interval`, and it stops a
daemon only when no running daemon depends on it and no tracked shell session
needs it, so a daemon can outlive its timeout a little. See pitchfork's
[idle shutdown](https://pitchfork.jdx.dev/guides/port-management#idle-shutdown)
rules.

### Where the scheme and port come from

mise builds each URL the way pitchfork does: the scheme follows `proxy.https`,
the TLD follows `proxy.tld`, and the port appears only when it is not the
standard one for the scheme. mise reads these settings from
`/etc/pitchfork/config.toml`, from the user file
`~/.config/pitchfork/config.toml` (`$PITCHFORK_CONFIG_DIR/config.toml` when
that variable is set), and from `PITCHFORK_PROXY_*` environment variables,
which take precedence. A project's own `[settings.proxy]` in a pitchfork file
is ignored. In pitchfork's LAN mode (`proxy.lan = true` or a `proxy.lan_ip`),
the TLD is `local`.

Without any pitchfork config, mise uses pitchfork's defaults: `https://` on
port 443 under `.localhost`. `mise env` therefore exports a URL even before the
proxy is turned on. See pitchfork's
[port management guide](https://pitchfork.jdx.dev/guides/port-management) to
enable the proxy and trust its certificate.
