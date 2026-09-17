---
description: "Declare, install, and maintain shared host packages with mise bootstrap."
socialDescription: "Manage native libraries, build dependencies, and host applications from mise.toml."
---

# Bootstrap Packages

Use `[bootstrap.packages]` to declare native libraries, build dependencies, and
applications shared by the whole machine. Apply them explicitly with
`mise bootstrap packages apply`, or install them alongside your tools with
[`mise bootstrap`](/bootstrap.html).

## Get started

Choose the package manager for your machine. For Debian or Ubuntu, add this to
`mise.toml`:

```toml
[bootstrap.packages]
"apt:libssl-dev" = "latest"
"apt:build-essential" = "latest"
```

Check what is installed, preview the changes, then apply them:

```sh
mise bootstrap packages status
mise bootstrap packages apply --dry-run
mise bootstrap packages apply
```

**`"latest"` accepts an already-installed version.** Applying the configuration
installs missing packages; it does not upgrade them on every run. Use
[`mise bootstrap packages upgrade`](#upgrade-installed-packages) to update
installed packages.

## Host packages or mise tools

Use `[bootstrap.packages]` for software that belongs in the host's package
database or shared installation prefix. These installations are shared across
projects: changing directories does not switch their versions, and mise does
not create shims for them.

Use [`[tools]`](/dev-tools/) when each project needs its own selected tool
versions. A project can use both: for example, a compiler from `[tools]` and
native development libraries from `[bootstrap.packages]`.

## Supported package managers

The manager prefix in `"manager:package"` is required. Each manager's page
explains its prerequisites, package names, and version support.

| Manager         | Platform and requirements                                          | Guide                                               |
| --------------- | ------------------------------------------------------------------ | --------------------------------------------------- |
| `apk`           | Alpine Linux                                                       | [apk](/bootstrap/packages/apk.html)                 |
| `apt`           | Debian, Ubuntu                                                     | [apt](/bootstrap/packages/apt.html)                 |
| `aur`           | Arch, Manjaro with yay or paru                                     | [AUR](/bootstrap/packages/aur.html)                 |
| `dnf`           | Fedora, RHEL, CentOS, Rocky, Alma                                  | [dnf](/bootstrap/packages/dnf.html)                 |
| `pacman`        | Arch, Manjaro                                                      | [pacman](/bootstrap/packages/pacman.html)           |
| `brew`          | macOS arm64; Linux x86_64/arm64; no Homebrew installation required | [Homebrew](/bootstrap/packages/brew.html)           |
| `brew-cask`     | macOS; font-only casks on Linux; no Homebrew installation required | [Casks](/bootstrap/packages/brew.html#casks)        |
| `macos-app`     | macOS; a declared app download URL and checksum                    | [Direct app downloads](#macos-apps-without-a-cask)  |
| `flatpak`       | Linux with `flatpak` on `PATH`; system scope                       | [Flatpak](/bootstrap/packages/flatpak.html)         |
| `flatpak-user`  | Linux with `flatpak` on `PATH`; user scope                         | [Flatpak](/bootstrap/packages/flatpak.html)         |
| `nix`           | Linux and macOS with `nix` on `PATH`; user profile                 | [Nix](/bootstrap/packages/nix.html)                 |
| `mas`           | macOS with `mas` on `PATH`                                         | [Mac App Store](/bootstrap/packages/mas.html)       |
| `scoop`         | Windows with Scoop's `scoop` shim on `PATH`                        | [Scoop](/bootstrap/packages/scoop.html)             |
| `winget`        | Windows with `winget` on `PATH`                                    | [WinGet](/bootstrap/packages/winget.html)           |
| Package plugins | Defined by each plugin                                             | [Package plugins](/bootstrap/packages/plugins.html) |

[Package manager plugins](/bootstrap/packages/plugins.html) extend this list
with other host packages, such as editor extensions and application plugins.
On Linux, `brew-cask` supports only font casks without lifecycle hooks or
structured flight steps; see the [cask guide](/bootstrap/packages/brew.html#casks).

## Declare packages

An entry's value is either a version string or a table of options:

```toml
[bootstrap.packages]
"brew:coreutils" = "latest"
"brew-cask:1password" = { os = "macos" }
"brew-cask:font-jetbrains-mono" = { os = ["linux", "macos"] }
"winget:BurntSushi.ripgrep.MSVC" = { os = "windows" }
```

In table form, `version` defaults to `"latest"`. Version pins use the manager's
native format and work only where that manager supports them.
[`macos-app`](#macos-apps-without-a-cask) requires an explicit version and
additional download fields.

### Choose platforms

Use `os` to restrict a package to an operating system or OS/architecture pair.
It accepts one value or a list, with the same names and aliases as `[tools]`,
such as `linux`, `macos`, `windows`, `linux/x64`, and `macos/arm64`.

Entries with a nonmatching selector are skipped. Managers unavailable on the
current machine are also skipped when applying the full configuration. Status
still lists unavailable managers so you can distinguish skipped packages from
installed ones. See [Choosing which managers run](#choosing-which-managers-run)
for a machine-wide selection.

### Remove a package declaratively

`pacman` and `scoop` support `state = "absent"`:

```toml
[bootstrap.packages]
"pacman:libreoffice-fresh" = { state = "absent" }
"scoop:neovim" = { state = "absent" }
```

If the package is installed, `status --missing` reports drift and `apply`
removes it. One exception: an app installed only in Scoop's global scope is
outside the user scope mise manages, so `apply` fails with the elevated
`scoop uninstall --global` command to run instead of removing it. See
[Scoop availability and scope](/bootstrap/packages/scoop.html#availability-and-scope).

Other built-in managers currently support only the default
`state = "present"`. Removing an entry from the configuration does not itself
uninstall a package; see [Import and prune](#import-and-prune).

### Adopt an existing Homebrew cask app

For `brew-cask`, set `adopt = true` to keep an existing app in place instead of
replacing it. Normally, its contents must match the downloaded app. Casks that
declare `auto_updates: true` can adopt a differing app because it may have
updated itself.

Set `[bootstrap.brew] adopt = true` to enable adoption for all casks, with
per-entry `adopt = false` overrides. See the
[cask adoption guide](/bootstrap/packages/brew.html#casks) for examples.
Direct [`macos-app` downloads](#adopt-an-existing-app) have their own stricter
adoption rules; the Homebrew default does not apply to them.

## Semantics

Package declarations merge across the [configuration hierarchy](/configuration.html).
A project can add packages to the global list or override an entry with the same
key, including its version or `state`. Packages declared elsewhere remain in
the combined list unless overridden.

Installation is explicit. `mise install` prints a one-time hint about missing
host packages; it does not install them. Run `mise bootstrap packages apply`,
`mise bootstrap packages use`, or the full `mise bootstrap` to install them.

Unknown managers produce a warning and a package-plugin installation hint.
Their entries are ignored, allowing a configuration to include managers a
particular mise installation does not yet support.

## Commands

### Apply or record packages

```sh
mise bootstrap packages status --json
mise bootstrap packages status --missing
mise bootstrap packages apply --manager apt --dry-run
mise bootstrap packages apply --manager apt
mise bootstrap packages apply --update
```

`apply` without package arguments reads the active configuration. An explicit
request such as `mise bootstrap packages apply apt:curl` installs a package
without recording it. A `macos-app:<name>` request must already have a download
declaration in the configuration.

`--update` refreshes package metadata before applying changes. `--yes` skips
mise's confirmation prompt but does not provide sudo credentials.

To record a package and install it, use `use`:

```sh
mise bootstrap packages use apt:curl
mise bootstrap packages use -g brew:ffmpeg
mise bootstrap packages use winget:BurntSushi.ripgrep.MSVC
```

`use` writes the declaration to the local `mise.toml`, or the global config with
`-g`, and installs what is missing. Entries for unavailable managers are written
without installation, so you can add an `apt:` declaration while working on a
Mac. Pass `--no-install` to write declarations without checking or installing
packages; this flag works with every package manager.

### Upgrade installed packages

```sh
mise bootstrap packages upgrade --manager apt --dry-run
mise bootstrap packages upgrade --manager apt
mise bootstrap packages upgrade --manager winget
```

`upgrade` refreshes manager metadata and updates configured packages that are
already installed. Missing packages are skipped; use `apply` to install them.
The manager determines which version is available and how it is installed.

Version pins remain subject to the manager's capabilities. For example, apk,
apt, and dnf honor configured pins. AUR, pacman, brew, brew-cask, flatpak,
flatpak-user, and mas cannot install pins, so pinned entries are skipped with a
warning. [`scoop`](/bootstrap/packages/scoop.html) installs pins but cannot hold
them, so `upgrade` skips its pinned entries. See the manager's guide for details.

For `macos-app`, there is no version discovery: update the declaration yourself
before applying or upgrading it. See [Update a declared app](#update-a-declared-app).

### Locate an installed package

`where` currently supports Homebrew formulae on macOS arm64 and Linux
x86_64/arm64, including keg-only formulae. It prints the formula's stable,
absolute `opt` root:

```sh
if package_root="$(mise bootstrap packages where brew:unzip)"; then
  export PATH="$package_root/bin:$PATH"
fi
```

Use the canonical formula name; aliases are not resolved. Qualified names such
as `brew:homebrew/core/unzip` and `brew:owner/tap/unzip` query the same local
`unzip` rack without checking its tap. Neither a declaration nor the Homebrew
executable is required.

A missing or invalid installation fails with empty stdout. Install it with
`mise bootstrap packages apply brew:unzip`, or follow the diagnostic to restore
its `opt` link.

This is a local lookup using environment variables and global CLI options.
It does not load project/global configuration or `.miserc.toml`, evaluate their
executable templates, or run automatic updates and startup housekeeping.
See [formula roots](/bootstrap/packages/brew.html#locate-an-installed-formula)
for details.

### Import and prune

Import existing Homebrew formulae to record them in your configuration:

```sh
mise bootstrap packages import --manager brew --dry-run
mise bootstrap packages import --manager brew
```

Import reads active Homebrew `opt` links and writes
`"brew:<formula>" = "latest"` entries. By default it includes only formulae whose
keg receipts mark them as explicitly requested; `--all` includes dependencies.
These declarations also protect imported formulae from subsequent pruning.

Pruning is an explicit removal operation. It preserves packages needed by the
current configuration or by trusted, loadable tracked configurations. Always
inspect the plan first:

```sh
mise bootstrap packages prune --manager brew --dry-run
```

Remove `--dry-run` to apply the plan. The scope depends on the manager:

- **`brew`** removes linked formulae that are no longer needed, including
  formulae installed by Homebrew itself. This is also the default manager when
  `--manager` is omitted.
- **`brew-cask`** removes only mise-owned direct artifacts with current receipts
  and unchanged content fingerprints. It skips Homebrew-owned casks, older
  receipts, pkg, installer, command-wrapper and generic artifacts, lifecycle
  actions, changed or shared targets, and incomplete transactions. Each skip
  includes a reason; `zap` metadata is never applied.
- **Package plugins** remove only packages mise observed changing from missing
  to installed during `PackageInstall`. Existing or manually installed
  packages are not adopted. The plugin must implement `PackageUninstall`;
  mise verifies removal with `PackageInstalled` before updating ownership.
  Dry runs print the removal batch without invoking the uninstall hook.

`macos-app` does not support pruning.

## macOS apps without a cask

Use `macos-app` for a vendor download or internal app that has no Homebrew cask.
Prefer `brew-cask` when a suitable cask exists: it supplies download metadata
and tracks releases for you.

### Declare a download

Add a table with all four required fields. This example uses a placeholder URL
and checksum; replace both with the values for your app:

```toml
[bootstrap.packages."macos-app:example"]
version = "1.2.3"
url = "https://example.com/Example-{{version}}-arm64.dmg"
sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
artifact = "Example.app"
os = "macos/arm64"
```

| Field      | Meaning                                                                                                                                               |
| ---------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| `version`  | The explicit release to install. `latest` is not supported.                                                                                           |
| `url`      | The archive download URL. `{{version}}` is replaced with the declared version. A `.git` URL is rejected: a clone cannot be verified against `sha256`. |
| `sha256`   | The archive's SHA-256 checksum: exactly 64 hexadecimal characters. Homebrew's `no_check` value is not accepted.                                       |
| `artifact` | The app bundle to install from the archive, such as `Example.app`.                                                                                    |

Choose a download for your Mac's architecture; the optional `os` selector above
limits this example to Apple Silicon. Use HTTPS where possible. Non-HTTPS URLs
produce a warning; checksum verification is still required.

Preview and install the declared app:

```sh
mise bootstrap packages apply macos-app:example --dry-run
mise bootstrap packages apply macos-app:example
```

mise verifies the archive's checksum and installs the app into `/Applications`
using the cask installer. This manager supports app bundles in `.dmg` and `.zip`
archives, not `pkg` installers, command-line binaries, or fonts. To choose
another app directory, use
[`MISE_BREW_CASK_OPT_APPDIR`](/bootstrap/packages/brew.html#overriding-the-application-directory).
Receipts are stored in mise's state directory, separately from Homebrew's
Caskroom.

### Adopt an existing app

If the destination already contains an app this entry does not own, installation
is refused by default. To take over an identical app without replacing its
bundle, add this to the declaration:

```toml
adopt = true
```

mise compares the installed bundle with the download. If they differ, adoption
fails and leaves the existing app untouched. Remove the existing app first if
you want to install a different build.

Adoption records ownership and authorizes later replacements by mise. If
Homebrew or another manager installed the app, reconcile that manager's record
before letting mise manage future updates. Separate receipts do not prevent
two managers from targeting the same app.

Keeping the existing bundle in place avoids a replacement that may require you
to grant macOS Privacy & Security permissions again. This is stricter than the
default `brew-cask` behavior, which warns before replacing an existing app.

The same explicit adoption is required after an interrupted installation that
left an app without a completed ownership receipt. A pending transaction alone
does not establish ownership. Changing `artifact` or the app directory also
requires mise to assess ownership of the new destination.

An app that appears at the destination while mise is staging its own bundle is
refused the same way, and left untouched:

```
macos-app: '/Applications/Example.app' was created by something else while this
app was being staged; it was left untouched
```

A dry run warns about an existing unowned app, but cannot determine whether
adoption will succeed: it has not downloaded the archive to compare contents.

### Update a declared app

Change `version` and `sha256` for each release, and update `url` if it does not
use `{{version}}` or the vendor has changed the URL format. Then run:

```sh
mise bootstrap packages apply macos-app:example
```

`upgrade` can also install the newly declared version when the app is already
installed. Neither command discovers releases from a plain download URL. With
an unchanged declaration and a matching installed version, there is no update
to apply.

## Choosing which managers run

By default, mise acts on every configured manager available on the current
machine. Availability depends on the platform and required commands; mise
does not choose one preferred manager. A Linux host can use both apt and the
built-in Homebrew manager if both have declarations.

Use `--manager` to select a manager for one command. To limit which managers run
across commands, set
[`system_packages.managers`](/configuration/settings.html#system_packages.managers):

```toml
[settings]
system_packages.managers = ["apt"]
```

Per-package `os` selectors provide finer control. If you put selections in
`mise.macos.toml` or `mise.linux.toml`, activate that configuration environment
with `-E`/`MISE_ENV` or enable
[`auto_env`](/configuration/environments.html#platform-environments). The filename
alone does not activate it.

## sudo

apk, apt, dnf, and pacman need root for package changes. mise uses sudo when
necessary, with the following behavior:

- **Already root:** commands run directly, without sudo.
- **Interactive terminal:** sudo can prompt normally for a password.
- **Non-interactive without passwordless sudo:** mise fails and prints the
  command to run manually instead of waiting for a password.

mise logs the full command before running it. Set
[`system_packages.sudo = false`](/configuration/settings.html#system_packages.sudo)
to forbid elevation; mise prints the command for you to run instead.

AUR helpers build as the current user and handle their own installation
privileges. Flatpak user installations do not need root. Homebrew formulae may
need elevation to create the canonical prefix, and casks may need it to install
artifacts; see the [Homebrew guide](/bootstrap/packages/brew.html).
Package plugins never use mise's sudo path and must not elevate themselves.

## CI usage

Install host packages before project tools:

```sh
mise bootstrap packages apply --yes
mise install
```

In containers running as root, these commands do not need sudo prompts.
[`mise bootstrap --yes`](/bootstrap.html) combines both steps and then runs a
task named `bootstrap`, if one is defined.

Use `mise bootstrap packages status --missing` to check for drift without
installing anything; it exits 1 when requirements are unmet. Inspect
`status --json` as well if a required manager might be unavailable: a skipped
declaration does not prove the package is installed. `mise doctor` also reports
configured host packages and warns about missing ones.

For NixOS, you can [export a module](/bootstrap/packages/nix.html#export-to-nixos)
with `mise bootstrap packages export --format nix`.
