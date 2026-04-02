# Sandboxing <Badge type="warning" text="experimental" />

Mise supports lightweight process sandboxing for `mise exec` and `mise run`, inspired by [zerobox](https://github.com/afshinm/zerobox). Sandboxing restricts filesystem, network, and environment variable access with granular controls. No Docker required, minimal overhead.

::: warning
Sandboxing is an experimental feature. Enable it with `mise settings experimental=true`.
:::

## Quick Start

Any `--deny-*` or `--allow-*` flag implicitly enables sandboxing:

```bash
# Full lockdown — no writes, no network, no env vars
mise x --deny-all -- node script.js

# Block network only
mise x --deny-net -- npm run build

# Block writes except to ./dist
mise x --allow-write=./dist -- npm run build

# Block everything, allow specific exceptions
mise x --deny-all --allow-read=. --allow-write=./dist --allow-net=registry.npmjs.org -- npm install
```

## CLI Flags

| Flag                   | Description                                                                                   |
| ---------------------- | --------------------------------------------------------------------------------------------- |
| `--deny-all`           | Block reads, writes, network, and env vars                                                    |
| `--deny-read`          | Block filesystem reads (system libs and tool dirs still accessible)                           |
| `--deny-write`         | Block all filesystem writes (except `/tmp`)                                                   |
| `--deny-net`           | Block all network access                                                                      |
| `--deny-env`           | Block env var inheritance (only `PATH`, `HOME`, `USER`, `SHELL`, `TERM`, `LANG` pass through) |
| `--allow-read=<path>`  | Allow reads from specific path (implies `--deny-read` for everything else)                    |
| `--allow-write=<path>` | Allow writes to specific path (implies `--deny-write` for everything else)                    |
| `--allow-net=<host>`   | Allow network to specific host (implies `--deny-net` for everything else)                     |
| `--allow-env=<var>`    | Allow specific env var through (implies `--deny-env` for everything else)                     |

These flags work with both `mise exec` (`mise x`) and `mise run`.

## Task Sandboxing

Tasks defined in `mise.toml` can declare sandbox permissions:

```toml
[tasks.build]
run = "npm run build"
deny_net = true
allow_write = ["./dist"]

[tasks.lint]
run = "eslint ."
deny_all = true
allow_read = ["."]

[tasks.install]
run = "npm install"
deny_all = true
allow_read = ["."]
allow_write = ["./node_modules"]
allow_net = ["registry.npmjs.org"]
```

CLI flags on `mise run` override task-level config:

```bash
# Run with task's declared sandbox
mise run build

# Override: also allow network to a specific host
mise run --allow-net=registry.npmjs.org build
```

## Implicit Access

When filesystem restrictions are active, certain paths remain accessible so tools can function:

### Always Readable

- **System paths** (Linux): `/usr`, `/lib`, `/lib64`, `/bin`, `/sbin`, `/etc`, `/dev`, `/proc`, `/sys`, `/tmp`, `/nix`, `/snap`, `/home/linuxbrew`
- **System paths** (macOS): `/System`, `/Library`, `/usr`, `/bin`, `/sbin`, `/dev`, `/etc`, `/var/run`, `/tmp`, `/private`, `/opt/homebrew`, `/nix`
- **Mise tool dirs**: `~/.local/share/mise/installs/...`

### Always Writable

- `/tmp` (and `/private/tmp` on macOS)
- `/dev` (for `/dev/null`, `/dev/tty`, etc.)

### Implicit Rules

- `--allow-write` paths are implicitly readable
- `--allow-read` paths include system essentials above

## Platform Support

| Feature                                 | Linux              | macOS    |
| --------------------------------------- | ------------------ | -------- |
| Deny/allow reads                        | Landlock           | Seatbelt |
| Deny/allow writes                       | Landlock           | Seatbelt |
| Deny all network                        | seccomp            | Seatbelt |
| Per-host network (`--allow-net=<host>`) | Not supported (v1) | Seatbelt |
| Env filtering                           | Built-in           | Built-in |
| Docker support                          | Yes                | N/A      |

### Linux

Filesystem sandboxing uses [Landlock](https://landlock.io/) (available since Linux 5.13). Network sandboxing uses [seccomp-bpf](https://www.kernel.org/doc/html/latest/userspace-api/seccomp_filter.html) to block inet socket creation while allowing Unix sockets.

If Landlock is unavailable (older kernels), a warning is printed and the command runs unsandboxed.

**Limitation**: Per-host network filtering (`--allow-net=<host>`) is not supported on Linux in v1. On Linux, `--allow-net` falls back to allowing all network access. This works on macOS via Seatbelt.

### macOS

Uses Apple's `sandbox-exec` (Seatbelt) with a generated profile. Supports all features including per-host network filtering.

### Windows

Sandboxing is not currently supported on Windows. A warning is printed and the command runs unsandboxed.

## Examples

### Run untrusted script with no filesystem writes

```bash
mise x --deny-write -- bash untrusted-script.sh
```

### Build with network isolation

```bash
mise x --deny-net -- make build
```

### Run tool with minimal permissions

```bash
mise x --deny-all --allow-read=./src --allow-write=./dist node@20 -- node build.js
```

### Sandboxed task definition

```toml
[tasks.test]
run = "npm test"
deny_net = true
deny_write = true
allow_write = ["./coverage", "./node_modules/.cache"]
```
