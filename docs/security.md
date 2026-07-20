# Security

mise includes several supply-chain controls for installing and managing tools. These controls have
different coverage depending on the backend and the metadata available from upstream.

## Software verification

mise provides native software verification for aqua tools without requiring external dependencies.
For aqua tools, Cosign/Minisign signatures, SLSA provenance, and GitHub artifact attestations are
verified automatically using mise's built-in implementation.

For other verification needs, such as GPG, you can install additional tools:

```sh
brew install gpg
```

To configure aqua verification, which is enabled by default:

```sh
# Disable specific verification methods if needed
export MISE_AQUA_COSIGN=false
export MISE_AQUA_SLSA=false
export MISE_AQUA_GITHUB_ATTESTATIONS=false
export MISE_AQUA_MINISIGN=false
```

For lockfile checksum and provenance behavior, see [mise.lock](/dev-tools/mise-lock.html).

## Safe mode

Safe mode (`MISE_SAFE=1`, or the [`safe`](/configuration/settings.html#safe) setting) is a hard
boundary against **project configuration executing code**. It is intended for running mise against
configuration you do not control — most notably automation that refreshes `mise.lock` on pull
request branches, such as a scheduled `mise lock --bump` job (see
[mise.lock](/dev-tools/mise-lock.html)).

```sh
# resolve tool versions from untrusted config without executing any of it
MISE_SAFE=1 mise lock --bump --dry-run --json
```

When enabled, mise **refuses with an error** (never a silent fallback) to:

- run `exec()` or `read_file()` in config templates
- source shell scripts via the `_.source` env directive
- run hooks (suppressed like `--no-hooks`, since hooks fire ambiently from `mise env`/`hook-env`)
- run tasks
- execute asdf plugin scripts
- install plugins

Version resolution still works for every HTTP-based backend — `core`, `aqua`, `github`, `gitlab`,
`http`, `cargo`, `pipx`, `gem`, `dotnet`, and `npm` — as well as `go` (which runs with
`GOTOOLCHAIN=local` so a project `go.mod` cannot trigger a toolchain download). Refreshing
`mise.lock` and listing installed tools work normally.

Already-installed and embedded vfox plugins also keep working: their code was chosen by the
operator, not by the repository being processed, and version resolution short-circuits on
plugins that are not installed without executing anything.

::: tip
Safe mode is a code-execution boundary; it does not replace the [trust](/cli/trust.html)
system. Untrusted configs still require `mise trust` (or a
[trusted config path](/configuration/settings.html#trusted_config_paths)). Safe mode limits what a
config can do; trust limits which configs are loaded.
:::

`MISE_SAFE` is `global_only`, so it can only be set via the environment or global config — a project
`mise.toml` cannot turn it off for itself.

## Minimum release age

To limit supply-chain risk, you can restrict mise to only install versions released before a
certain date or duration. This is similar to Renovate's
[minimum release age](https://docs.renovatebot.com/key-concepts/minimum-release-age/) concept:
newly published versions are ignored until they have been available for a configurable amount of
time.

```toml
# mise.toml
[settings]
minimum_release_age = "7d"  # only install versions released more than 7 days ago
```

Supports relative durations (`7d`, `6mo`, `1y`) and absolute dates (`2024-06-01`). For most
backends, this only affects fuzzy version resolution, such as `node@20` or `latest`.
Explicitly pinned versions like `node@22.5.0` bypass the filter.
During ordinary toolset resolution, already-installed fuzzy matches remain eligible:
`minimum_release_age` limits remote version selection and does not make an installed version
inactive. Lockfile generation may re-check fuzzy installed matches against release metadata.

Capability depends on the backend:

| Capability                                     | Backends                                                                                                                               |
| ---------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| Top-level version filtering                    | Backends that provide release timestamps, such as `aqua:`, `cargo:`, `github:`, `gitlab:`, `go:`, `npm:`, `pipx:`, and many core tools |
| Transitive dependency filtering during install | `npm:` and `pipx:`                                                                                                                     |

Versions without timestamps are included by default. Backends without transitive dependency support
may still select an older top-level tool version, but they do not constrain dependencies fetched by
the tool's installer/compiler.

For `npm:` and `pipx:` transitive dependency support details, refer to the
[npm backend docs](/dev-tools/backends/npm.html) and
[pipx backend docs](/dev-tools/backends/pipx.html).

You can also set `minimum_release_age` per-tool to override the global setting:

```toml
# mise.toml
[settings]
minimum_release_age = "7d"  # default for all tools

[tools.trivy]
version = "latest"
minimum_release_age = "1d"  # trivy updates are time-sensitive, use a shorter window
```

Precedence: `--minimum-release-age` CLI flag > per-tool `minimum_release_age` > global
`minimum_release_age` setting.

Use `minimum_release_age_excludes` to exclude tools or backends from the global/default setting:

```toml
[settings]
minimum_release_age = "7d"
minimum_release_age_excludes = ["trivy", "npm:*"]
```

Exclusions can match backend wildcards like `npm:*`, tool shorthands like `trivy`, or full backend
IDs like `npm:prettier`. Matching tools skip the global setting and built-in default. Per-tool
`minimum_release_age` options and the CLI flag still apply even when a tool matches the exclusion
list.

See [`minimum_release_age`](/configuration/settings.html#minimum_release_age) for the setting
reference.
