---
description: "Install and remove Arch Linux packages with pacman from mise.toml."
---

# Arch Linux packages (pacman)

The `pacman` manager installs and removes packages on Arch Linux, Manjaro,
EndeavourOS, and other Arch-based distributions. It uses
[sudo](/bootstrap/packages/#sudo) when mise is not running as root. For AUR
packages, use [`aur`](/bootstrap/packages/aur.html).

```toml
[bootstrap.packages]
"pacman:openssl" = "latest"
"pacman:base-devel" = "latest"
"pacman:libreoffice-fresh" = { state = "absent" }
```

```sh
mise bootstrap packages apply --manager pacman --dry-run
mise bootstrap packages apply --manager pacman
```

## Prerequisites

The manager is available on Linux when `pacman` is on `PATH`. On other machines,
its entries show as [`skipped`](/bootstrap/packages/#choose-platforms). If
`/var/lib/pacman/sync` has no package databases (fresh containers), mise runs
`pacman -Sy` before installing.

## Package names

Use the package name as `pacman -S` takes it, from any configured
repository. An installed package that provides the requested name counts as
installed.

## Version pins

Arch repositories carry only the current version of each package, so mise
cannot install a pin. A pinned entry shows as `version mismatch` while another
version is installed, and `apply` skips it with a warning.

## What mise runs

| Operation                       | Command                                                           |
| ------------------------------- | ----------------------------------------------------------------- |
| Check installed state (no sudo) | `pacman -Q` and `pacman -T`                                       |
| Install                         | `pacman -S --noconfirm --needed -- <packages>`                    |
| `apply --update`                | `pacman -Sy` first                                                |
| Upgrade                         | `pacman -Sy`, then `pacman -S --noconfirm --needed -- <packages>` |
| Remove                          | `pacman -R --noconfirm -- <packages>`                             |

## Upgrade

`mise bootstrap packages upgrade` refreshes the package databases and upgrades
only the configured packages. That is a
[partial upgrade](https://wiki.archlinux.org/title/System_maintenance#Partial_upgrades_are_unsupported),
which Arch does not support. On an Arch system, run `sudo pacman -Syu` yourself
to keep everything current, and use `upgrade` only where you accept that risk.
Entries satisfied by a package that provides the name are skipped, so the
provider is not replaced.

## Remove packages

An entry with `state = "absent"` is removed with `pacman -R --noconfirm`. This
works for packages from any configured repository. mise does not cascade the
removal or remove orphaned dependencies, so pacman refuses to remove a package
that another installed package still needs.
