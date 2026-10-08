---
description: "Install Homebrew casks, such as macOS apps and fonts, from mise.toml without installing Homebrew."
---

# Homebrew casks (brew-cask)

The `brew-cask` manager installs Homebrew casks, mostly macOS apps and fonts,
without Homebrew. mise reads each cask from Homebrew's API, downloads its
artifact, checks the sha256 when the cask provides one, and installs app
bundles into `/Applications`.

```toml
[bootstrap.packages]
"brew-cask:firefox" = "latest"
"brew-cask:visual-studio-code" = "latest"
"brew-cask:font-jetbrains-mono" = { os = ["linux", "macos"] }
```

```sh
mise bootstrap packages apply --manager brew-cask --dry-run
mise bootstrap packages apply --manager brew-cask
```

For a vendor download that has no cask, use
[`macos-app`](/bootstrap/packages/macos-app.html). For command-line formulae,
use [`brew`](/bootstrap/packages/brew.html).

## Supported platforms

`brew-cask` runs on macOS on Apple Silicon and Intel. mise records casks under
`/opt/homebrew/Caskroom` on both. On an Intel Mac, that is not the
`/usr/local/Caskroom` of an Intel Homebrew installation, so casks that Homebrew
installed there are not detected. Casks that depend on formulae need Apple
Silicon, because the `brew` manager installs only Apple Silicon bottles on
macOS.

### Linux font casks

On Linux, `brew-cask` installs only font casks that have no `preflight` or
`postflight` steps. Fonts go into `$XDG_DATA_HOME/fonts`, which defaults to
`~/.local/share/fonts`:

```toml
[bootstrap.packages]
"brew-cask:font-heavy-data-nerd-font" = "latest"
```

Other casks in `[bootstrap.packages]` show as `skipped` on Linux, so macOS and
Linux can share one package list. An explicit request such as
`mise bootstrap packages apply brew-cask:firefox` fails with an
unsupported-platform error. Add `os = "macos"` to app casks to make the intent
clear.

## Cask names

Use the cask token Homebrew uses, such as `brew-cask:firefox`.
`brew-cask:homebrew/cask/firefox` names the same cask. A versioned token such
as `temurin@17` includes the `@` as part of the name. Casks from other taps use
their fully-qualified name, such as `brew-cask:owner/tap/app`; see
[Third-party taps](/bootstrap/packages/brew.html#third-party-taps).

## Version pins

A cask exists only at its current version, so mise cannot install a pin. A
pinned entry shows as `version mismatch` while another version is installed,
and `apply` and `upgrade` skip it with a warning.

## Application directory {#overriding-the-application-directory}

App bundles go into `/Applications` by default, as with Homebrew. To install
them somewhere else, such as a `~/Applications` folder that needs no sudo, set
`MISE_BREW_CASK_OPT_APPDIR`. Export it in your shell startup file to use it on
every run:

```sh
export MISE_BREW_CASK_OPT_APPDIR="$HOME/Applications"
```

To use another directory for one cask, set `appdir` on its entry. It takes
precedence over the environment variable, so a cask that macOS requires in
`/Applications` can stay there:

```toml
[bootstrap.packages]
"brew-cask:1password" = { appdir = "/Applications" }
```

The directory must be an absolute path without `..`, and it cannot be `/`.
`appdir` also accepts a leading `~/`. An empty `MISE_BREW_CASK_OPT_APPDIR` means
`/Applications`. Casks that install into `/Applications` go to the chosen
directory instead, keeping any subdirectory the cask asks for. Casks that
install under `$HOMEBREW_PREFIX/Applications` stay there. This matches
`brew install --cask --appdir`.

`appdir` also applies to cask dependencies installed with that cask. It affects
installs and upgrades only: mise does not move an app that is already
installed. A first install into an `appdir` that already holds an app of the
same name fails unless you [adopt](#adopt-an-existing-app) that app.

## Adopt an existing app

When mise first installs a cask into the default directory and an app of the
same name is already there, mise replaces it and prints a warning. Replacing a
bundle can make macOS [drop the app's privacy permissions](#macos-privacy-security-tcc).
To keep the existing app instead, adopt it:

```toml
[bootstrap.packages]
"brew-cask:firefox" = { adopt = true }
```

To adopt by default, set `adopt` under `[bootstrap.brew]`. An entry can opt
out with `adopt = false`:

```toml
[bootstrap.brew]
adopt = true

[bootstrap.packages]
"brew-cask:textmate" = "latest"
"brew-cask:replace-me" = { adopt = false }
```

As with `brew install --cask --adopt`, mise downloads and verifies the cask,
then takes over the existing app only if its content is identical to the
download. If it differs, the install fails and the app is left untouched.
[Self-updating apps](#self-updating-apps) are the exception: they are adopted
as they are, because the app may have updated itself.

Adoption applies only to a cask that is not installed yet. It also applies when
you name casks on the command line, so you can move apps under mise one at a
time:

```sh
mise bootstrap packages apply brew-cask:firefox
```

## Self-updating apps

Casks marked `auto_updates: true` in Homebrew, such as `firefox` and
`visual-studio-code`, update themselves. mise installs the current version once
and leaves the app alone on `apply`.

`upgrade` replaces such an app only when the installed app reports an older
version than the cask and the app is not running. Otherwise it skips the app
and leaves it to its own updater. For pkg-based casks such as `tailscale-app`,
mise compares the installed package receipts instead. The cask definition
decides this; you cannot override it per entry.

Status shows these entries as `installed (auto-updates)`, and `--json` adds
`"auto_updates": true`. The version shown is the one mise installed; the app
may have updated itself since.

## macOS privacy permissions {#macos-privacy-security-tcc}

When mise replaces an app bundle, as an upgrade does, macOS may drop the
Privacy & Security permissions you granted that app, such as Accessibility,
Screen Recording, and Full Disk Access. You may need to grant them again in
System Settings. mise prints a warning when it replaces an existing app. To
bring an app you installed by hand under mise without replacing it,
[adopt it](#adopt-an-existing-app).

## Supported artifacts

| Cask artifact                     | macOS                                                             | Linux                  |
| --------------------------------- | ----------------------------------------------------------------- | ---------------------- |
| App bundles (`app`)               | Installed into the application directory                          | No                     |
| Binaries and command wrappers     | Linked into `<prefix>/bin`                                        | No                     |
| Fonts (`font`)                    | Installed into `~/Library/Fonts`                                  | `$XDG_DATA_HOME/fonts` |
| Installer packages (`pkg`)        | Installed with `installer`, when the cask lists its `pkgutil` IDs | No                     |
| Script installers (`installer`)   | Run, except installers that read from stdin                       | No                     |
| Shell completions                 | Linked into the prefix                                            | No                     |
| Generic `artifact` entries        | Copied to the target the cask names                               | No                     |
| Manual pages (`manpage`)          | Not linked                                                        | Not linked             |
| Services and other artifact types | Not supported                                                     | Not supported          |

mise installs a cask's formula and cask dependencies first, and a declared
cask conflict fails before anything changes. It runs a cask's `preflight` and
`postflight` steps itself. Pkg installers and steps that need root go through
[mise's sudo handling](/bootstrap/packages/#sudo), so a run without a terminal
needs passwordless sudo or root, and fails instead of waiting for a password. A
cask that needs an artifact type or step mise does not implement fails with an
error; mise never falls back to running `brew`.

## Casks installed by Homebrew

If Homebrew installed a cask, mise counts it as installed and leaves it alone:
`apply` does nothing and `upgrade` skips it. Use Homebrew to upgrade or remove
it. If Homebrew's records for the cask show no version or several, mise stops
with an error that tells you how to repair them in Homebrew.

## Casks mise installed

mise records these casks in its own receipt in the Caskroom and does not write
Homebrew's cask metadata, so they stay mise-owned. Use mise, not Homebrew, to
upgrade or prune them.

A cask counts as installed while its app, font, binary, and completion files
are in place. Changes inside an installed app, such as a self-update, do not
make mise reinstall it. A broken or retargeted binary or completion link does,
on the next `apply`. So does an install that was interrupted.

## Upgrade

`mise bootstrap packages upgrade --manager brew-cask` installs the current
version of each configured cask whose installed version differs from
Homebrew's. Casks whose Homebrew version is `latest` are skipped, and
[self-updating apps](#self-updating-apps) follow their own rules.

## Prune

[`mise bootstrap packages prune --manager brew-cask`](/cli/bootstrap/packages/prune.html)
removes only casks that mise installed and that are unchanged since. It keeps
every cask declared in the current configuration or any trusted config file
mise tracks:

```sh
mise bootstrap packages prune --manager brew-cask --dry-run
```

Prune skips the following, and prints the reason for each: casks Homebrew
installed; adopted and self-updating apps; casks with pkg installers, script
installers, command wrappers, or generic artifacts; casks with install or
uninstall steps; casks whose files changed or are shared with another cask;
interrupted installs; and casks installed by a mise version that did not record
prune information, until an upgrade refreshes them. Prune never runs a cask's
`zap` cleanup. Casks cannot be imported.

Deleting a `brew-cask:` entry does not uninstall the cask until you prune.

## Troubleshooting

| Symptom                                                | What to do                                                                                                       |
| ------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------- |
| `already exists and is not owned`                      | Remove the existing app and run `apply` again, set `adopt = true` if it is identical, or manage it outside mise. |
| `cannot adopt ...: existing artifact is not identical` | The installed app is a different build. Remove it to install the cask's build, or drop the entry.                |
| `unsupported artifact type`                            | mise cannot install this cask. Install it with Homebrew, or remove the entry.                                    |
| An app lost its permissions after an upgrade           | Grant them again in System Settings under Privacy & Security.                                                    |
