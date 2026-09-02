# Arch User Repository (AUR)

The `aur` manager installs packages from the
[Arch User Repository](https://aur.archlinux.org/) with `yay` or `paru`:

```toml
[bootstrap.packages]
"aur:google-chrome" = "latest"
"aur:visual-studio-code-bin" = "latest"
```

mise prefers `yay` when both helpers are on `PATH`, matching Omarchy's default,
and otherwise uses `paru`. The helper runs as the current user because AUR
packages are built with `makepkg`; the helper requests elevation from pacman
when it installs the finished package.

Package state is checked read-only through pacman's local database with its
foreign-package filter. A same-named package from a configured repository does
not satisfy an `aur:` declaration. Virtual package names are accepted only when
their installed provider is also a foreign package. Installs use the helper's
AUR-only mode with `--noconfirm` and `--needed`, so a repository package with the
same name is not selected instead. `mise bootstrap packages apply --update`
additionally asks the helper to refresh repository metadata.

AUR helpers build the current PKGBUILD rather than resolving historical package
versions, so version pins are status-only. Use `"latest"` for entries mise can
install automatically.

```sh
mise bootstrap packages status --manager aur
mise bootstrap packages apply --manager aur
mise bootstrap packages upgrade --manager aur
```

`upgrade` rebuilds only the configured AUR packages. It does not upgrade every
foreign package on the machine.
