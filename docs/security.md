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
