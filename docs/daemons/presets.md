---
description: Run PostgreSQL, Redis, CockroachDB, NATS, and SpiceDB as project daemons with built-in service presets.
socialDescription: PostgreSQL, Redis, CockroachDB, NATS, and SpiceDB as project daemons.
---

# Service presets <Badge type="warning" text="experimental" />

A service preset runs a common server for local development. It supplies the
command, the tool to install, a readiness check, a data directory, and the
connection variables your application reads.

::: warning Experimental
Daemons are experimental. Enable them with `experimental = true` under
`[settings]`; see [Requirements](/daemons.html#requirements).
:::

| Preset                        | Tool          | Default port | Named ports                           | Also needs       |
| ----------------------------- | ------------- | ------------ | ------------------------------------- | ---------------- |
| [`postgres`](#postgresql)     | `postgres`    | 5432         | None                                  |                  |
| [`redis`](#redis)             | `redis`       | 6379         | None                                  |                  |
| [`cockroachdb`](#cockroachdb) | `cockroach`   | 26257        | `http_port` 8080                      |                  |
| [`nats`](#nats)               | `nats-server` | 4222         | `monitor_port` 8222                   | `curl` on `PATH` |
| [`spicedb`](#spicedb)         | `spicedb`     | 50051        | `http_port` 8443, `metrics_port` 9090 | `curl` on `PATH` |

NATS and SpiceDB use `curl` for their HTTP readiness checks.

::: warning Local development only
The presets listen on loopback addresses and use either no authentication or a
fixed development key. Use them only on a trusted local machine. For shared or
untrusted environments, write a custom daemon with proper authentication and
network restrictions.
:::

## Declare a preset

Name the entry after the preset and give a version request:

```toml
[daemons]
postgres = "18"
redis = "8"
```

Use a table to run a second instance of a preset, or to change its port,
options, or data directory. The table's name becomes the daemon's name:

```toml
[daemons.analytics]
preset = "postgres"
version = "18"
port = 5433
options.database = "analytics"
```

A preset installs its server from the tool in the table above. Set `tool` to
install it from a different tool instead. See [Daemon keys](/daemons.html#daemon-keys)
for every key a preset accepts.

Presets get no proxy hostname, because their clients do not speak HTTP. See
[Per-daemon proxy settings](/daemons/worktrees.html#per-daemon-proxy-settings)
to change that.

## Connection variables and tool versions

Each preset exports connection variables, listed in its section below. They are
available through `mise env`, `mise exec`, and tasks. Explicit `[env]` values
override a preset's exports. If several instances export the same variable, the
last declaration wins; use `[env]` to choose the instance your application uses.

An explicit `[tools]` version must satisfy the preset's version request. For
example, `postgres = "18.1"` in `[tools]` satisfies a preset requesting `"18"`.
Instances that share a tool must use the same version request.

## Ports

Each preset listens on its default port, and on its named ports if it has any.
Set `port` for a different port, `port = "auto"` to give each Git worktree its
own, or `ports.<name>` to move a named port. See [Ports](/daemons/worktrees.html#ports).

## Preset options

Set options on a preset instance, for example `options.database = "app"`. File
paths resolve relative to the project root, and a leading `~/` expands to your
home directory. mise checks option types and formats when it loads the config.
Database names can contain only letters, numbers, and underscores.

Options used during first initialization, such as database names and
CockroachDB cluster settings, do not change existing data when you change them.
SpiceDB migrations run on every start and use the current datastore options.

### PostgreSQL

PostgreSQL uses the `postgres` user with local trust authentication. Set
`options.database` to create a database during first initialization; it
defaults to `postgres`. Changing it later does not create another database in
an existing cluster.

PostgreSQL does not run as root. Run mise as a regular user to start it, for
example with `USER` in a container image. As root, starting a PostgreSQL daemon
or provider fails before mise installs anything.

Exports: `PGHOST`, `PGPORT`, `PGUSER`, `PGDATABASE`, and `DATABASE_URL`.

### Redis

Redis enables append-only persistence and has no options.

Exports: `REDIS_URL`.

### CockroachDB

CockroachDB runs a single insecure node with the `root` database user.

| Option       | Default       | Purpose                                                          |
| ------------ | ------------- | ---------------------------------------------------------------- |
| `database`   | `"defaultdb"` | Database named in exported connection strings                    |
| `databases`  | `[]`          | Databases to create during first initialization                  |
| `settings`   | `[]`          | Cluster setting assignments to apply during first initialization |
| `locality`   | `""`          | Node locality passed to `--locality`                             |
| `max_offset` | `""`          | Clock offset limit passed to `--max-offset`, such as `"500ms"`   |

`database` selects the database in the connection strings; it does not create
one. Include the name in `databases` to create it. Each `settings` entry is an
assignment such as `"sql.defaults.vectorize = 'off'"`; mise prefixes it with
`SET CLUSTER SETTING`.

A `databases` entry can set a primary region with `name=region`. Declare the
same region in `locality` so it exists during initialization:

```toml
[daemons.crdb]
preset = "cockroachdb"
version = "26"
options.database = "app"
options.databases = ["app=us-east-2"]
options.locality = "region=us-east-2"
```

Exports: `COCKROACH_HOST`, `COCKROACH_URL`, and `DATABASE_URL`.

### NATS

NATS enables JetStream by default and stores its data in the daemon's data
directory.

| Option                | Default | Purpose                                                    |
| --------------------- | ------- | ---------------------------------------------------------- |
| `jetstream`           | `true`  | Enable JetStream when no configuration file is supplied    |
| `config`              | `""`    | Path to a NATS configuration file                          |
| `tls_cert`, `tls_key` | `""`    | Paths to the server certificate and private key            |
| `tls_ca`              | `""`    | Path to a CA certificate for verifying client certificates |

When `config` is set, that file controls whether JetStream is on and where it
stores data. mise then passes no JetStream or storage flags, and the
`jetstream` option has no effect.

Set both `tls_cert` and `tls_key` to enable TLS. `tls_ca` requires those two
and turns on client certificate verification.

Exports: `NATS_URL` and `NATS_MONITORING_URL`. With TLS on, `NATS_URL` uses the
`tls://` scheme so clients connect over TLS.

### SpiceDB

SpiceDB uses an in-memory datastore by default, so its authorization data is
lost when the process stops. To keep it, set both a `datastore_engine` other
than `memory` and a `datastore_uri`. Setting only one is an error.

| Option             | Default          | Purpose                                  |
| ------------------ | ---------------- | ---------------------------------------- |
| `datastore_engine` | `"memory"`       | Backing datastore engine                 |
| `datastore_uri`    | `""`             | Connection URI for the backing datastore |
| `datastore_daemon` | `""`             | Local daemon to wait for before starting |
| `preshared_key`    | `"mise-dev-key"` | gRPC preshared key                       |

With a persistent datastore, mise runs `spicedb migrate head` before every
start. Set `datastore_daemon` when another daemon hosts the datastore, as in
the [example below](#example-cockroachdb-spicedb-and-nats). It sets the start
order only: `datastore_uri` is a literal string, so it does not follow that
daemon's [automatic port](/daemons/worktrees.html#automatic-ports). If the port
changes, update the URI too.

Exports: `SPICEDB_ENDPOINT` and `SPICEDB_PRESHARED_KEY`.

## Example: CockroachDB, SpiceDB, and NATS

This config stores application data and SpiceDB's authorization data in
separate CockroachDB databases, with NATS for messaging:

```toml
[settings]
experimental = true

[daemons.crdb]
preset = "cockroachdb"
version = "26"
options.database = "app"
options.databases = ["app", "spicedb"]

[daemons.spicedb]
preset = "spicedb"
version = "1"
options.datastore_engine = "cockroachdb"
options.datastore_uri = "postgresql://root@127.0.0.1:26257/spicedb?sslmode=disable"
options.datastore_daemon = "crdb"

[daemons.events]
preset = "nats"
version = "2"
```

Run `mise daemons start` to start all three. On first initialization, mise
creates the `app` and `spicedb` databases. Once CockroachDB is ready, mise runs
`spicedb migrate head` and then starts SpiceDB. The migration runs on every
start, so it applies pending schema changes after an upgrade or a datastore
reset.

`options.database = "app"` selects the database in the exported `DATABASE_URL`
and `COCKROACH_URL`; `options.databases` lists the databases to create.

## Windows

- The `redis` preset has no Windows build.
- The other presets run under pitchfork's default `cmd /C` shell. A different
  `windows_shell` is not supported.
- PostgreSQL stops cleanly only with pitchfork 2.29.0 or later, which sends it
  Ctrl+C. Older releases terminate it, and it recovers on the next start.
- PostgreSQL refuses to run with administrative rights. Do not start it from
  an elevated prompt, or from a supervisor running as an administrator.
- Windows reserves some port ranges for Hyper-V and WSL. List them with
  `netsh interface ipv4 show excludedportrange protocol=tcp`, and if a preset's
  port falls in one, set `port` or `ports` to another.
- [Shared server providers](/daemons/sharing.html#shared-server-providers) are
  not available on Windows.
