# Mac App Store applications (mas)

Mac App Store apps via the [`mas`](https://github.com/mas-cli/mas) CLI.

```toml
[bootstrap.packages]
"mas:497799835" = "latest"       # Xcode
```

`mas` apps are part of `[bootstrap.packages]`, just like apt packages,
Homebrew formulae, and casks. The package name is the App Store app ID:
a numeric ADAM ID accepted by `mas install` and `mas upgrade`.

mise needs the `mas` CLI before it can inspect or install these apps. Install
it as a mise tool:

```sh
mise use -g mas
```

Or install it through the built-in [brew manager](/bootstrap/packages/brew.html)
and ensure the Homebrew prefix's `bin` directory is on `PATH`:

```sh
mise bootstrap packages use brew:mas
```

Declaring `mas` in `[tools]` alone does not make a fresh full bootstrap install
it before the built-in packages phase. Install the CLI first, then apply the App
Store declarations. Confirm `mas` can see the current user's App Store state
with `mise exec -- mas list`.

## Commands

```sh
mise bootstrap packages use mas:497799835
mise bootstrap packages status
mise bootstrap packages apply --manager mas --dry-run
mise bootstrap packages apply --manager mas
mise bootstrap packages upgrade --manager mas
```

`mise bootstrap packages apply` runs `mas install <id>` for missing apps.
`mise bootstrap packages upgrade` runs `mas upgrade <id>` for installed apps.
Both commands require numeric ADAM IDs; bundle identifiers such as
`com.apple.dt.Xcode` are not valid package names.

## Caveats

`mas` is macOS-only and must be on `PATH`. On other platforms, or when the
`mas` command is missing, shared configs list the entries as skipped instead
of failing. Explicit commands such as `mise bootstrap packages apply
--manager mas` still fail when `mas` is unavailable, matching the other
package managers.

Mac App Store operations may require an Apple Account signed in to the App
Store, macOS authentication, prior purchase/claiming for paid apps, and valid
Spotlight indexing. mise surfaces errors from `mas` rather than trying to
purchase or claim apps itself.

App Store version pins are not supported by these commands. `"latest"` accepts
an installed app; use `mise bootstrap packages upgrade --manager mas` when you
want mise to request an update. App installation does not by itself accept
Xcode's license or complete its first-launch setup.

## Finding IDs

Use `mas search` or copy an App Store URL and extract the numeric ID:

```sh
mas search xcode
```

For example, Xcode's App Store URL contains `id497799835`, so the package
entry is:

```toml
[bootstrap.packages]
"mas:497799835" = "latest"
```
