# Bootstrap Packages

Declare shared host packages in `[bootstrap.packages]`, then apply them with
`mise bootstrap packages apply` or the full [bootstrap](/bootstrap.html).
Use this for native libraries, build dependencies, and host applications.

Start with the manager used by your machine. For a Debian or Ubuntu host:

```toml
[bootstrap.packages]
"apt:libssl-dev" = "latest"
"apt:build-essential" = "latest"
```

Preview and apply the configured packages:

```sh
mise bootstrap packages status
mise bootstrap packages apply --dry-run
mise bootstrap packages apply
```

Each entry is keyed `"manager:package"` — the manager prefix is required —
and the value is a version: `"latest"` for whatever the manager installs, or
a pin in the manager's native format where supported (see the per-manager
pages). **`"latest"` accepts an already-installed version.** It does not trigger
an upgrade on every apply; use `mise bootstrap packages upgrade` for that.

Use the table form to restrict an individual package by operating system or
OS/architecture. `os` accepts one value or a list and uses the same names and
aliases as `[tools]` (`linux`, `macos`, `windows`, `linux/x64`,
`macos/arm64`, and so on). `version` defaults to `"latest"` when omitted:

```toml
[bootstrap.packages]
"brew:coreutils" = "latest"
"brew-cask:1password" = { os = "macos" }
"brew-cask:font-jetbrains-mono" = { os = ["linux", "macos"] }
"pacman:libreoffice-fresh" = { state = "absent" }
```

`pacman` entries may set `state = "absent"` to declaratively remove a package.
`mise bootstrap packages status --missing` treats an installed package with
that declaration as drift, and `mise bootstrap packages apply` removes it.
Other built-in managers currently support only the default `state = "present"`.

`brew-cask` entries additionally accept `adopt = true` to adopt an identical
app already installed at the cask destination. Set `bootstrap.brew.adopt = true`
to make adoption the default for all casks, with per-cask `adopt = false`
overrides. See the
[brew cask documentation](/bootstrap/packages/brew.html#casks).

## Host packages or mise tools

Host package declarations can include version constraints where the manager
supports them, but installations are shared outside the project. Changing
directories does not switch them, and mise does not create shims for them.
Use [`[tools]`](/dev-tools/) when you need isolated versions selected by each
project. Use `[bootstrap.packages]` when the software belongs in the host's
package database or shared prefix.

The manager list is extensible through [package manager plugins](./plugins.md)
for host-owned state such as editor extensions and other applications' plugins.

## Supported package managers

| Manager        | Platform                                                       | Page                                                |
| -------------- | -------------------------------------------------------------- | --------------------------------------------------- |
| `apk`          | Alpine Linux                                                   | [apk](/bootstrap/packages/apk.html)                 |
| `apt`          | Debian, Ubuntu                                                 | [apt](/bootstrap/packages/apt.html)                 |
| `aur`          | Arch, Manjaro with yay or paru                                 | [AUR](/bootstrap/packages/aur.html)                 |
| `dnf`          | Fedora, RHEL, CentOS, Rocky, Alma                              | [dnf](/bootstrap/packages/dnf.html)                 |
| `pacman`       | Arch, Manjaro                                                  | [pacman](/bootstrap/packages/pacman.html)           |
| `brew`         | macOS (arm64), Linux (x86_64/arm64) — **no Homebrew required** | [brew](/bootstrap/packages/brew.html)               |
| `brew-cask`    | macOS; Linux (font casks) — **no Homebrew required**           | [brew](/bootstrap/packages/brew.html)               |
| `flatpak`      | Linux with the `flatpak` CLI on `PATH` (system scope)          | [Flatpak](/bootstrap/packages/flatpak.html)         |
| `flatpak-user` | Linux with the `flatpak` CLI on `PATH` (user scope)            | [Flatpak](/bootstrap/packages/flatpak.html)         |
| `mas`          | macOS with the `mas` CLI on `PATH`                             | [mas](/bootstrap/packages/mas.html)                 |
| plugin         | Declared by the plugin                                         | [Package plugins](/bootstrap/packages/plugins.html) |

## Semantics

- **Declarative and additive by default** — entries merge across the
  [config hierarchy](/configuration.html) (global → project) as a union of
  keys. A project can add packages on top of the global list (and override a
  global entry's version pin). A more local config can override a pacman entry
  with `state = "absent"`. Pruning is an explicit,
  manager-scoped destructive operation: `mise bootstrap packages prune`
  defaults to Homebrew, while plugin-owned packages require
  `mise bootstrap packages prune --manager <plugin>`. It removes only packages
  no longer needed by the current config or by trusted, loadable tracked
  configs.
- **OS-filtered** — entries whose `os` selector does not match and entries for
  a manager that isn't available on the current machine are not acted on, so
  the same config works across platforms: `apt` entries are ignored on macOS,
  `dnf` entries on Ubuntu, and so on. `brew` works on both macOS and Linux;
  `brew-cask` works on macOS and supports font-only casks without lifecycle
  hooks or structured flight steps on Linux;
  `flatpak` and `flatpak-user` work on Linux when the `flatpak` CLI is on
  `PATH`; `mas` works on macOS when the `mas` CLI is on `PATH`. Status commands
  still list unavailable managers so nothing is silently hidden.
- **Manual installation only** — mise never installs system packages
  implicitly. `mise install` prints a one-time hint when packages are
  missing. Explicit `packages apply`, `packages use`, and the full
  `mise bootstrap` perform installation; `packages upgrade` updates installed
  packages.
- **Unknown managers are ignored with a warning** and a package-plugin install
  hint, so configs using managers from newer mise versions still parse.

## Commands

### Apply or record packages

```sh
mise bootstrap packages status --json
mise bootstrap packages status --missing
mise bootstrap packages apply --manager apt --dry-run
mise bootstrap packages apply --manager apt
mise bootstrap packages apply --update

mise bootstrap packages use apt:curl
mise bootstrap packages use -g brew:ffmpeg
```

`apply` without package arguments reads the active configuration. An explicit
request such as `mise bootstrap packages apply apt:curl` can install a package
without recording it. Use `use` when the package should remain declared.
`--update` refreshes metadata according to the manager; `--yes` skips mise's
confirmation prompt but does not provide sudo credentials.

`mise bootstrap packages use` is `mise use` for system packages: it writes
`"manager:package" = "version"` entries to `mise.toml` (the local file by
default, the global one with `-g`) and installs whatever is missing. Entries
for managers that aren't available on the current machine are written without
installing — that's how a shared config picks up `apt:` lines authored on a
Mac.

### Import and prune

```sh
mise bootstrap packages import --manager brew --dry-run
mise bootstrap packages import --manager brew
mise bootstrap packages prune --manager brew --dry-run
```

Inspect the prune plan before running without `--dry-run`. Formula pruning can
include software installed by Homebrew itself, not just by mise.

`mise bootstrap packages import --manager brew` is the inverse for Homebrew
formulae: it reads the active Homebrew `opt` links and writes requested
formulae to `[bootstrap.packages]` as `"brew:<formula>" = "latest"`. By
default it imports only formulae whose keg receipt says they were installed
on request; pass `--all` to include dependency formulae too. Future prune runs
keep imported formulae because they are now declared in config.

`mise bootstrap packages prune --manager brew` removes linked brew formulae
that are no longer needed by the current config or by trusted, loadable tracked
configs. This includes formulae installed by a real Homebrew. It is mise's
declarative cleanup command, similar in spirit to
[Homebrew Bundle cleanup](https://docs.brew.sh/Manpage), not the old upstream
`brew prune` command, which Homebrew removed.

`mise bootstrap packages prune --manager brew-cask` removes only mise-owned
direct artifacts backed by a current install-time receipt and unchanged content
fingerprints. It skips older receipts, Homebrew-owned casks, pkg and command
wrapper artifacts, casks with lifecycle actions, changed or shared targets,
and incomplete transactions. Skips include a reason, and `zap` metadata is
never applied.

For a package-plugin manager, prune considers only packages that mise observed
transitioning from missing to installed during `PackageInstall`. Existing or
manually installed packages are never adopted. The plugin must implement
`PackageUninstall`; dry runs print the approved removal batch without invoking
the hook, and mise verifies removals with `PackageInstalled` before updating its
ownership state.

### Upgrade installed packages

```sh
mise bootstrap packages upgrade --manager apt --dry-run
mise bootstrap packages upgrade --manager apt
```

`mise bootstrap packages upgrade` refreshes package manager metadata and upgrades the
configured packages that are already installed to the newest available
version — apk, apt, and dnf also honor a version pinned in config
([AUR](/bootstrap/packages/aur.html), [pacman](/bootstrap/packages/pacman.html),
brew, brew-cask, flatpak, flatpak-user, and mas can't install pins, so
pinned entries are skipped with a warning). Packages that aren't installed
yet are skipped — that's `mise bootstrap packages apply`'s job. For brew,
this pours the formula's current bottle and replaces the old keg; for
brew-cask, this installs the current cask artifact; for flatpak and flatpak-user,
this updates the configured applications and runtimes in their respective
scopes; for mas, this runs `mas upgrade`.

`mise doctor` also reports configured system packages and warns when any are
missing.

## Choosing which managers run

By default, mise acts on every configured manager that is available on the
current machine. Availability checks the supported platform and required
commands; it is not a choice of one preferred manager. For example, a Linux host
can use both apt and mise's built-in Homebrew manager if both have declarations.

If more than one manager could apply — several package managers installed on
one machine, or a shared config listing managers you don't want here — pick a
subset with the [`system_packages.managers`](/configuration/settings.html#system_packages.managers)
setting:

```toml
[settings]
system_packages.managers = ["apt"]
```

You can also use the per-package `os` selector shown above. To put selections
in `mise.macos.toml` or `mise.linux.toml`, activate that configuration environment
with `-E`/`MISE_ENV` or enable
[`auto_env`](/configuration/environments.html#platform-environments); the filename
alone does not currently activate it.

## sudo

The apk, apt, dnf, and pacman managers need root for package changes. mise
uses sudo when necessary. AUR helpers build as the current user and handle their
own package-install elevation; Flatpak user installations do not need root.
The same mise sudo path is used when login-shell setup must edit `/etc/shells`:

- already root (containers, CI): no sudo, commands run directly
- interactive terminal: e.g. `sudo apt-get install ...` with a normal sudo
  prompt
- non-interactive without passwordless sudo: mise errors and prints the exact
  command to run manually — it never hangs waiting for a password
- in every case, the full command line is logged before it runs

Set [`system_packages.sudo = false`](/configuration/settings.html#system_packages.sudo) to forbid
elevation entirely; mise prints the command for you to run yourself
instead. Homebrew formula installation may need elevation to create its
canonical prefix; cask installers can also need elevation for their artifacts
(see [brew](/bootstrap/packages/brew.html)).
Package plugins never use mise's sudo path and must never elevate themselves.

## CI usage

In containers you're typically already root, so no prompts occur:

```sh
mise bootstrap packages apply --yes
mise install
```

[`mise bootstrap --yes`](/bootstrap.html) combines both (and runs a task
named `bootstrap` afterwards, if one is defined) — one command to set up a
fresh machine or container.

`mise bootstrap packages status --missing` exits 1 when packages are missing, which makes
for a convenient CI check without installing anything. Inspect JSON status as
well when a required manager may be unavailable: skipped declarations are not
proof that their packages are installed.
