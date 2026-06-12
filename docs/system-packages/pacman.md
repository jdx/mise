# pacman <Badge type="warning" text="experimental" />

System packages for Arch-family Linux (Arch, Manjaro, EndeavourOS, ...).

```toml
[system.packages]
pacman = ["openssl", "base-devel"]
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
Arch packages are not versioned in the repositories — entries are plain
package names. AUR packages are not supported (they require an AUR helper
and building from source).
:::
