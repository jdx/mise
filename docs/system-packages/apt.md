# apt <Badge type="warning" text="experimental" />

System packages for Debian-family Linux (Debian, Ubuntu, Mint, ...).

```toml
[system.packages]
apt = [
  "libssl-dev",
  "curl=8.5.0-2ubuntu10", # version pin
  "gcc:arm64",            # architecture qualifier
]
```

## Behavior

- Package state is checked with `dpkg-query` (read-only, never elevates).
- Missing packages are installed with `apt-get install -y`, elevated with
  sudo when necessary (see [sudo](/system-packages/index.html#sudo)).
- Entries pass through to apt verbatim, so apt's native syntax works:
  `name=version` pins and `name:arch` qualifiers.
- `DEBIAN_FRONTEND=noninteractive` is set so installs never block on
  configuration prompts.

## Metadata refresh

If `/var/lib/apt/lists` contains no package lists (fresh containers), mise
runs `apt-get update` automatically before installing. Otherwise it does not
touch apt metadata — if an install fails with "Unable to locate package",
refresh explicitly:

```sh
mise system install --update
```

## Version pins

A pinned entry (`curl=8.5.0-2ubuntu10`) shows as `version mismatch` in
`mise system status` when a different version is installed, and
`mise system install` passes the pin to apt to correct it. Unpinned entries
are satisfied by any installed version — mise does not upgrade them.
