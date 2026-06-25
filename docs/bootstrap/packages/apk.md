# apk <Badge type="warning" text="experimental" />

System packages for Alpine Linux.

```toml
[bootstrap.packages]
"apk:build-base" = "latest"
"apk:zlib-dev" = "1.3.1-r2" # version pin
```

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

A pinned entry (`"apk:zlib-dev" = "1.3.1-r2"`) shows as `version mismatch`
in `mise bootstrap packages status` when a different version is installed,
and `mise bootstrap packages apply` passes the pin to apk to correct it.
`"latest"` entries are satisfied by any installed version — use
`mise bootstrap packages upgrade` to move them to the newest available
version.
