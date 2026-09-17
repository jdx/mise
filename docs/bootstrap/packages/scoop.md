---
description: Converge local Windows packages with Scoop during mise bootstrap.
---

# Scoop

Windows command-line tools and applications via [Scoop](https://scoop.sh).

```toml
[bootstrap.packages]
"scoop:ripgrep" = "latest"
"scoop:extras/vscode" = "latest"
"scoop:neovim" = "0.11.0"
```

Use the app name Scoop shows in `scoop search`. Qualify it with a bucket —
`extras/vscode` — to install from a bucket other than `main`. mise adds a
qualified bucket that is missing with `scoop bucket add <bucket>` before
installing, so a shared config does not depend on the machine having run that
command already. A bucket Scoop does not know by name needs a one-time
`scoop bucket add <name> <repo>`; mise does not manage bucket remotes.

Declare the app name, not a manifest URL or local path. Scoop can install from
one, but it records the app under the name the manifest declares, which mise
cannot know — the entry would read as missing on every run. mise rejects those
declarations instead.

## Commands

```sh
mise bootstrap packages use scoop:ripgrep
mise bootstrap packages status
mise bootstrap packages apply --manager scoop
mise bootstrap packages apply --manager scoop --update
mise bootstrap packages upgrade --manager scoop
```

`mise bootstrap packages status` runs `scoop export` and compares each
installed version with an optional pin as an opaque string. `"latest"` is
satisfied by any installed version; use the upgrade command to move it to the
newest version in the configured buckets. An app Scoop reports as a failed
install is shown as needing repair, and applying the configuration reinstalls
it. Scoop installs an app under its own name whatever bucket it came from, so
mise matches installed state on the app name and ignores the bucket qualifier.

Applying the configuration passes `--no-update-scoop` so installing a package
does not sync Scoop and every bucket as a side effect. Use `--update` to run
`scoop update` first. Upgrade always runs it, because `scoop update <app>` syncs
buckets only when Scoop already considers itself outdated — within that window
an upgrade would compare against a stale bucket clone and find nothing to do.

## Version pins

`scoop install <app>@<version>` installs a pinned version by generating a
manifest for it, so pins work for apps that are not installed yet. Scoop skips
that command when the app is already installed at another version, so mise
uninstalls the app first and then installs the pin — the same uninstall
`scoop update` performs internally when it moves an app's version. Persisted
data under `persist\` is kept, because mise never passes `--purge`.

Not every version resolves: Scoop generates the pinned manifest from the
current one, and the pin fails if the upstream download for that version is
gone.

`mise bootstrap packages upgrade` skips pinned entries with a warning.
`scoop update` always installs the bucket's current version and cannot hold a
pin, so upgrading a pinned entry would move it off its pin.

## Removal

Scoop supports declarative removal:

```toml
[bootstrap.packages]
"scoop:neovim" = { state = "absent" }
```

Applying that runs `scoop uninstall neovim`, which keeps the app's persisted
data. `mise bootstrap packages import` and `prune` do not cover Scoop; removing
an entry from the configuration does not uninstall the app.

## Availability and scope

The manager is available only on Windows when Scoop's `scoop` shim is on
`PATH`. Shared configs may contain `scoop:` entries alongside Linux or macOS
package entries; unavailable managers are reported as skipped and do not block
the other platform's bootstrap.

mise installs into the current user's Scoop installation. Global installs
(`scoop install --global`) need administrator rights and are not managed here.
An app that exists only as a global install is reported as installed, and mise
does not upgrade or reinstall it. Because `scoop uninstall` without `--global`
exits successfully without touching a global install, `state = "absent"` on one
fails with a message telling you to run `scoop uninstall --global <app>` from
an elevated shell rather than reporting a removal that did not happen. Any
other Scoop packages in the same run are removed first, so one global install
does not strand the rest of the batch. An app installed in both scopes loses
its user-scope copy and then reports the global one the same way. A pinned
entry whose only install is global gets the pinned version installed into the
user scope, which then takes precedence for mise.
