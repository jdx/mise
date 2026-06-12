# dnf <Badge type="warning" text="experimental" />

System packages for RedHat-family Linux (Fedora, RHEL, CentOS Stream, Rocky,
Alma, ...).

```toml
[system.packages]
dnf = ["openssl-devel", "postgresql-server"]
```

## Behavior

- Package state is checked with `rpm -q` (read-only, never elevates).
- Missing packages are installed with `dnf install -y`, elevated with sudo
  when necessary (see [sudo](/system-packages/index.html#sudo)).
- Entries pass through to dnf verbatim, so dnf's native syntax works,
  including `name-version-release` pins.
- `mise system install --update` adds `--refresh` to force a metadata
  refresh; otherwise dnf manages its own metadata expiry.

::: info
Only `dnf` is supported — not legacy `yum`-only systems. On RHEL/CentOS 8+
and all current Fedora releases `dnf` is the default.
:::
