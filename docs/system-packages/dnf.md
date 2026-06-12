# dnf <Badge type="warning" text="experimental" />

System packages for RedHat-family Linux (Fedora, RHEL, CentOS Stream, Rocky,
Alma, ...).

```toml
[system.packages]
"dnf:openssl-devel" = "latest"
"dnf:postgresql-server" = "latest"
"dnf:bash" = "5.2.26-3.fc40" # version or version-release pin
```

## Behavior

- Package state is checked with `rpm -q` (read-only, never elevates).
- Missing packages are installed with `dnf install -y`, elevated with sudo
  when necessary (see [sudo](/system-packages/index.html#sudo)).
- Version pins are passed to dnf as its native `name-version` /
  `name-version-release` syntax; a version-only pin is satisfied by any
  release of that version.
- `mise system install --update` adds `--refresh` to force a metadata
  refresh; otherwise dnf manages its own metadata expiry.

::: info
Only `dnf` is supported — not legacy `yum`-only systems. On RHEL/CentOS 8+
and all current Fedora releases `dnf` is the default.
:::
