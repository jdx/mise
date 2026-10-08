---
description: "Declare machine-wide packages in mise.toml and install them with apt, Homebrew, WinGet, and other package managers."
socialDescription: "Declare machine-wide packages in mise.toml and install them with your package managers."
---

# Bootstrap packages

`[bootstrap.packages]` declares machine-wide software, such as native
libraries, build dependencies, and desktop apps, and installs it with the
host's package managers.

Every project on the machine shares these installations: mise does not switch
their versions per directory or create shims for them. Use
[`[tools]`](/dev-tools/) for tools whose version each project chooses. A project
can use both, for example a compiler from `[tools]` and the OpenSSL headers it
links against from `[bootstrap.packages]`.

mise installs these packages only when you ask, with
`mise bootstrap packages apply` or the full [`mise bootstrap`](/bootstrap.html).
`mise install` only prints a one-time hint about missing ones.

## Get started

On Debian or Ubuntu, add this to `mise.toml`:

```toml
[bootstrap.packages]
"apt:libssl-dev" = "latest"
"apt:build-essential" = "latest"
```

Check what is already installed:

```sh
mise bootstrap packages status
```

```text
Manager  Package          Installed           State
apt      libssl-dev       3.0.13-0ubuntu3.16  installed
apt      build-essential                      missing
```

Preview the commands mise would run, then run them:

```sh
mise bootstrap packages apply --dry-run
# sudo env DEBIAN_FRONTEND=noninteractive apt-get install -y -- build-essential
mise bootstrap packages apply
```

On other systems, change the prefix, for example `dnf:openssl-devel` on Fedora
or `brew:openssl@3` on macOS. The [table below](#supported-package-managers)
lists every manager.

A `"latest"` entry is satisfied by whatever version is installed: `apply`
installs missing packages but does not upgrade installed ones. Run
[`mise bootstrap packages upgrade`](#upgrade-installed-packages) to update them.

## Supported package managers

The `manager:` prefix in each key is required. Each manager's page covers its
prerequisites, package names, and the commands mise runs.

| Manager                                                     | Runs on                                           | Version pins                                        | `state = "absent"` | Import, prune                             |
| ----------------------------------------------------------- | ------------------------------------------------- | --------------------------------------------------- | ------------------ | ----------------------------------------- |
| [`apk`](/bootstrap/packages/apk.html)                       | Alpine Linux                                      | Yes                                                 | No                 | No                                        |
| [`apt`](/bootstrap/packages/apt.html)                       | Debian, Ubuntu, and derivatives                   | Yes                                                 | No                 | No                                        |
| [`aur`](/bootstrap/packages/aur.html)                       | Arch-based Linux with `yay` or `paru`             | No                                                  | No                 | No                                        |
| [`brew`](/bootstrap/packages/brew.html)                     | macOS on Apple Silicon, Linux x86_64 and arm64    | No; use a versioned formula such as `postgresql@17` | No                 | Import and prune                          |
| [`brew-cask`](/bootstrap/packages/brew-cask.html)           | macOS; font casks on Linux                        | No                                                  | No                 | Prune casks mise installed                |
| [`dnf`](/bootstrap/packages/dnf.html)                       | Fedora, RHEL, and derivatives                     | Yes                                                 | No                 | No                                        |
| [`flatpak`](/bootstrap/packages/flatpak.html)               | Linux with `flatpak`; system-wide installation    | No                                                  | No                 | No                                        |
| [`flatpak-user`](/bootstrap/packages/flatpak.html)          | Linux with `flatpak`; per-user installation       | No                                                  | No                 | No                                        |
| [`macos-app`](/bootstrap/packages/macos-app.html)           | macOS                                             | Required                                            | No                 | No                                        |
| [`mas`](/bootstrap/packages/mas.html)                       | macOS with `mas` from mise `[tools]` or on `PATH` | No                                                  | No                 | No                                        |
| [`nix`](/bootstrap/packages/nix.html)                       | Linux and macOS with `nix` on `PATH`              | No; pin the flake source instead                    | No                 | No; can export to NixOS                   |
| [`pacman`](/bootstrap/packages/pacman.html)                 | Arch-based Linux                                  | No                                                  | Yes                | No                                        |
| [`scoop`](/bootstrap/packages/scoop.html)                   | Windows with Scoop                                | Yes; `upgrade` skips pinned entries                 | Yes                | No                                        |
| [`winget`](/bootstrap/packages/winget.html)                 | Windows with WinGet                               | Yes                                                 | No                 | No                                        |
| [`zypper`](/bootstrap/packages/zypper.html)                 | openSUSE and SUSE Linux Enterprise                | Yes                                                 | Yes                | No                                        |
| [Package manager plugins](/bootstrap/packages/plugins.html) | Set by each plugin                                | Set by each plugin                                  | No                 | Prune, when the plugin supports uninstall |

## Declare packages

Each key is `"manager:package"`. The value is a version string or a table of
options:

```toml
[bootstrap.packages]
"apt:libssl-dev" = "latest"
"apt:curl" = "8.5.0-2ubuntu10"
"brew-cask:1password" = { os = "macos" }
"brew-cask:font-jetbrains-mono" = { os = ["linux", "macos"] }
"winget:BurntSushi.ripgrep.MSVC" = { os = "windows" }
"brew-cask:slack" = { env = "work" }
```

### Entry options

| Key                         | Managers              | Meaning                                                                                                                                                                                                                            |
| --------------------------- | --------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `version`                   | All                   | `"latest"` (the default) or a [version pin](#version-pins) in the manager's own format.                                                                                                                                            |
| `os`                        | All                   | Install only on these operating systems; see [By operating system](#by-operating-system).                                                                                                                                          |
| `env`                       | All                   | Install only when one of these [config environments](/configuration/environments.html) is active; see [By config environment](#by-config-environment).                                                                             |
| `state`                     | pacman, scoop, zypper | `"present"` (the default) or `"absent"`; see [Remove packages](#remove-a-package-declaratively).                                                                                                                                   |
| `adopt`                     | brew-cask, macos-app  | Take over an identical app that is already installed instead of replacing it; see [brew-cask](/bootstrap/packages/brew-cask.html#adopt-an-existing-app) and [macos-app](/bootstrap/packages/macos-app.html#adopt-an-existing-app). |
| `appdir`                    | brew-cask             | Application directory for this cask; see [Application directory](/bootstrap/packages/brew-cask.html#overriding-the-application-directory).                                                                                         |
| `url`, `sha256`, `artifact` | macos-app             | The download to install; see [Declare a download](/bootstrap/packages/macos-app.html#declare-a-download).                                                                                                                          |

mise warns about and ignores `adopt`, `appdir`, `url`, `sha256`, and `artifact`
on managers that do not use them. Any other key makes the config file fail to
parse.

### Version pins

A pin uses the manager's own version format, such as `"8.5.0-2ubuntu10"` for
apt. Status reports a package installed at another version as
`version mismatch`, and `apply` installs the pinned version. A `"latest"` entry
accepts any installed version.

For managers that cannot install a pin (see the
[table](#supported-package-managers)), status still reports the mismatch, and
`apply` and `upgrade` skip the entry with a warning. For `brew` and `brew-cask`,
the `@` in names such as `postgresql@17` is part of the Homebrew name, not a pin.

To write a pin from the command line, append `@<version>` to the package in
[`mise bootstrap packages use`](#add-packages-with-use).

## Choose where packages apply {#choose-platforms}

By default, mise acts on every entry whose manager is available on the current
machine. Entries for unavailable managers are skipped, and `status` lists them
as `skipped` with the reason, so one config can list packages for several
platforms. A Linux machine with both apt and the built-in Homebrew manager
installs both sets of declarations.

### By operating system

Use `os` to restrict an entry to an operating system, or to an OS and
architecture pair. It takes one value or a list, with the same names as
[OS-specific tools](/dev-tools/#os-specific-tools), such as `linux`, `macos`,
`windows`, `unix`, `linux/x64`, and `macos/arm64`:

```toml
[bootstrap.packages]
"brew-cask:1password" = { os = "macos" }
"brew:jq" = { os = ["macos/arm64", "linux"] }
```

Windows builds of mise do not include the Homebrew managers. There, `brew:`
entries are ignored, and `brew-cask:` and `macos-app:` entries warn as unknown
managers. Add `os = ["macos", "linux"]` to Homebrew entries in a config shared
with Windows machines.

### By config environment

Use `env` to restrict an entry to one or more
[config environments](/configuration/environments.html). It takes one name or a
list, and the entry applies only when one of them is active, through `-E`,
`MISE_ENV`, `env` in `.miserc.toml`, or a platform environment enabled by
[`auto_env`](/configuration/environments.html#platform-environments):

```toml
[bootstrap.packages]
"brew-cask:slack" = { env = "work" }
"apt:postgresql" = { env = ["server", "ci"] }
```

```sh
mise -E work bootstrap packages apply
```

The same applies to declarations in `mise.macos.toml` or `mise.linux.toml`:
they count only while that environment is active, so enable
[`auto_env`](/configuration/environments.html#platform-environments) or pass
`-E`. The file name alone does not activate it.

### By package manager {#choosing-which-managers-run}

Pass `--manager` to act on one manager for a single command. To limit which
managers run on a machine, set
[`system_packages.managers`](/configuration/settings.html#system_packages.managers):

```toml
[settings]
system_packages.managers = ["apt"]
```

Status lists entries for excluded managers as `skipped`, and naming an excluded
manager with `--manager` is an error.

## Remove packages {#remove-a-package-declaratively}

`pacman`, `scoop`, and `zypper` accept `state = "absent"`:

```toml
[bootstrap.packages]
"pacman:libreoffice-fresh" = { state = "absent" }
"scoop:neovim" = { state = "absent" }
```

When an absent package is installed, `status` reports it as
`unexpectedly installed`, and `apply` removes it. Each manager's page describes
the command it runs. Other managers report the same drift, but `apply` fails
instead of removing the package.

Deleting an entry from `mise.toml` never uninstalls anything. Use
[`prune`](#import-and-prune) for explicit cleanup where the manager supports it.

## How declarations combine {#semantics}

Package declarations merge across the
[config hierarchy](/configuration.html#configuration-hierarchy). A project adds
packages to the global list, and an entry with the same key in a more local file
replaces the global one, including its version and `state`.

An entry for an unknown manager produces a warning with a hint to install a
package plugin, and the entry is ignored. This keeps a config readable by mise
versions that do not have that manager yet.

If two active entries name the same package with different spellings and
disagree on version or state, mise reports an error. WinGet IDs and Scoop app
names are case-insensitive, and Scoop ignores the bucket prefix, so
`scoop:Git` and `scoop:extras/git` are the same app. Spell the key the same way
in every file so the normal override applies. Entries filtered out by `os` or
`env` are not checked.

## Commands

The [`mise bootstrap packages`](/cli/bootstrap/packages.html) reference lists
every subcommand and flag.

### Check status

[`mise bootstrap packages status`](/cli/bootstrap/packages/status.html) compares
the active declarations with what is installed:

```sh
mise bootstrap packages status
mise bootstrap packages status --json
mise bootstrap packages status --missing # exit 1 if anything is out of sync
```

Status does not install anything or use sudo. A package can be
`installed`, `missing`, `version mismatch`, `needs repair`, `absent`,
`unexpectedly installed`, or `skipped` when its manager is unavailable.

### Apply {#apply-or-record-packages}

[`mise bootstrap packages apply`](/cli/bootstrap/packages/apply.html) installs
missing and mismatched packages and removes `absent` ones:

```sh
mise bootstrap packages apply --dry-run
mise bootstrap packages apply --manager apt
mise bootstrap packages apply --update --yes
mise bootstrap packages apply apt:curl
```

`--dry-run` prints the commands without running them. `--manager` limits the
run to one manager and fails if that manager is unavailable. In an interactive
terminal, mise asks before it installs or removes anything; `--yes` skips that
prompt but does not supply a sudo password.

`--update` refreshes the manager's package metadata first, for example with
`apt-get update`, `apk add --update-cache`, `dnf install --refresh`,
`pacman -Sy`, `zypper refresh`, `scoop update`, or `winget source update`.
`brew`, `brew-cask`, `macos-app`, `flatpak`, and `mas` ignore it.

Packages named on the command line, such as `apt:curl`, are installed without
being recorded in a config file, and an unavailable manager is an error.

### Add packages with use

[`mise bootstrap packages use`](/cli/bootstrap/packages/use.html) writes
declarations and installs what is missing:

```sh
mise bootstrap packages use apt:curl
mise bootstrap packages use apt:curl@8.5.0-2  # writes "apt:curl" = "8.5.0-2"
mise bootstrap packages use brew:postgresql@17 # @17 is part of the formula name
mise bootstrap packages use -g brew-cask:firefox
```

`use` writes to the local `mise.toml` by default. `-g` writes to the global
config, `-e <env>` to `mise.<env>.toml`, and `-p <path>` to a specific file.
With `-g`, new entries go to
[`write_targets.packages`](/configuration/settings.html#write_targets.packages)
when that setting is set; an entry that already exists is updated where it is
declared.

Append `@<version>` to write a pin. `@latest`, or no `@`, writes `"latest"`.
`brew:` and `brew-cask:` names keep the `@` as part of the name, and `mas:` and
`nix:` reject a pin. `--dry-run` prints the entries and commands without writing
anything.

Entries for a manager that is unavailable on this machine are written without
installing, so you can add an `apt:` declaration from a Mac. `--no-install`
writes declarations without checking or installing anything.

### Upgrade installed packages

[`mise bootstrap packages upgrade`](/cli/bootstrap/packages/upgrade.html)
updates configured packages that are already installed, refreshing the
manager's package metadata first where it has any:

```sh
mise bootstrap packages upgrade --dry-run
mise bootstrap packages upgrade --manager apt
```

Missing packages are skipped with a warning; use `apply` to install them. The
manager decides which version is current. Managers that honor pins upgrade a
pinned entry to its pin, and each manager's page says what it runs.

### Import and prune

[`mise bootstrap packages import`](/cli/bootstrap/packages/import.html) records
Homebrew formulae that are already installed:

```sh
mise bootstrap packages import --manager brew --dry-run
mise bootstrap packages import --manager brew
```

It writes `"brew:<formula>" = "latest"` for each formula installed on request;
`--all` includes dependencies. `--global` follows `write_targets.packages` in
the same way as `use -g`.

[`mise bootstrap packages prune`](/cli/bootstrap/packages/prune.html)
uninstalls packages that no configuration declares. Always preview it first:

```sh
mise bootstrap packages prune --manager brew --dry-run
```

Prune keeps every package declared by the current configuration or by any
trusted config file mise tracks for other projects, in every config
environment. It defaults to `--manager brew`. What it removes depends on the
manager:

| Manager         | What prune removes                                                                                                                                                                     |
| --------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| brew            | Linked formulae that are neither declared nor needed by a declared formula or cask, including ones Homebrew installed; see [brew](/bootstrap/packages/brew.html#importing-and-pruning) |
| brew-cask       | Only casks mise installed that are unchanged since; see [brew-cask](/bootstrap/packages/brew-cask.html#prune)                                                                          |
| Package plugins | Only packages mise installed, when the plugin supports uninstall; see [plugins](/bootstrap/packages/plugins.html#prune)                                                                |

### Export to NixOS

`mise bootstrap packages export --format nix` writes `nix:` declarations as a
NixOS module instead of installing them; see
[Export to NixOS](/bootstrap/packages/nix.html#export-to-nixos).

### Locate a Homebrew formula {#locate-an-installed-package}

`mise bootstrap packages where brew:<formula>` prints the formula's stable
`opt` directory for scripts and build flags. It supports only `brew:` formulae;
see [Locate an installed formula](/bootstrap/packages/brew.html#locate-an-installed-formula).

## Root privileges and sudo {#sudo}

apk, apt, dnf, pacman, and zypper need root to change packages. When mise is
not running as root, it runs their commands through `sudo` and logs each
command first. In an interactive terminal, sudo can prompt for your password.
Without a terminal, mise checks for passwordless sudo and, when there is none,
fails and prints the command to run yourself. If `sudo` is not installed, mise
asks you to run the command as root.

Set [`system_packages.sudo`](/configuration/settings.html#system_packages.sudo)
to `false` to forbid elevation. Commands that need root then fail and print the
command instead.

Other managers handle privileges differently:

- AUR helpers build as your user and call sudo themselves to install the
  result, so `aur` needs the same sudo access and refuses to run as root.
- `flatpak` system installs are authorized by Flatpak's own polkit prompt, not
  sudo. `flatpak-user` needs no privileges.
- Homebrew formulae need sudo when the prefix does not exist yet, to create it
  and give it to your user
  ([brew](/bootstrap/packages/brew.html#the-prefix)). Casks may need it for
  pkg installers and some install steps
  ([brew-cask](/bootstrap/packages/brew-cask.html#supported-artifacts)).
- `nix`, `mas`, `scoop`, and `winget` never use sudo. A Windows installer can
  still raise its own UAC prompt.
- Package plugins never use sudo and must not elevate themselves.

## Use in CI {#ci-usage}

Install bootstrap packages before project tools:

```sh
mise bootstrap packages apply --yes
mise install
```

Containers that run as root need no sudo.
`mise bootstrap --only packages,tools --yes` runs the same two
[bootstrap](/bootstrap.html) phases in one command.

To check for drift without installing anything, run
`mise bootstrap packages status --missing`, which exits 1 when a package is not
in its desired state. A `skipped` entry does not count as missing, so when a
required manager might be unavailable, also check `status --json`, where the
manager shows `"available": false`. `mise doctor` also warns about missing
packages.
