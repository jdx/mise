# pacman <Badge type="warning" text="experimental" />

System packages for Arch-family Linux (Arch, Manjaro, EndeavourOS, ...).

```toml
[system.packages]
"pacman:openssl" = "latest"
"pacman:base-devel" = "latest"
```

## Behavior

- Package state is checked with `pacman -Q` (read-only, never elevates).
- Missing packages are installed with `pacman -S --noconfirm --needed`,
  elevated with sudo when necessary (see
  [sudo](/system-packages/index.html#sudo)). `--needed` makes installs
  idempotent.
- If `/var/lib/pacman/sync` contains no databases (fresh containers), mise
  runs `pacman -Sy` automatically before installing. Force a refresh with
  `mise system install --update`.

::: warning
Arch repositories only carry the latest version of each package, so pacman
entries cannot be installed at a pinned version — `mise system install`
skips pinned entries with a warning, though `mise system status` still
reports a `version mismatch` for them. AUR packages are not supported (they
require an AUR helper and building from source).
:::
