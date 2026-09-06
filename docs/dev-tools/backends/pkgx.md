# pkgx Backend <Badge type="warning" text="experimental" />

The `pkgx` backend installs packages from the [pkgx pantry](https://github.com/pkgxdev/pantry) without shelling out to the `pkgx` CLI. mise resolves pantry metadata, downloads pkgx bottles from `dist.pkgx.dev`, verifies bottle checksums when available, and writes wrapper scripts that set the package runtime environment.

This backend is experimental. Enable it for the project in `mise.toml`:

```toml
[settings]
experimental = true
```

Or prefix an individual command with `MISE_EXPERIMENTAL=1`.

## Usage

Install a pkgx package by its pantry project name. This is often a domain and
path rather than the executable name:

```sh
mise use pkgx:stedolan.github.io/jq@1.7.1
mise exec -- jq --version
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

With `--locked`, mise requires a lockfile URL for the current platform and fails instead of performing a live pantry resolution if the lockfile is missing or incomplete.

## Notes

- This backend currently supports platforms that pkgx publishes bottles for.
- Version requirements are resolved from pkgx pantry metadata using npm-style semver ranges.
- The runtime environment from pantry manifests is applied through generated wrappers.

## Troubleshooting

Use `mise ls-remote pkgx:stedolan.github.io/jq` to check the package identifier and
available versions. A version still needs bottles for your platform and its
transitive dependencies. Use `mise exec -- jq --version` to verify the generated
launcher and runtime environment.

Do not bypass the launcher by copying its underlying binary elsewhere: the
wrapper supplies the pantry-defined library paths and environment. For locked
installs, keep the shared `pkgx-packages` entries as well as the top-level tool
entry.
