# Arch User Repository (AUR)

The `aur` manager installs packages from the
[Arch User Repository](https://aur.archlinux.org/) with `yay` or `paru`:

::: warning Review AUR packages before installing
AUR packages are user-submitted build recipes, not packages vetted or supported
by Arch Linux. A compromised or malicious PKGBUILD can execute code as your user
during the build. Review the PKGBUILD and related sources before installing a
package, and review upstream changes before applying upgrades. mise delegates to
your AUR helper and does not add an independent trust or verification layer.
:::

```toml
[bootstrap.packages]
"aur:google-chrome" = "latest"
"aur:visual-studio-code-bin" = "latest"
```

Install a working AUR helper and its build prerequisites before applying this
configuration. Run as a regular user, not root.

mise prefers `yay` when both helpers are on `PATH`,
and otherwise uses `paru`. The helper runs as the current user because AUR
packages are built with `makepkg`; the helper requests elevation from pacman
when it installs the finished package.

Package state is checked read-only through pacman's local database with its
foreign-package filter. A same-named package from a configured repository does
not satisfy an `aur:` declaration. Virtual package names are accepted only when
their installed provider is also a foreign package. Installs use the helper's
AUR-only mode with `--noconfirm`, so a repository package with the
same name is not selected instead. mise intentionally omits `--needed` so the
helper can replace an installed repository package that has the requested AUR
package's name. `mise bootstrap packages apply --update`
additionally asks the helper to refresh repository metadata.

AUR helpers build the current PKGBUILD rather than resolving historical package
versions, so version pins are status-only. Use `"latest"` for entries mise can
install automatically.

```sh
mise bootstrap packages status
mise bootstrap packages apply --manager aur --dry-run
mise bootstrap packages apply --manager aur
mise bootstrap packages upgrade --manager aur
```

`upgrade` rebuilds only the configured AUR packages. It does not upgrade every
foreign package on the machine. The helper can still resolve dependencies while
building those packages. A dry run shows the helper invocation; it does not
fetch and review the PKGBUILD for you.
