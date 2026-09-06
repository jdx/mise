# Alpine packages (apk)

System packages for Alpine Linux.

```toml
[bootstrap.packages]
"apk:build-base" = "latest"
"apk:zlib-dev" = "1.3.1-r2" # version pin
```

## Preview and apply

```sh
mise bootstrap packages status
mise bootstrap packages apply --manager apk --dry-run
mise bootstrap packages apply --manager apk
```

These commands use the active `[bootstrap.packages]` declarations. To add and
install a package together, use `mise bootstrap packages use apk:build-base`.
The manager must be available on the host; an explicit `--manager apk` fails
when it is unavailable.

## Behavior

- Package state is checked with `apk info -e -v` (read-only, never elevates).
- Missing packages are installed with `apk add`, elevated with sudo when
  necessary (see [sudo](/bootstrap/packages/#sudo)).
- Version pins are passed to apk as its native `name=version` syntax.
- `mise bootstrap packages apply --update` adds `--update-cache` to refresh
  apk metadata.
- `mise bootstrap packages upgrade` runs `apk upgrade --available --update-cache`
  for the configured packages that are already installed.

## Version pins

The pinned version above is illustrative. Check `apk policy zlib-dev` on the
target and select a version available from its configured repositories. A pin
does not add an old Alpine repository or retrieve archived packages.

A pinned entry (`"apk:zlib-dev" = "1.3.1-r2"`) shows as `version mismatch`
in `mise bootstrap packages status` when a different version is installed,
and `mise bootstrap packages apply` passes the pin to apk to correct it.
`"latest"` entries are satisfied by any installed version — use
`mise bootstrap packages upgrade` to move them to the newest available
version.
