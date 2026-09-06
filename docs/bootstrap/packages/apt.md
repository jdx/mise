# Debian and Ubuntu packages (apt)

System packages for Debian-family Linux (Debian, Ubuntu, Mint, ...).

```toml
[bootstrap.packages]
"apt:libssl-dev" = "latest"
"apt:curl" = "8.5.0-2ubuntu10" # version pin
"apt:gcc:arm64" = "latest"     # architecture qualifier
```

## Preview and apply

```sh
mise bootstrap packages status
mise bootstrap packages apply --manager apt --dry-run
mise bootstrap packages apply --manager apt
```

These commands use the active `[bootstrap.packages]` declarations. To add and
install a package together, use `mise bootstrap packages use apt:libssl-dev`.
The manager must be available on the host; an explicit `--manager apt` fails
when it is unavailable.

## Behavior

- Package state is checked with `dpkg-query` (read-only, never elevates).
- Missing packages are installed with `apt-get install -y`, elevated with
  sudo when necessary (see [sudo](/bootstrap/packages/#sudo)).
- Version pins are passed to apt as its native `name=version` syntax;
  `name:arch` qualifiers pass through in the package name.
- `DEBIAN_FRONTEND=noninteractive` is set so installs never block on
  debconf configuration prompts. This does not supply sudo credentials or
  guarantee that every maintainer script is non-interactive.
- `mise bootstrap packages upgrade` runs `apt-get update` and then
  `apt-get install --only-upgrade` for the configured packages, so nothing
  requested package that is not already installed becomes an install target.
  apt still resolves dependencies required by those upgrades.

## Metadata refresh

If `/var/lib/apt/lists` contains no package lists (fresh containers), mise
runs `apt-get update` automatically before installing. Otherwise, it does not
touch apt metadata — if an install fails with "Unable to locate package",
refresh explicitly:

```sh
mise bootstrap packages apply --update
```

## Architecture-qualified packages

`gcc:arm64` is a package name with an architecture qualifier. The target's dpkg
architecture configuration and apt repositories must supply that architecture;
this declaration does not enable multiarch or add a repository.

## Version pins

The version above is illustrative and specific to a distribution release. Use
`apt-cache policy curl` to inspect candidates on the target before pinning it.
An exact pin must remain available in the configured repositories; mise does
not turn apt into a historical package archive.

A pinned entry (`"apt:curl" = "8.5.0-2ubuntu10"`) shows as `version mismatch`
in `mise bootstrap packages status` when a different version is installed, and
`mise bootstrap packages apply` passes the pin to apt to correct it. `"latest"` entries
are satisfied by any installed version — use `mise bootstrap packages upgrade` to move
them to the newest available version.
