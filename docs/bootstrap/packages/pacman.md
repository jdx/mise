# Arch packages (pacman)

System packages for Arch-family Linux (Arch, Manjaro, EndeavourOS, ...).

```toml
[bootstrap.packages]
"pacman:openssl" = "latest"
"pacman:base-devel" = "latest"
"pacman:libreoffice-fresh" = { state = "absent" }
```

For a rolling-release workstation, keep the whole system current using Arch's
supported full-system upgrade workflow before adding packages. mise's scoped
`upgrade` command is not a replacement for that workflow; see the partial-upgrade
limitation below.

## Preview and apply

```sh
mise bootstrap packages status
mise bootstrap packages apply --manager pacman --dry-run
mise bootstrap packages apply --manager pacman
```

These commands use the active `[bootstrap.packages]` declarations. To add and
install a package together, use `mise bootstrap packages use pacman:openssl`.
The manager must be available on the host; an explicit `--manager pacman` fails
when it is unavailable.

## Behavior

- Package state is checked with `pacman -Q` and `pacman -T` (read-only, never
  elevates). An installed package that satisfies the requested name through
  `Provides` counts as installed.
- Missing packages are installed with `pacman -S --noconfirm --needed`,
  elevated with sudo when necessary (see
  [sudo](/bootstrap/packages/#sudo)). `--needed` makes installs
  idempotent.
- Packages declared with `state = "absent"` are removed with
  `pacman -R --noconfirm`. Removal is based on pacman's installed package
  database, so it works the same for official Arch packages and packages from
  configured third-party repositories such as the Omarchy Package Repository.
  mise does not cascade to dependents or remove orphaned dependencies.
- If `/var/lib/pacman/sync` contains no databases (fresh containers), mise
  runs `pacman -Sy` automatically before installing. Force a refresh with
  `mise bootstrap packages apply --update`.
- `mise bootstrap packages upgrade` runs `pacman -Sy` and then upgrades only the
  configured packages. Requests satisfied through `Provides` are skipped to
  avoid replacing the installed provider. Arch officially supports
  only full-system upgrades (`pacman -Syu`) — upgrading individual packages is a
  [partial upgrade](https://wiki.archlinux.org/title/System_maintenance#Partial_upgrades_are_unsupported),
  so prefer running `pacman -Syu` yourself on a rolling-release system.

::: warning
Arch repositories only carry the latest version of each package, so pacman
entries cannot be installed at a pinned version — `mise bootstrap packages apply`
skips pinned entries with a warning, though `mise bootstrap packages status` still
reports a `version mismatch` for them. Declare packages that must be built from
the Arch User Repository with the separate [`aur:` manager](./aur.md).
:::
