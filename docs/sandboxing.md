# Sandboxing

mise can restrict filesystem, network, and environment access for commands launched by
`mise exec` and `mise run`. Restrictions use the host operating system, with different support
on Linux and macOS. Read [platform support](#platform-support) before relying on a policy;
Windows does not enforce filesystem or network restrictions.

The sandbox applies to the child command. Configuration evaluation, tool installation, and
other preparation by mise happen outside that command's sandbox. For untrusted configuration,
see [safe mode](/security.html#safe-mode).

## Quick Start

Any `--deny-*` or `--allow-*` flag enables the corresponding restriction. In a project that
has Node installed:

```sh
# Block network access for a local build
mise exec --deny-net -- npm run build

# Restrict writes to an existing output directory, plus implicit system exceptions
mkdir -p dist
mise exec --allow-write=./dist -- npm run build

# Deny reads, writes, network, and nonessential environment variables,
# then allow reading this project and writing its output
mise exec --deny-all --allow-read=. --allow-write=./dist -- node build.js
```

The npm commands require a `build` script; the final command requires `build.js`. Adjust
allowed paths for the files and caches your build actually uses. `--deny-all` retains the
[implicit access](#implicit-access) described below; it is not a container with an empty filesystem.

## CLI Flags

| Flag                   | Description                                                                                                            |
| ---------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `--deny-all`           | Block reads, writes, network, and env vars                                                                             |
| `--deny-read`          | Block filesystem reads (system libs and tool dirs still accessible)                                                    |
| `--deny-write`         | Block writes except implicit temporary/device paths                                                                    |
| `--deny-net`           | Block all network access                                                                                               |
| `--deny-env`           | Block env var inheritance (essential variables and explicit exceptions still pass through)                             |
| `--allow-read=<path>`  | Allow reads from specific path (implies `--deny-read` for everything else)                                             |
| `--allow-write=<path>` | Allow writes to specific path (implies `--deny-write` for everything else)                                             |
| `--allow-net=<host>`   | Request host exceptions on macOS (see platform limitations); rejected on Linux                                         |
| `--allow-env=<var>`    | Allow specific env var through (implies `--deny-env` for everything else). Supports wildcards: `--allow-env='MYAPP_*'` |

These flags work with both `mise exec` (`mise x`) and `mise run`.

## Default Restrictions

Sandbox deny rules can be enabled for every `mise exec` and `mise run` invocation with settings:

```toml
[settings.sandbox]
deny_all = true
```

The available settings mirror the deny flags: `deny_all`, `deny_read`, `deny_write`, `deny_net`,
and `deny_env`. Tasks and CLI flags can still add `allow_read`, `allow_write`, `allow_net`, or
`allow_env` exceptions as needed.

## Task Sandboxing

Tasks can declare restrictions next to the command. This example assumes Node is configured,
`npm run build` exists, and the output directory has been created:

```toml
[tasks.build]
run = "npm run build"
deny_net = true
allow_write = ["./dist"]

[tasks.lint]
run = "npm run lint"
deny_write = true
```

```sh
mkdir -p dist
mise run build
```

Global settings, task deny rules, and CLI deny flags are combined. Task and CLI allow lists
are combined too; CLI flags add exceptions rather than replacing the task policy. Task paths
are relative to the task's working directory, while CLI paths are relative to the directory
where you invoke mise.

The host-exception flag is intended for macOS tasks that need network access:

```sh
mise run --allow-net=registry.npmjs.org build
```

A package manager may contact additional hosts and write a cache or lockfile outside the
output directory. Allow only the resources required by the actual command. Linux rejects
`--allow-net`; macOS can also reject the generated profile as described below. Use
`--deny-net` for a command that needs no internet sockets.

## Implicit Access

When filesystem restrictions are active, certain paths remain accessible so tools can function:

### Always Readable

- **System paths** (Linux): `/usr`, `/lib`, `/lib64`, `/bin`, `/sbin`, `/etc`, `/dev`, `/proc`, `/sys`, `/tmp`, `/nix`, `/snap`, `/home/linuxbrew`
- **System paths** (macOS): `/System`, `/Library`, `/usr`, `/bin`, `/sbin`, `/dev`, `/etc`, `/var/run`, `/tmp`, `/private/tmp`, `/private/etc`, `/private/var/run`, `/opt/homebrew`, `/nix`
- **Mise data directory**: the configured `MISE_DATA_DIR`, not just individual tool binaries

### Always Writable

- `/tmp` (and `/private/tmp` on macOS)
- `/dev` (for `/dev/null`, `/dev/tty`, etc.)

### Implicit Rules

- `--allow-write` paths are implicitly readable
- `--allow-read` paths include system essentials above

When environment filtering is active, `PATH`, `HOME`, `USER`, `SHELL`, `TERM`, `COLORTERM`,
and `LANG` remain available, along with explicitly allowed variables and task-specific
pass-through/cache environment inputs. Unix sockets remain available even with `--deny-net`.

## Platform Support

| Feature                                 | Linux    | macOS    |
| --------------------------------------- | -------- | -------- |
| Deny/allow reads                        | Landlock | Seatbelt |
| Deny/allow writes                       | Landlock | Seatbelt |
| Deny all network                        | seccomp  | Seatbelt |
| Per-host network (`--allow-net=<host>`) | Rejected | Seatbelt |
| Env filtering                           | Built-in | Built-in |
| Docker support                          | Yes      | N/A      |

### Linux

Filesystem sandboxing uses [Landlock](https://landlock.io/) (available since Linux 5.13). Network sandboxing uses [seccomp-bpf](https://www.kernel.org/doc/html/latest/userspace-api/seccomp_filter.html) to block inet socket creation while allowing Unix sockets.

If Landlock is unavailable or cannot apply filesystem restrictions, the command fails.

**Limitation**: Per-host network filtering (`--allow-net=<host>`) is not supported on Linux.
mise returns an error before executing the command; it does not silently allow all network
access. Use `--deny-net` to block internet sockets, or omit network restrictions when needed.

**Limitation**: An allow-list entry has to exist when the sandbox is built. Landlock binds each rule to an open descriptor, so a path that has not been created yet cannot be named by one, and mise warns that the rule was dropped. The task can still reach that path if another rule covers it — an allowed ancestor directory, for instance — but nothing else grants access on the dropped rule's behalf. To let a task create something, allow a directory that already exists and contains it.

```toml
[tasks.install]
run = "npm install"
allow_read = ["package.json", "~/.npm"]
# not ["node_modules"] — that does not exist until the task creates it
allow_write = [".", "~/.npm"]
```

Landlock cannot restrict creation to a single name, so allowing the containing directory necessarily grants write access to everything else in it. This applies to Linux only; on macOS, Seatbelt rules are path patterns and do not need the path to exist.

### macOS

Sandboxing uses Apple's `sandbox-exec` (Seatbelt) with a generated profile. Network host
exceptions resolve hostnames to IP addresses when the profile is built. The intended policy allows those IPs,
not a particular HTTP hostname or URL path; services sharing an IP may also be reachable.

**Limitation**: `sandbox-exec` can reject the generated host-exception profile with
`host must be * or localhost in network address`. This prevents the child command from
starting; it does not fall back to unrestricted network access. If you need network access
to selected hosts, verify the policy on your macOS version and use an external network
control when `--allow-net` cannot express it.

When reads are restricted, Seatbelt requires data access to the root directory for process startup.
Sandboxed processes can enumerate names directly under `/`, but cannot read unallowed entries or
their descendants.

### Windows

Filesystem and network sandboxing is not supported on Windows. mise warns and runs the
command without those OS restrictions. Do not treat a successful Windows invocation with
sandbox flags as evidence that the filesystem or network policy was enforced.

## Examples

### Restrict script writes {#run-untrusted-script-with-no-filesystem-writes}

```bash
mise x --deny-write -- bash script.sh
```

### Build with network isolation

```bash
mise x --deny-net -- make build
```

### Run tool with minimal permissions

```bash
mkdir -p dist
mise exec --deny-all --allow-read=. --allow-write=./dist -- node build.js
```

### Restrict env vars to a namespace

```bash
# Pass MYAPP_* in addition to the essential variables
mise x --allow-env='MYAPP_*' -- node app.js

# Allow multiple patterns
mise x --allow-env='MYAPP_*' --allow-env='NODE_*' -- node app.js
```

### Sandboxed task definition

Create `coverage/` and `node_modules/.cache/` before running this task on Linux, and adjust
the paths for your test runner. Allowing `NODE_*` and `npm_*` also exposes any matching
credentials or runtime options; list exact variable names if a narrower policy is needed.

```toml
[tasks.test]
run = "npm test"
deny_net = true
deny_write = true
allow_write = ["./coverage", "./node_modules/.cache"]
allow_env = ["NODE_*", "npm_*"]
```
