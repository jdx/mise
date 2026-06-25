# pacman <Badge type="warning" text="experimental" />

System packages for Arch-family Linux (Arch, Manjaro, EndeavourOS, ...).

```toml
[bootstrap.packages]
"pacman:openssl" = "latest"
"pacman:base-devel" = "latest"
```

## Behavior

- Package state is checked with `pacman -Q` (read-only, never elevates).
- Missing packages are installed with `pacman -S --noconfirm --needed`,
  elevated with sudo when necessary (see
  [sudo](/bootstrap/packages/#sudo)). `--needed` makes installs
  idempotent.
- If `/var/lib/pacman/sync` contains no databases (fresh containers), mise
  runs `pacman -Sy` automatically before installing. Force a refresh with
  `mise bootstrap packages apply --update`.
- `mise bootstrap packages upgrade` runs `pacman -Sy` and then upgrades only the
  configured packages. Note that Arch officially supports only full-system
  upgrades (`pacman -Syu`) — upgrading individual packages is a
  [partial upgrade](https://wiki.archlinux.org/title/System_maintenance#Partial_upgrades_are_unsupported),
  so prefer running `pacman -Syu` yourself on a rolling-release system.

::: warning
Arch repositories only carry the latest version of each package, so pacman
entries cannot be installed at a pinned version — `mise bootstrap packages apply`
skips pinned entries with a warning, though `mise bootstrap packages status` still
reports a `version mismatch` for them. AUR packages are not supported (they
require an AUR helper and building from source).
:::
