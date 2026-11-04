---
description: Start a daemon defined in another repository, or share one PostgreSQL, CockroachDB, or NATS server across checkouts.
socialDescription: "Use another repository's daemon, or share one database server."
---

# Share daemons across projects <Badge type="warning" text="experimental" />

A project reference runs a daemon that another repository declares, as one
process for every project that uses it. A shared server provider runs one
database or NATS server from your global config and gives each checkout its
own database or account on it.

::: warning Experimental
Daemons are experimental. Enable them with `experimental = true` under
`[settings]`; see [Requirements](/daemons.html#requirements).
:::

|             | Project reference               | Shared server provider                                          |
| ----------- | ------------------------------- | --------------------------------------------------------------- |
| Declared in | The other project's `mise.toml` | Your global config, `~/.config/mise/config.toml`                |
| Shared      | The whole process and its data  | The server; each checkout gets its own database or NATS account |
| Works with  | Any daemon                      | The `postgres`, `cockroachdb`, and `nats` presets               |
| Platforms   | All                             | Not Windows                                                     |

## Use a daemon from another project

Use `project` to run a daemon that another checkout defines, without copying
its declaration. For example, `../mirror-pipeline/mise.toml` declares a worker:

```toml
[daemons.worker]
run = "exec npm run worker"
```

Reference it from your application's `mise.toml`:

```toml
[daemons.pipeline]
project = "../mirror-pipeline"
name = "worker"

[daemons.api]
run = "exec npm run dev"
depends = ["pipeline"]
```

After reviewing the other project's config, trust it and start the API. mise
registers and starts the worker first, because `api` depends on it:

```sh
mise trust ../mirror-pipeline
mise daemons start api
```

Use the local name, `pipeline`, in commands, in `depends`, and in a task's
`daemons`. mise resolves it to the worker's full ID, so you do not need to
configure a namespace. Starting follows `depends` across projects, but `stop`
and `logs` act only on the daemons this project names, in the projects that own
them. A reference cannot be a [`[daemon_groups]`](/daemons.html#groups) member,
because a group covers only the daemons this project declares.

### Paths and configuration

`project` takes an absolute path or a path relative to the declaring project's
root. A reference in `.config/mise/config.toml` also resolves against the
project root, not the config file's directory, and an inherited reference
resolves against its parent project's root.

`name` selects the daemon in the referenced project and defaults to the local
name. The referenced project can inherit daemon declarations and settings from
its parent config files. Reference the project that defines the daemon; a
reference cannot point to another reference. A reference table accepts only
`project` and `name`, so set environment variables and other daemon options in
the project that defines the daemon.

All of the referenced project's config must be trusted, including inherited
files. mise reports the path and the `mise trust` command to run after you
review it. `mise run` and `mise daemons start` never trust another project's
config for you.

### Environment and lifecycle

The worker runs in its own project, with that project's tools, environment,
namespace, and data directories. Referencing a preset does not add its exports,
such as `DATABASE_URL`, to your application's environment; configure the
application's connection yourself.

`mise daemons start` and `restart` install missing tools in each project, then
start the selected daemons and their dependencies. The referenced project's
other daemons stay registered and can keep running on their own.
[Start and stop with your shell](/daemons.html#automatic-start-and-stop) covers
only the current project's own daemons, so start a referenced daemon with
`mise daemons start`.

### Missing or untrusted projects

If the referenced checkout is missing or untrusted, mise drops the reference
and the rest of your config keeps working, so `mise run` and `mise exec` work
for teammates who have not checked out every repository. Naming the missing
daemon, or starting a daemon that `depends` on it, fails with the expected path
or the `mise trust` command to run. Other daemon commands warn and continue, so
you can still list and stop your own daemons. mise never trusts a checkout for
you.

## Shared server providers

A provider is one PostgreSQL, CockroachDB, or NATS server that all your
checkouts share, with a separate database or NATS account for each checkout.
Use one when running a server per worktree costs too much memory or disk.
Define providers in your global config, `~/.config/mise/config.toml`:

```toml
[daemon_providers.local-postgres]
preset = "postgres"
version = "18"
```

A provider accepts `preset`, `version`, `port`, `ports`, `options`, `data_dir`,
and `tool` (to install the server from a different tool than the preset's
default). Provider names use lowercase letters, digits, and hyphens.
mise rejects a `[daemon_providers]` entry from any other config file when the
entry is used, by `mise daemons providers` or by a daemon that selects it.

A provider's `port` defaults to `"auto"`. For a provider, that always offsets
the preset's default port by a slot derived from the provider's state
directory, so the server does not collide with a project's own preset on the
default port. Set an integer `port` for a fixed one. `mise daemons providers ls --json`
shows the resolved port.

Providers are not supported on Windows; use an ordinary project preset there.

Manage providers by name:

```sh
mise daemons providers ls --json
mise daemons providers start local-postgres
mise daemons providers stop local-postgres
mise daemons providers restart local-postgres
```

Providers do not join project daemon groups or shell sessions, and they do not
stop when idle. To change a provider's settings, edit the global config and run
`mise daemons providers restart`; mise refuses to change a running provider's
configuration through another start.

By default, provider data lives in
`$MISE_STATE_DIR/daemon-providers/<name>/data`. Set `data_dir` to choose
another location; a relative path resolves inside the provider's state
directory. Removing a project or pruning deleted worktrees does not remove
provider data, and renaming a provider does not move its data.

A provider's server and readiness checks run with the provider's own tools and
a minimal environment, without the invoking project's environment, tools, or
profile. Servers listen on loopback and use the presets' local-development
authentication.

### Use a provider from a project

Keep an ordinary preset declaration when a checkout should own its server and
data. To share the server while keeping a separate database, select the
provider:

```toml
[daemons.db]
provider = "local-postgres"
```

A provider lives in your own global config, so select it from
`mise.local.toml` or a profile file such as `mise.dev.toml`, not the shared
`mise.toml`. That declaration [replaces](/daemons.html#override-a-daemon) the
project's own `db`, and teammates without the provider keep their own server.
mise never picks a provider for you, and naming a missing provider is an error.
A provider reference accepts only `provider` and `resource`; the server's
version, options, and storage belong in the global config.

mise derives a database name from the checkout's canonical path and the
daemon's name, so each worktree or unrelated project gets its own database on
the same server. Symlinked paths to the same checkout keep the same database.
Moving the checkout changes the name, and the old database stays on the
provider.

To share the database too, use the same `resource` name in each consumer:

```toml
[daemons.db]
provider = "local-postgres"
resource = "shared_app"
```

Resource names start with a lowercase letter and contain at most 63 lowercase
letters, digits, or underscores. This keeps trusted local projects apart during
development. It is not a security boundary: SQL clients use the preset's local
superuser authentication.

PostgreSQL and CockroachDB resources export the preset's usual connection
variables, pointing at the selected database. Explicit `[env]` values still
win. The provider's tool version does not become a tool requirement of the
consumer.

Starting `db`, running a task with `daemons = ["db"]`, or starting a daemon
with `depends = ["db"]` waits for the server and for the database to be
created. `mise daemons register` prepares this chain without starting the
server or creating databases, so a later request to an application's hostname
can start it.

Each consumer has a small readiness process managed by pitchfork. Stopping or
pruning a consumer stops that process and leaves the shared server and its data
in place. Starting another consumer creates its database even when the
provider's data already exists. Concurrent creation is serialized, and existing
databases are kept. mise does not run application migrations or delete
databases.

After you change a consumer's `resource`, restart its daemon.
`mise daemons ls --json` shows each daemon's `provider`, `resource`, and
`ownership`; `mise daemons providers ls --json` shows the server's port and
storage location.

### Share NATS without sharing messages

A NATS provider gives each resource its own account. Accounts have separate
subject and JetStream namespaces, so two checkouts can use the same stream and
subject names without receiving each other's messages:

```toml
# ~/.config/mise/config.toml
[daemon_providers.local-nats]
preset = "nats"
version = "2"
```

```toml
# mise.local.toml in the project
[daemons.messages]
provider = "local-nats"
```

As with SQL providers, leave out `resource` for an account per checkout, or set
the same `resource` in several consumers to share an account and its messages.
`NATS_URL` includes the account's username and password.

mise generates persistent credentials the first time it resolves a NATS
resource's connection settings, including when you inspect the environment.
Credentials and the managed server config are stored in private files under the
provider's state directory. Inspecting the environment does not start NATS or
create a live account. Daemon listings omit passwords; treat an exported
`NATS_URL` as a credential.

Starting a consumer adds its account through a validated configuration reload.
Existing accounts and their connections stay available. Restarting the provider
keeps credentials and JetStream data, and stopping a consumer does not remove
its account. `options.jetstream = false` turns off JetStream and keeps the
separate subject namespaces.

NATS providers require mise-managed, loopback-only configuration: they reject
custom configuration files and TLS or certificate authentication. Use an
ordinary NATS daemon for those setups. mise does not rewrite an existing NATS
configuration or certificate mapping.
