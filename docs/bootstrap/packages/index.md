# Bootstrap Packages <Badge type="warning" text="experimental" />

mise can ensure machine-global system packages are installed via the
`[bootstrap.packages]` section of `mise.toml`:

```toml
[bootstrap.packages]
"apk:build-base" = "latest"
"apt:libssl-dev" = "latest"
"apt:build-essential" = "latest"
"brew:postgresql@17" = "latest"
"brew:ffmpeg" = "latest"
"brew-cask:firefox" = "latest"
"mas:497799835" = "latest"
```

Each entry is keyed `"manager:package"` — the manager prefix is required —
and the value is a version: `"latest"` for whatever the manager installs, or
a pin in the manager's native format where supported (see the per-manager
pages).

mise can also place config files (dotfiles) — see
[Dotfiles](/dotfiles.html), which uses `mise dotfiles` commands.

System packages are intentionally separate from [`[tools]`](/configuration.html):
they are not version-pinned per-project, do not get shims, and are installed
machine-globally by the platform's package manager — or, for `brew` and
`brew-cask`, by mise's built-in Homebrew installers, which don't require
Homebrew itself. Use them for shared libraries, build dependencies, and
machine-global GUI apps (`libssl-dev`, `postgresql`, `ffmpeg`, `firefox`),
not for project dev tools — those belong in `[tools]`.

The `[bootstrap]` section can also declare
[repos](/bootstrap/repos.html) (`[bootstrap.repos]`), applied by
`mise bootstrap repos apply`, [macOS defaults](/bootstrap/macos-defaults.html)
(`[bootstrap.macos.defaults]`), applied by
`mise bootstrap macos defaults apply`, and macOS
[LaunchAgents](/bootstrap/launchd.html) (`[bootstrap.macos.launchd.agents]`),
applied by `mise bootstrap macos launchd-agents apply`. Linux user services live in
[systemd](/bootstrap/systemd.html) (`[bootstrap.linux.systemd.units]`),
applied by `mise bootstrap linux systemd-units apply`. Shell activation snippets live in
[Shell Activation](/bootstrap/shell.html) (`[bootstrap.mise_shell_activate]`).
Current-user
[login shells](/bootstrap/user.html) (`[bootstrap.user].login_shell`) are
applied by `mise bootstrap user apply` or [`mise bootstrap`](/cli/bootstrap.html).

## Supported package managers

| Manager     | Platform                                                       | Page                                      |
| ----------- | -------------------------------------------------------------- | ----------------------------------------- |
| `apk`       | Alpine Linux                                                   | [apk](/bootstrap/packages/apk.html)       |
| `apt`       | Debian, Ubuntu                                                 | [apt](/bootstrap/packages/apt.html)       |
| `dnf`       | Fedora, RHEL, CentOS, Rocky, Alma                              | [dnf](/bootstrap/packages/dnf.html)       |
| `pacman`    | Arch, Manjaro                                                  | [pacman](/bootstrap/packages/pacman.html) |
| `brew`      | macOS (arm64), Linux (x86_64/arm64) — **no Homebrew required** | [brew](/bootstrap/packages/brew.html)     |
| `brew-cask` | macOS — **no Homebrew required**                               | [brew](/bootstrap/packages/brew.html)     |
| `mas`       | macOS with the `mas` CLI on `PATH`                             | [mas](/bootstrap/packages/mas.html)       |

## Semantics

- **Declarative and additive by default** — entries merge across the
  [config hierarchy](/configuration.html) (global → project) as a union of
  keys. A project can add packages on top of the global list (and override a
  global entry's version pin) but not remove them. For Homebrew formulae,
  `mise bootstrap packages prune --manager brew` is an explicit destructive
  command that removes mise-managed formulae no longer declared by the merged
  config.
- **OS-filtered** — entries for a manager that isn't available on the current
  machine are not acted on, so the same config works across platforms: `apt`
  entries are ignored on macOS, `dnf` entries on Ubuntu, and so on. `brew`
  works on both macOS and Linux; `brew-cask` works on macOS; `mas` works on
  macOS when the `mas` CLI is on `PATH`. Status commands still list
  unavailable managers so nothing is silently invisible.
- **Manual installation only** — mise never installs system packages
  implicitly. `mise install` will print a one-time hint when packages are
  missing, but only `mise bootstrap packages apply` ever installs anything.
- **Unknown managers are ignored with a warning** so configs using managers
  from newer mise versions still parse.

For current-user login shell setup, use `[bootstrap.user].login_shell`:

```toml
[bootstrap.user]
login_shell = "/bin/zsh"
```

See [User Login Shell](/bootstrap/user.html) for details.

## Commands

```sh
mise bootstrap packages status            # table of requested vs installed packages
mise bootstrap packages status --json     # machine-readable
mise bootstrap packages status --missing  # exit 1 if anything is out of sync (CI check)

mise bootstrap packages apply           # install whatever is missing (prompts first)
mise bootstrap packages apply apt:curl  # install specific packages (configured or not)
mise bootstrap packages apply --dry-run # print the commands without running them
mise bootstrap packages apply --yes     # skip the confirmation prompt
mise bootstrap packages apply --manager apt
mise bootstrap packages apply --update  # refresh package manager metadata first

mise bootstrap packages use apk:zlib-dev apt:curl brew:jq brew-cask:firefox mas:497799835
mise bootstrap packages use -g brew:ffmpeg                      # write globally
mise bootstrap packages use apt:curl@8.5.0-2   # pin a version (brew pins via the
                                   # formula name: brew:postgresql@17)

mise bootstrap packages import --manager brew   # add installed requested brew formulae
mise bootstrap packages import --manager brew --all
mise bootstrap packages import --manager brew --dry-run

mise bootstrap packages prune --manager brew    # remove adopted unconfigured brew formulae
mise bootstrap packages prune --manager brew --dry-run
mise bootstrap packages prune --manager brew --yes

mise bootstrap packages upgrade           # upgrade installed packages to current versions
mise bootstrap packages upgrade --manager brew
mise bootstrap packages upgrade --manager brew-cask
mise bootstrap packages upgrade --manager mas
```

`mise bootstrap packages use` is `mise use` for system packages: it writes
`"manager:package" = "version"` entries to mise.toml (the local file by
default, the global one with `-g`) and installs whatever is missing. Entries
for managers that aren't available on the current machine are written without
installing — that's how a shared config picks up `apt:` lines authored on a
Mac.

`mise bootstrap packages import --manager brew` is the inverse for Homebrew
formulae: it reads the active Homebrew `opt` links and writes requested
formulae to `[bootstrap.packages]` as `"brew:<formula>" = "latest"`. By
default it imports only formulae whose keg receipt says they were installed
on request; pass `--all` to include dependency formulae too. Imported formulae
and their resolved dependency closure are adopted into mise's brew ledger, so
they can be pruned later.

`mise bootstrap packages prune --manager brew` removes brew formulae that
mise installed or adopted but that are no longer needed by the merged
`[bootstrap.packages]` config. It does not touch unmanaged Homebrew formulae.
This is mise's declarative cleanup command, similar in spirit to
[Homebrew Bundle cleanup](https://docs.brew.sh/Manpage), not the old upstream
`brew prune` command, which Homebrew removed.

`mise bootstrap packages upgrade` refreshes package manager metadata and upgrades the
configured packages that are already installed to the newest available
version — apk, apt, and dnf also honor a version pinned in config (pacman, brew,
brew-cask, and mas [can't install pins](/bootstrap/packages/pacman.html), so
pinned entries are skipped with a warning). Packages that aren't installed
yet are skipped — that's `mise bootstrap packages apply`'s job. For brew
this pours the formula's current bottle and replaces the old keg; for
brew-cask this installs the current cask artifact; for mas this runs
`mas upgrade`.

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
elevates with `sudo`, which prompts for your password as usual. The same
sudo path is used when `[bootstrap.user].login_shell` needs to add a shell to
`/etc/shells`, and it only happens during an explicit `mise bootstrap`:

- already root (containers, CI): no sudo, commands run directly
- interactive terminal: e.g. `sudo apt-get install ...` with a normal sudo
  prompt
- non-interactive without passwordless sudo: mise errors and prints the exact
  command to run manually — it never hangs waiting for a password
- the full command line is logged before it runs

Set [`system_packages.sudo = false`](/configuration/settings.html) to forbid
elevation entirely; mise will print the command for you to run yourself
instead. The `brew` manager never needs sudo except once to create
`/opt/homebrew` (see [brew](/bootstrap/packages/brew.html)).

## CI usage

In containers you're typically already root, so no prompts occur:

```sh
mise bootstrap packages apply --yes
mise install
```

[`mise bootstrap --yes`](/cli/bootstrap.html) combines both (and runs a task
named `bootstrap` afterwards, if one is defined) — one command to set up a
fresh machine or container.

`mise bootstrap packages status --missing` exits 1 when packages are missing, which makes
a convenient CI check without installing anything.
