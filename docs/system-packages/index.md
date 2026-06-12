# System Packages <Badge type="warning" text="experimental" />

mise can ensure machine-global system packages are installed via the
`[system.packages]` section of `mise.toml`:

```toml
[system.packages]
"apt:libssl-dev" = "latest"
"apt:build-essential" = "latest"
"brew:postgresql@17" = "latest"
"brew:ffmpeg" = "latest"
```

Each entry is keyed `"manager:package"` — the manager prefix is required —
and the value is a version: `"latest"` for whatever the manager installs, or
a pin in the manager's native format where supported (see the per-manager
pages).

System packages are intentionally separate from [`[tools]`](/configuration.html):
they are not version-pinned per-project, do not get shims, and are installed
machine-globally by the platform's package manager — or, for `brew`, by
mise's built-in Homebrew bottle installer, which doesn't require Homebrew
itself. Use them for shared libraries and build dependencies that dev tools
need (`libssl-dev`, `postgresql`, `ffmpeg`), not for the dev tools
themselves — those belong in `[tools]`.

## Supported package managers

| Manager  | Platform                                                       | Page                                   |
| -------- | -------------------------------------------------------------- | -------------------------------------- |
| `apt`    | Debian, Ubuntu                                                 | [apt](/system-packages/apt.html)       |
| `dnf`    | Fedora, RHEL, CentOS, Rocky, Alma                              | [dnf](/system-packages/dnf.html)       |
| `pacman` | Arch, Manjaro                                                  | [pacman](/system-packages/pacman.html) |
| `brew`   | macOS (arm64), Linux (x86_64/arm64) — **no Homebrew required** | [brew](/system-packages/brew.html)     |

## Semantics

- **Declarative and additive** — entries merge across the
  [config hierarchy](/configuration.html) (global → project) as a union of
  keys. A project can add packages on top of the global list (and override a
  global entry's version pin) but not remove them.
- **OS-filtered** — entries for a manager that isn't available on the current
  machine are not acted on, so the same config works across platforms: `apt`
  entries are ignored on macOS, `dnf` entries on Ubuntu, and so on (`brew`
  works on both macOS and Linux). `mise system status` and `mise doctor`
  still list unavailable managers so nothing is silently invisible.
- **Manual installation only** — mise never installs system packages
  implicitly. `mise install` will print a one-time hint when packages are
  missing, but only `mise system install` ever installs anything.
- **Unknown managers are ignored with a warning** so configs using managers
  from newer mise versions still parse.

## Commands

```sh
mise system status            # table of requested vs installed packages
mise system status --json     # machine-readable
mise system status --missing  # exit 1 if anything is missing (CI check)

mise system install           # install whatever is missing (prompts first)
mise system install apt:curl  # install specific packages (configured or not)
mise system install --dry-run # print the commands without running them
mise system install --yes     # skip the confirmation prompt
mise system install --manager apt
mise system install --update  # refresh package manager metadata first
```

`mise doctor` also reports configured system packages and warns when any are
missing.

## Choosing which managers run

By default mise acts on every configured manager that is available on the
current machine. Since availability implies the OS (`apt` only exists on
Debian-family systems, `brew` wherever a bottle exists), this usually does the right
thing without configuration.

If more than one manager could apply — several package managers installed on
one machine, or a shared config listing managers you don't want here — pick a
subset with the [`system_packages.managers`](/configuration/settings.html)
setting:

```toml
[settings]
system_packages.managers = ["apt"]
```

This composes with [platform-specific config files](/configuration.html)
(`mise.macos.toml`, `mise.linux.toml`) when you want different selections per
OS.

## sudo

The Linux package managers require root. When not running as root, mise
elevates with `sudo`, which prompts for your password as usual. This is the
only place mise ever elevates privileges, and it only happens during an
explicit `mise system install`:

- already root (containers, CI): no sudo, commands run directly
- interactive terminal: e.g. `sudo apt-get install ...` with a normal sudo
  prompt
- non-interactive without passwordless sudo: mise errors and prints the exact
  command to run manually — it never hangs waiting for a password
- the full command line is logged before it runs

Set [`system_packages.sudo = false`](/configuration/settings.html) to forbid
elevation entirely; mise will print the command for you to run yourself
instead. The `brew` manager never needs sudo except once to create
`/opt/homebrew` (see [brew](/system-packages/brew.html)).

## CI usage

In containers you're typically already root, so no prompts occur:

```sh
mise system install --yes
mise install
```

`mise system status --missing` exits 1 when packages are missing, which makes
a convenient CI check without installing anything.
