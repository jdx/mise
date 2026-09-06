# RPM packages (dnf)

System packages for Red Hat-family Linux (Fedora, RHEL, CentOS Stream, Rocky,
Alma, ...).

```toml
[bootstrap.packages]
"dnf:openssl-devel" = "latest"
"dnf:postgresql-server" = "latest"
"dnf:bash" = "5.2.26-3.fc40" # version or version-release pin
```

## Preview and apply

```sh
mise bootstrap packages status
mise bootstrap packages apply --manager dnf --dry-run
mise bootstrap packages apply --manager dnf
```

These commands use the active `[bootstrap.packages]` declarations. To add and
install a package together, use `mise bootstrap packages use dnf:openssl-devel`.
The manager must be available on the host; an explicit `--manager dnf` fails
when it is unavailable.

## Behavior

- Package state is checked with `rpm -q` (read-only, never elevates).
- Missing packages are installed with `dnf install -y`, elevated with sudo
  when necessary (see [sudo](/bootstrap/packages/#sudo)).
- Version pins are passed to dnf as its native `name-version` /
  `name-version-release` syntax; a version-only pin is satisfied by any
  release of that version.
- `mise bootstrap packages apply --update` adds `--refresh` to force a metadata
  refresh; otherwise dnf manages its own metadata expiry.
- `mise bootstrap packages upgrade` runs `dnf upgrade -y --refresh` for the configured
  packages — only already-installed packages are touched.

## Version selection

The Fedora version-release above illustrates syntax; it is not portable across
RPM distributions or releases. Select a version available in the target's
enabled repositories. mise passes the constraint to dnf and does not add a
repository or fetch an archived RPM to satisfy it.

`"latest"` accepts an installed package. Use `upgrade` to request an update;
source packages and native dependency resolution remain dnf's responsibility.

::: info
Only `dnf` is supported — not legacy `yum`-only systems. On RHEL/CentOS 8+
and all current Fedora releases, `dnf` is the default.
:::
