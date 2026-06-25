# apt <Badge type="warning" text="experimental" />

System packages for Debian-family Linux (Debian, Ubuntu, Mint, ...).

```toml
[bootstrap.packages]
"apt:libssl-dev" = "latest"
"apt:curl" = "8.5.0-2ubuntu10" # version pin
"apt:gcc:arm64" = "latest"     # architecture qualifier
```

## Behavior

- Package state is checked with `dpkg-query` (read-only, never elevates).
- Missing packages are installed with `apt-get install -y`, elevated with
  sudo when necessary (see [sudo](/bootstrap/packages/#sudo)).
- Version pins are passed to apt as its native `name=version` syntax;
  `name:arch` qualifiers pass through in the package name.
- `DEBIAN_FRONTEND=noninteractive` is set so installs never block on
  configuration prompts.
- `mise bootstrap packages upgrade` runs `apt-get update` and then
  `apt-get install --only-upgrade` for the configured packages, so nothing
  not already installed gets pulled in.

## Metadata refresh

If `/var/lib/apt/lists` contains no package lists (fresh containers), mise
runs `apt-get update` automatically before installing. Otherwise it does not
touch apt metadata — if an install fails with "Unable to locate package",
refresh explicitly:

```sh
mise bootstrap packages apply --update
```

## Version pins

A pinned entry (`"apt:curl" = "8.5.0-2ubuntu10"`) shows as `version mismatch`
in `mise bootstrap packages status` when a different version is installed, and
`mise bootstrap packages apply` passes the pin to apt to correct it. `"latest"` entries
are satisfied by any installed version — use `mise bootstrap packages upgrade` to move
them to the newest available version.
