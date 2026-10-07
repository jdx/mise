---
description: "Install Flatpak apps and runtimes system-wide or for the current user from mise.toml."
---

# Flatpak apps (flatpak)

The `flatpak` manager installs Flatpak apps and runtimes into the system-wide
installation, and `flatpak-user` installs them for the current user. Both use
the [`flatpak`](https://docs.flatpak.org/en/latest/flatpak-command-reference.html)
command.

```toml
[bootstrap.packages]
"flatpak:org.mozilla.firefox" = "latest"
"flatpak-user:org.gnome.Builder" = "latest"
```

```sh
mise bootstrap packages apply --manager flatpak --dry-run
mise bootstrap packages apply --manager flatpak
mise bootstrap packages apply --manager flatpak-user
```

## Prerequisites

Both managers are available on Linux when `flatpak` is on `PATH`. On other
machines, their entries show as
[`skipped`](/bootstrap/packages/#choose-platforms). mise does not install
Flatpak or configure remotes, so add a remote such as Flathub in the scope you
use before you apply.

For system-wide apps:

```sh
flatpak remote-add --system --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
mise bootstrap packages use flatpak:org.mozilla.firefox
```

For your user only:

```sh
flatpak remote-add --user --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
mise bootstrap packages use flatpak-user:org.gnome.Builder
```

A remote added in one scope is not available to the other. Check with
`flatpak remotes --system` or `flatpak remotes --user`.

mise does not run Flatpak with sudo. System-wide changes go through Flatpak's
own polkit authorization, which can prompt in a desktop session. Where no one
can answer that prompt, run mise as root or use `flatpak-user`, which needs no
authorization.

## Package names

Use the app or runtime ID that `flatpak install` takes, such as
`org.mozilla.firefox`. The two scopes are separate, so a config can declare the
same ID in both. When several remotes provide an ID, resolve the ambiguity in
Flatpak's configuration; a declaration cannot name a remote.

Installed apps get no mise shims. Start them from the desktop, or with
`flatpak run org.mozilla.firefox`.

## Version pins

Flatpak cannot install a specific older version through `flatpak install`, so
mise cannot install a pin. A pinned entry shows as `version mismatch` while
another version is installed, and `apply` and `upgrade` skip it with a
warning. Use `"latest"`.

## What mise runs

| Operation                       | Command                                               |
| ------------------------------- | ----------------------------------------------------- |
| Check installed state (no sudo) | `flatpak list --system --columns=application,version` |
| Install                         | `flatpak install --system --noninteractive <ids>`     |
| Upgrade                         | `flatpak update --system --noninteractive <ids>`      |

`flatpak-user` runs the same commands with `--user` in place of `--system`.
`apply --update` has no effect for Flatpak.

## Remove packages

mise does not remove Flatpak apps. Deleting an entry leaves the app installed;
run `flatpak uninstall` yourself.
