---
description: "Install, pin, and remove Windows apps with Scoop from mise.toml."
---

# Scoop apps (scoop)

The `scoop` manager installs, pins, and removes Windows apps with
[Scoop](https://scoop.sh) in your user scope.

```toml
[bootstrap.packages]
"scoop:ripgrep" = "latest"
"scoop:extras/vscode" = "latest"
"scoop:neovim" = "0.11.0"
```

```sh
mise bootstrap packages apply --manager scoop --dry-run
mise bootstrap packages apply --manager scoop
```

## Prerequisites

Install Scoop first. The manager is available on Windows when `scoop` is on
`PATH`. On other machines, `scoop:` entries show as
[`skipped`](/bootstrap/packages/#choose-platforms), so one config can hold
packages for several platforms.

## Package names

Use the app name that `scoop search` shows. To install from a bucket other than
`main`, qualify the name with the bucket, as in `extras/vscode`. Before it
installs, mise runs `scoop bucket add <bucket>` for a qualified bucket Scoop
does not have yet, so a shared config does not depend on that step. A bucket
that Scoop does not know by name needs a one-time
`scoop bucket add <name> <repository>`; mise does not manage bucket URLs.

Declare the app name, not a manifest URL or local path. Scoop records such an
app under the name its manifest declares, which mise cannot know, so mise
rejects those declarations.

Scoop app names are case-insensitive, and installed apps are matched by name
whichever bucket they came from, so `scoop:Git` and `scoop:extras/git` are the
same app. Declare each app once; mise rejects two spellings that disagree on
version or state.

## Version pins

A pin must match Scoop's version string exactly. mise installs it with
`scoop install <app>@<version>`, which generates a manifest for that version,
so a pin works even for an app that is not installed yet. A pin fails if the
upstream download for that version no longer exists.

To move an installed app to a pinned version, mise uninstalls it and installs
the pinned version. The app's `persist` data is kept.

`upgrade` skips pinned entries with a warning, because `scoop update` always
moves to the bucket's current version.

## What mise runs

| Operation             | Command                                    |
| --------------------- | ------------------------------------------ |
| Check installed state | `scoop export`                             |
| Install               | `scoop install --no-update-scoop <apps>`   |
| `apply --update`      | `scoop update` first                       |
| Upgrade               | `scoop update`, then `scoop update <apps>` |
| Remove                | `scoop uninstall <apps>`                   |

`apply` passes `--no-update-scoop`, so installing an app does not update Scoop
and every bucket. `upgrade` always runs `scoop update` first, so it sees the
latest bucket versions. An app that Scoop reports as a failed install shows as
`needs repair`, and `apply` reinstalls it.

## Remove packages {#removal}

```toml
[bootstrap.packages]
"scoop:neovim" = { state = "absent" }
```

`apply` runs `scoop uninstall neovim`, which keeps the app's persisted data.
Deleting an entry does not uninstall the app, and `import` and `prune` do not
support Scoop.

## Global installs {#availability-and-scope}

mise manages only the current user's Scoop apps. Apps installed with
`scoop install --global` need administrator rights and are handled like this:

- A global-only app counts as installed. mise never upgrades or reinstalls it.
- `state = "absent"` on a global-only app fails the run and tells you to run
  `scoop uninstall --global <app>` from an elevated shell. Other Scoop entries
  in the same run are still installed or removed, and the managers after Scoop
  still run.
- With `state = "absent"`, an app installed in both scopes loses its user
  copy, and the global copy is reported the same way.
- A pinned entry whose only install is global gets the pinned version installed
  in your user scope.
