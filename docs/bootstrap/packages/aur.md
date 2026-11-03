---
description: "Install Arch User Repository packages from mise.toml with the yay or paru AUR helper."
---

# Arch User Repository packages (aur)

The `aur` manager builds and installs packages from the
[Arch User Repository](https://aur.archlinux.org/) with the `yay` or `paru`
helper. For packages from the official Arch repositories, use
[`pacman`](/bootstrap/packages/pacman.html).

```toml
[bootstrap.packages]
"aur:google-chrome" = "latest"
"aur:visual-studio-code-bin" = "latest"
```

```sh
mise bootstrap packages apply --manager aur --dry-run
mise bootstrap packages apply --manager aur
```

::: warning Review AUR packages before installing
AUR packages are user-submitted build scripts that Arch does not review. A
malicious PKGBUILD runs code as your user while it builds. mise runs the helper
with `--noconfirm`, so the helper does not stop to show you the PKGBUILD or its
diff. Read it yourself, for example on aur.archlinux.org, before the first
install and before each upgrade.
:::

## Prerequisites

Install a working AUR helper and `base-devel` first. The manager is available
on Linux when `pacman` and either `yay` or `paru` are on `PATH`. mise uses
`yay` if it is on `PATH`, otherwise `paru`.

Run mise as a regular user, not root. The helper builds as your user and calls
sudo itself to install the result, so mise refuses to run it as root, and it
needs the same [sudo access](/bootstrap/packages/#sudo) as the other Linux
managers.

## Package names

Use the AUR package name, such as `visual-studio-code-bin`. Status uses
`pacman -Qm`, so only packages that are in none of your configured pacman
repositories count, including an AUR package that provides the requested name.
A package with the same name from any configured repository, official or
third-party, does not satisfy an `aur:` entry, and installing the entry
replaces it.

## Version pins

AUR helpers always build the current PKGBUILD, so mise cannot install a pinned
version. A pinned entry shows as `version mismatch` while another version is
installed, and `apply` skips it with a warning. Use `"latest"`.

## What mise runs

| Operation                       | Command                                              |
| ------------------------------- | ---------------------------------------------------- |
| Check installed state (no sudo) | `pacman -Qm`                                         |
| Install                         | `yay -S --aur --noconfirm -- <packages>` (or `paru`) |
| `apply --update`                | Adds `--refresh` to the helper command               |
| Upgrade                         | `yay -S --aur --noconfirm --refresh -- <packages>`   |

`upgrade` rebuilds only the configured AUR packages that are installed, not
every foreign package on the machine. The helper can still install
dependencies while it builds them. A dry run prints the helper command; it does
not fetch or review the PKGBUILD for you.

## Remove packages

mise does not remove AUR packages. Deleting an entry leaves the package
installed; run `pacman -R` yourself.
