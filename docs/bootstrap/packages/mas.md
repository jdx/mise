---
description: "Install Mac App Store apps from mise.toml with the mas command, using numeric App Store IDs."
---

# Mac App Store apps (mas)

The `mas` manager installs Mac App Store apps with the
[`mas`](https://github.com/mas-cli/mas) command. Each package name is the
app's numeric App Store ID, such as `497799835` for Xcode.

```toml
[bootstrap.packages]
"mas:497799835" = "latest" # Xcode
```

```sh
mise bootstrap packages apply --manager mas --dry-run
mise bootstrap packages apply --manager mas
```

## Prerequisites

`mas` is macOS-only. mise uses a `mas` installed by your mise `[tools]`
configuration, or one on `PATH`. On other platforms, or when neither is found,
`mas:` entries show as [`skipped`](/bootstrap/packages/#choose-platforms).
Install it as a mise tool, or with the [`brew`](/bootstrap/packages/brew.html)
manager and the Homebrew prefix's `bin` directory on `PATH`:

```sh
mise use -g mas
# or
mise bootstrap packages use brew:mas
```

`mise bootstrap` applies packages before it installs `[tools]`. On a new Mac
where `mas` comes only from `[tools]`, the first run skips the `mas:` entries,
so run `mise bootstrap` a second time.

`mas` needs an Apple Account signed in to the App Store, and its operations can
also need macOS authentication and working Spotlight indexing. Paid apps must
already be purchased or claimed with that account; mise reports errors from
`mas` and does not buy or claim apps itself. Installing Xcode does not accept
its license or complete its first-launch setup.

## Find an app ID

`mas search xcode` lists matching apps with their IDs. You can also copy the
app's App Store URL: Xcode's contains `id497799835`. A
bundle identifier such as `com.apple.dt.Xcode` is not a valid package name, and
mise rejects it.

## Version pins

The App Store installs only the current version, so mise cannot install a pin.
A pinned entry shows as `version mismatch` while another version is installed,
and `apply` and `upgrade` skip it with a warning.
[`mise bootstrap packages use`](/cli/bootstrap/packages/use.html) rejects
`mas:<id>@<version>`.

## What mise runs

| Operation             | Command                                            |
| --------------------- | -------------------------------------------------- |
| Check installed state | `mas list --json`, or `mas list` on older versions |
| Install               | `mas install <ids>`                                |
| Upgrade               | `mas upgrade <ids>`                                |

Status shows the app name that `mas list` reports next to each installed ID,
and `--json` includes it as `name`:

```text
Manager  Package            Installed  State
mas      497799835 (Xcode)  16.2       installed
```

A missing app shows only its ID, because `mas list` reports only installed
apps. `apply --update` has no effect for `mas`.

## Remove packages

mise does not remove App Store apps. Deleting an entry leaves the app
installed; remove it yourself.
