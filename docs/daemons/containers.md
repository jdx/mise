---
description: Run a container image with Docker or Podman as a project daemon, with per-worktree ports, stale-container cleanup, and clean shutdown.
socialDescription: Docker and Podman containers as project daemons.
---

# Container daemons <Badge type="warning" text="experimental" />

A container daemon runs an image with Docker or Podman instead of a tool mise
installs. It is declared with `image` and otherwise behaves like any other
daemon: it has [ports](/daemons/worktrees.html#ports) that follow the worktree,
exports their variables, waits for a readiness check, and is started and
stopped with `mise daemons` and [`daemons` tasks](/daemons.html#tasks-that-require-daemons).

::: warning Experimental
Daemons are experimental. Enable them with `experimental = true` under
`[settings]`; see [Requirements](/daemons.html#requirements).
:::

## Declare a container

```toml
[daemons.postgres]
image = "postgres:18"
port = { auto = true, base = 5432 }
env = { POSTGRES_PASSWORD = "dev" }
data_mount = "/var/lib/postgresql"
ready_cmd = "{{ runtime }} exec {{ container }} pg_isready -U postgres"
```

In the primary checkout, `mise daemons start postgres` publishes the container's
port `5432` on `127.0.0.1:5432`. In a linked worktree the host port is the
worktree's slot added to `base`, for example `5471`, and the container still
listens on `5432`. The application reads `POSTGRES_PORT` in both. See [Automatic ports](/daemons/worktrees.html#automatic-ports) for how
slots work.

mise runs the container in the foreground with `--rm`, so its output is the
daemon's log (`mise daemons logs postgres`) and the container is removed when it
stops.

## Keys

| Key              | Meaning                                                                                                                                                                       |
| ---------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `image`          | The image to run. This is what makes the daemon a container; it cannot be combined with `run`, `task`, or `preset`.                                                           |
| `command`        | The container's command and arguments, replacing the image's default. A string is shell text, as `run` is; an array is passed as-is.                                          |
| `port`           | The host port for the container's main port: an integer, or `{ auto = true, base = 5432 }`. `"auto"` alone needs a `base`, because an image has no default port.              |
| `container_port` | The port the container listens on for `port`. Defaults to `base` for an automatic port and to the port itself for a fixed one.                                                |
| `ports`          | More published ports by name. See [Several ports](#several-ports).                                                                                                            |
| `env`            | Variables passed into the container. Values are set in the daemon's environment and given to the runtime by name, so they stay off the command line.                          |
| `volumes`        | `source:target[:options]` mounts. A source starting with `.`, `/` or `~` is a path (relative to the project root) and is created if missing; anything else is a named volume. |
| `data_mount`     | A path inside the container to keep data at. See [Keep data](#keep-data).                                                                                                     |
| `data_dir`       | The host directory for `data_mount`, relative to the project root or absolute.                                                                                                |
| `runtime`        | `"auto"` (the default), `"docker"` or `"podman"`. See [Docker and Podman](#docker-and-podman).                                                                                |
| `runtime_args`   | Extra arguments for `<runtime> run`, such as `["--shm-size=1g"]`, placed before the image.                                                                                    |

`init`, `depends`, `ready_port`, `ready_cmd`, `proxy`, and the other
[daemon keys](/daemons.html#daemon-keys) work as they do on a custom daemon.
`init` runs on the host before the container starts. Like any custom daemon with
a port, a container is given a [proxy hostname](/daemons/worktrees.html#stable-urls-per-worktree);
set `proxy = false` for a database or broker that does not speak HTTP.

## Several ports

Give each additional published port a name. An integer publishes the same
number on the host; a table sets the container port and a fixed host port, or
turns on the same per-worktree allocation as `port`:

```toml
[daemons.cockroach]
image = "cockroachdb/cockroach:v25.1.0"
command = ["start-single-node", "--insecure"]
port = { auto = true, base = 26257 }
ports.http = { container = 8080, auto = true, base = 27000 }
ready_cmd = "{{ runtime }} exec {{ container }} cockroach sql --insecure -e 'select 1'"
```

An automatic named port uses the same slot as `port` (or the project root's slot
when `port` is fixed), and its `base` defaults to the container port. With a
slot of 7, this daemon publishes `26264` and `27007`.

Each port is exported as `<NAME>_PORT` for `port`, and `<NAME>_<PORT>_PORT` for a
named one: `COCKROACH_PORT` and `COCKROACH_HTTP_PORT` here. Every published port
must be different. Ports are TCP and bound to `127.0.0.1` only, so nothing on
your network can reach a container daemon.

## Readiness

A published port opens on the host as soon as the container starts, before the
server inside it can answer, so a `ready_port` check can pass early. Prefer a
`ready_cmd` that asks the server. `ready_cmd` and `health_cmd` can
use these variables, which mise fills in; other templates, such as
pitchfork's <code v-pre>{{ name }}</code>, are left for pitchfork. `runtime` is a shell
expression, so use it in the string form of a command or as the script of
`["sh", "-c", "..."]`, not as another argument of an argument array:

| Variable                           | Value                                                |
| ---------------------------------- | ---------------------------------------------------- |
| <code v-pre>{{ container }}</code> | The container's name                                 |
| <code v-pre>{{ runtime }}</code>   | The runtime to call, as the daemon resolved it       |
| <code v-pre>{{ port }}</code>      | The host port of `port`                              |
| <code v-pre>{{ NAME }}</code>      | The host port of the named port `NAME` under `ports` |

For example, <code v-pre>{{ runtime }} exec {{ container }} pg_isready</code> asks
the server in the container, and <code v-pre>pg_isready -h 127.0.0.1 -p {{ port }}</code>
asks through the published port.

## Keep data

`data_mount` stores a path of the container on the host. Without `data_dir`, the
directory is in mise's [state directory](/daemons/data.html#where-data-lives),
which is per project root, so each worktree has its own, and
`mise daemons prune` removes it with the project. Set `data_dir` to keep it in
the checkout instead, as for [presets](/daemons/data.html#keep-data-in-the-checkout):

```toml
[daemons.postgres]
image = "postgres:18"
port = { auto = true, base = 5432 }
data_mount = "/var/lib/postgresql"
data_dir = ".data/postgres"
```

For other mounts use `volumes`. On Linux, a directory Docker creates itself is
owned by root; mise creates path sources first, so they belong to you. The image
may still need `runtime_args = ["--user", "..."]` to write them.

## Worktrees and cleanup

The container is named `mise-<namespace>-<daemon>-<hash>`, where the
[namespace](/daemons/worktrees.html#namespaces) carries the project and, in a
linked worktree, the worktree, and `<hash>` is a short hash of the exact
namespace and daemon name. Two checkouts, or two daemons whose names only read
alike, therefore never share or fight over a container name, and each container is labelled `dev.mise.daemon` and
`dev.mise.project` so you can find it with `docker ps --filter label=...`.

When the daemon stops, mise runs `<runtime> stop` for the container and waits
for it to exit, so the image's shutdown signal and grace period apply. If a
previous run was killed rather than stopped and left its container behind, the
next start removes the container with that name first. Nothing else is removed.
Stopping a daemon while the image is still downloading cancels the start.

## Docker and Podman

`runtime = "auto"` uses `docker` if it is on `PATH`, and otherwise `podman`.
Set `runtime = "podman"` to pick one in the project. To choose per developer or
use a docker-compatible CLI such as `nerdctl`, set `MISE_CONTAINER_RUNTIME` to
its name or path; it overrides the declaration. mise looks the runtime up when
the daemon starts, inside the project's tool environment, so a runtime
installed by `[tools]` works. If it is missing, the start fails and the daemon
log says so.

The runtime is not started for you: Docker's or Podman's own service (Docker
Desktop, `podman machine`) must be running.

## Limitations

- Windows is not supported. The container's command is a POSIX shell script; on Windows the configuration still loads, and starting the daemon fails with a message.
- Only TCP ports are published, and only on `127.0.0.1`.
- Container daemons are not [shared servers](/daemons/sharing.html#shared-server-providers),
  and mise does not pull or build images; `docker run` pulls the image on first
  use.
- Variables from `[env]` are not passed into the container. Set `env` on the
  daemon.
- Stopping gives the container a ten second grace period before the runtime
  kills it. mise sets the daemon's `stop_signal` timeout to 15 seconds so
  pitchfork waits for that; a `stop_signal` you set yourself is kept, and should
  be longer than ten seconds.
