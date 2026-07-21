# pkgx Backend <Badge type="warning" text="experimental" />

The `pkgx` backend installs packages from the [pkgx pantry](https://github.com/pkgxdev/pantry) without shelling out to the `pkgx` CLI. mise resolves pantry metadata, downloads pkgx bottles from `dist.pkgx.dev`, verifies bottle checksums when available, and writes wrapper scripts that set the package runtime environment.

This backend is experimental. Enable it with:

```sh
mise settings experimental=true
```

Or set `MISE_EXPERIMENTAL=1` for a single shell/session.

## Usage

Install a pkgx package by its pantry project name:

```sh
mise use pkgx:stedolan.github.io/jq@1.7.1
jq --version
```

The version will be set in `mise.toml` with the following format:

```toml
[tools]
"pkgx:stedolan.github.io/jq" = "1.7.1"
```

## Lockfiles

The pkgx backend supports [`mise.lock`](/dev-tools/mise-lock). Locking records the main bottle URL and checksum on the tool entry, and records transitive pkgx dependencies in the shared `[pkgx-packages]` lockfile section.

```sh
mise lock
mise install --locked
```

When `--locked` is enabled, mise requires a lockfile URL for the current platform and will fail instead of doing a live pantry resolution if the lockfile is missing or incomplete.

## Notes

- This backend currently supports platforms that pkgx publishes bottles for.
- Version requirements are resolved from pkgx pantry metadata using npm-style semver ranges.
- Runtime environment from pantry manifests is applied through generated wrappers.
