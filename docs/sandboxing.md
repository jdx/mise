---
description: "Restrict the files, network access, and environment variables available to commands that mise exec and mise run start."
socialDescription: "Restrict files, network, and environment for commands run by mise exec and mise run."
---

# Sandboxing

[`mise exec`](/cli/exec.html) and [`mise run`](/cli/run.html) can limit what
the command they start may read, write, reach on the network and see in its
environment. Add a `--deny-*` or `--allow-*` flag, or set the matching property
on a task.

The sandbox covers only that command. mise loads config, installs tools and runs
hooks outside it. To load config you have not reviewed, use
[safe mode](/security.html#safe-mode) instead.

## Platform support

| Restriction                                                                       | Linux                                   | macOS                                   | Windows          |
| --------------------------------------------------------------------------------- | --------------------------------------- | --------------------------------------- | ---------------- |
| Reads and writes (`--deny-read`, `--deny-write`, `--allow-read`, `--allow-write`) | Landlock, Linux 5.13 or later           | Seatbelt (`sandbox-exec`)               | Not enforced     |
| All network access (`--deny-net`)                                                 | seccomp, on x86_64 and arm64            | Seatbelt                                | Not enforced     |
| Access to particular hosts (`--allow-net`)                                        | Not supported; mise exits with an error | Not supported; mise exits with an error | Not enforced     |
| Environment variables (`--deny-env`, `--allow-env`)                               | Filtered by mise                        | Filtered by mise                        | Filtered by mise |

On Windows, mise filters the environment and warns that file, network and
process restrictions are not supported, then runs the command without them. In a container, the host kernel provides Landlock and seccomp; if
Landlock is unavailable, the command fails instead of running unrestricted. See
[platform details](#platform-details).

## Quick start

<a id="run-untrusted-script-with-no-filesystem-writes"></a>

```sh
# Block network access
mise exec --deny-net -- npm run build

# Run a script that must not write files outside /tmp
mise exec --deny-write -- bash script.sh

# Write only to ./dist (on Linux it must already exist)
mkdir -p dist
mise exec --allow-write=./dist -- npm run build

# Deny everything, then allow reading the project and writing ./dist
mise exec --deny-all --allow-read=. --allow-write=./dist -- node build.js

# Pass only MYAPP_* variables, plus the essential ones
mise exec --allow-env='MYAPP_*' -- node app.js
```

`--deny-all` still allows the [implicit access](#implicit-access) that programs
need to start, such as system libraries and `/tmp`. Add any caches your command
uses, such as `~/.npm`, to the allowed paths. On Linux, an allowed path must
exist before the command starts ([why](#linux)).

## Flags and task properties {#cli-flags}

Each flag works with `mise exec` and `mise run`, and has a task property with
the same meaning. Repeat an `--allow-*` flag to allow more paths or variables.

| Flag                   | Task property      | Effect                                                                                           |
| ---------------------- | ------------------ | ------------------------------------------------------------------------------------------------ |
| `--deny-all`           | `deny_all`         | Turn on the read, write, network and environment restrictions                                    |
| `--deny-read`          | `deny_read`        | Block reads, except the [implicit paths](#implicit-access)                                       |
| `--deny-write`         | `deny_write`       | Block writes, except to `/tmp` and `/dev`                                                        |
| `--deny-net`           | `deny_net`         | Block IPv4 and IPv6 sockets; Unix sockets still work                                             |
| `--deny-env`           | `deny_env`         | Pass only the [essential variables](#environment-variables)                                      |
| `--allow-read=<path>`  | `allow_read`       | Allow reads under a path, and block other reads                                                  |
| `--allow-write=<path>` | `allow_write`      | Allow reads and writes under a path, and block other writes                                      |
| `--allow-env=<var>`    | `allow_env`        | Pass a variable or a `*` pattern, and block other variables                                      |
| `--allow-net=<host>`   | `allow_net`        | Does not work on any platform; see [access to particular hosts](#access-to-particular-hosts)     |
|                        | `pass_through_env` | Experimental. Pass a variable without adding it to the task cache key; does not filter by itself |

## Sandbox every command {#default-restrictions}

The `sandbox.*` settings apply deny rules to every `mise exec` and `mise run`:

```toml
[settings]
sandbox.deny_net = true
```

The settings are [`sandbox.deny_all`](/configuration/settings.html#sandbox.deny_all),
`sandbox.deny_read`, `sandbox.deny_write`, `sandbox.deny_net` and
`sandbox.deny_env`, with environment variables such as `MISE_SANDBOX_DENY_NET`.
Put them in a project's `mise.toml` to apply them to everyone who runs its tasks,
or in your global config. Tasks and flags can still allow particular paths and
environment variables, but not particular hosts.

## Task sandboxing

Declare restrictions next to the command:

```mise-toml
[tasks.build]
run = "npm run build"
deny_net = true
allow_write = ["./dist"]

[tasks.lint]
run = "npm run lint"
deny_write = true

[tasks.test]
run = "npm test"
deny_net = true
allow_write = ["./coverage", "./node_modules/.cache"]
allow_env = ["NODE_*", "npm_*"]
```

On Linux, create `dist`, `coverage` and `node_modules/.cache` before the first
run. Allowing `NODE_*` and `npm_*` also passes any credentials or runtime options
with those names; list exact names for a narrower policy.

Settings, task properties and flags combine. Any of them can turn a restriction
on, and the task's allow lists and the flags' allow lists are merged, so a flag
adds an exception rather than replacing the task's. Flags on `mise run` apply to
every task that command runs, including dependencies.

Relative paths in a task resolve from the task's working directory, and relative
paths in flags from the directory where you run mise. Paths can start with `~`,
and task paths can use templates such as <code v-pre>{{config_root}}</code>. See
the [task configuration reference](/tasks/task-configuration.html) for each
property.

## Implicit access

When filesystem restrictions are on, these paths stay available so that programs
can start.

### Always readable

- Linux: `/usr`, `/lib`, `/lib64`, `/bin`, `/sbin`, `/etc`, `/dev`, `/proc`,
  `/sys`, `/tmp`, `/nix`, `/snap`, `/home/linuxbrew`, and the file
  `/etc/resolv.conf` points to
- macOS: `/System`, `/Library`, `/usr`, `/bin`, `/sbin`, `/dev`, `/etc`,
  `/var/run`, `/tmp`, `/private/tmp`, `/private/etc`, `/private/var/run`,
  `/opt/homebrew`, `/nix`
- mise's data directory (`MISE_DATA_DIR`), so installed tools can run. On Linux
  this also covers `MISE_INSTALLS_DIR` when it is outside the data directory.

`--allow-read` adds to these paths; it does not replace them. Paths you allow for
writing are also readable.

### Always writable

- `/tmp`, and `/private/tmp` on macOS
- `/dev`, for `/dev/null`, `/dev/tty` and similar files

### Environment variables

When environment filtering is on, the command gets only `PATH`, `HOME`, `USER`,
`SHELL`, `TERM`, `COLORTERM` and `LANG`, plus the variables you allow. Variables
set in `[env]` are dropped too unless you allow them. `PATH` still
includes the tools mise adds. A task also keeps the variables in its
`pass_through_env` and in its cache `env`. On Windows a few more variables
stay; see [Windows](#windows).

## When the sandbox blocks something

A blocked operation fails inside the command, and the command decides what to
print. It sees an ordinary error such as `Permission denied` or
`Operation not permitted`; a blocked DNS lookup often appears as
`Could not resolve host`. mise does not report these.

mise itself reports problems that affect the sandbox before the command runs: a
missing allowed path on Linux is a warning, while `--allow-net` and an
unavailable Landlock are errors.

## Access to particular hosts

`--allow-net` and the `allow_net` task property cannot limit network access to
particular hosts:

- On Linux and macOS, mise exits with an error before it runs the command:
  `per-host network filtering (--allow-net=<host>) is not supported on Linux`
  (or `on macOS`). Landlock and seccomp cannot name a host, and Seatbelt
  accepts only `*` or `localhost` as a network host.
- On Windows, mise warns and runs the command without network restrictions.

Use `--deny-net` to block internet sockets, and a firewall or proxy outside mise
when a command may reach only certain hosts.

## Platform details

### Linux

Filesystem rules use [Landlock](https://landlock.io/), available since Linux
5.13. Landlock gained features over several kernel releases, and mise uses what
the running kernel supports, so an older kernel enforces fewer rules. If Landlock
is unavailable or cannot apply the rules, the command fails.

Network rules use a
[seccomp-bpf](https://www.kernel.org/doc/html/latest/userspace-api/seccomp_filter.html)
filter that blocks IPv4 and IPv6 sockets and allows Unix sockets. It supports
x86_64 and arm64; on other architectures, `--deny-net` is an error.

Every allowed path must exist when the command starts. Landlock attaches a rule
to an open file, so mise drops the rule for a missing path and warns:
`sandbox: <path> does not exist, so its rule was dropped`. To let a task create
`node_modules`, allow its existing parent directory instead:

```mise-toml
[tasks.install]
run = "npm install"
allow_write = [".", "~/.npm"] # node_modules does not exist yet
```

Allowing a directory allows everything in it.

### macOS

mise writes the rules into a Seatbelt profile and runs the command through
`sandbox-exec`. Seatbelt rules are path patterns, so an allowed path does not
need to exist yet.

`$TMPDIR` usually points under `/var/folders`, which is not in the writable list.
If a program writes temporary files there, allow it with
`--allow-write="$TMPDIR"`.

When reads are restricted, sandboxed programs can still list the names directly
under `/` and look up the directories that lead to each allowed path, which many
runtimes need to find their own location. They cannot list those directories or
read anything else in them.

### Windows

Windows has no filesystem, network or process sandbox. When one of those is
requested, mise warns and runs the command without it, so a successful run with
those flags is not evidence that the policy held.

Environment filtering works as on the other platforms. Besides the usual
variables, `--deny-env` keeps the ones Windows programs need to start:
`SystemRoot`, `SystemDrive`, `windir`, `ComSpec`, `PATHEXT`, `TEMP`, `TMP`,
`USERPROFILE` and `USERNAME`. Names match without regard to case.
