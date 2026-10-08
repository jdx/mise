---
description: "Add a shorthand for a widely used tool to the mise registry: the popularity bar, accepted backends, the entry format, and the test."
outline: [2, 3]
---

# Adding tools to the registry

A registry entry gives a widely used tool a short name, so `mise use ripgrep`
works without a backend identifier. Most proposals for new entries are
declined: check the [popularity bar](#popularity-bar) and the
[backend tiers](#backend-acceptance-tiers) before you write one.

You do not need an entry to use a tool. `mise use github:owner/repo`,
`mise use aqua:owner/repo` and `mise use cargo:name` install a tool without
one, and a [tool plugin](/tool-plugin-development.html) covers a tool that no
backend can install. The registry only adds the shorthand.

These rules apply to new tools and new shorthands. A fix to an existing entry
needs no popularity evidence.

## Quick start {#quick-start}

1. Check the tool against the [popularity bar](#popularity-bar).
2. Pick a backend from the [acceptance tiers](#backend-acceptance-tiers):
   `packslip` if the project publishes signed release manifests, otherwise
   `aqua` if the tool is in the aqua registry, otherwise `github` or `gitlab`.
3. Check that the backend lists installable versions:

   ```sh
   mise ls-remote aqua:owner/repo
   ```

   A backend that can install only a version you name explicitly is not
   enough.

4. Create `registry/<name>.toml` in the [entry format](#registry-format), with a
   [test](#tool-testing).
5. Build mise and test the entry with that build. The registry is compiled into
   the binary, so an installed mise does not see your file:

   ```sh
   mise run build
   target/debug/mise test-tool your-tool
   ```

6. Open a pull request titled `registry: add your-tool (aqua:owner/repo)`, with
   a `## Popularity` section in the description.

## The popularity bar {#popularity-bar}

<span id="guidelines-and-requirements"></span>

The registry is curated for tools that are already widely used. The bar is
normally thousands of GitHub stars, active maintenance, and real use outside
the author's own projects. Personal, internal, niche and low-popularity tools,
and forks, do not meet it. A working installer or a passing test is not
enough. @jdx won't explain why a given tool wasn't accepted.

Look up current numbers before you propose an entry, and put them in the pull
request description:

```md
## Popularity

- GitHub: 12.3k stars, 480 forks, last release 2026-04-12
- crates.io: 1.2M downloads
- Used by: <project A>, <project B>
```

Useful evidence includes stars and forks, recent releases, downloads from the
tool's package registry, and documentation or tools from other projects that
use it.

## Backend acceptance tiers {#backend-acceptance-tiers}

The backend you choose for an entry matters as much as the tool. These tiers
apply to registry entries only; you can use any backend in your own config.

| Tier                  | Backends                                      | Accepted when                                                                       |
| --------------------- | --------------------------------------------- | ----------------------------------------------------------------------------------- |
| 1, preferred          | `packslip`                                    | The project publishes signed release manifests                                      |
| 2, routinely accepted | `aqua`, `github`, `gitlab`                    | The project publishes no packslips                                                  |
| 3, high bar           | `conda`                                       | No tier 1 or 2 backend can reasonably install the tool                              |
| 4, very high bar      | `npm`, `pypi`, `gem`, `cargo`, `go`, `dotnet` | No tier 1 or 2 backend can install it, it is widely used, and jdx agreed beforehand |
| Not accepted          | `asdf`, `vfox`, `ubi`                         | Never, for new entries                                                              |

[`packslip`](/dev-tools/backends/packslip.html) verifies the signer and the
artifact digests without a plugin or a separate package manager.

When a project publishes no packslips, use [`aqua`](/dev-tools/backends/aqua.html)
if the tool is in the [aqua registry](https://github.com/aquaproj/aqua-registry):
its recipes carry per-version logic and SLSA and other verification metadata.
Use [`github`](/dev-tools/backends/github.html) for a tool that is not in the
aqua registry but ships GitHub releases, and
[`gitlab`](/dev-tools/backends/gitlab.html) for one released through GitLab.

[`conda`](/dev-tools/backends/conda.html) has a lower bar than tier 4 because it
needs no separately installed package manager: mise downloads and extracts
packages from anaconda.org itself, without `conda`, `mamba` or `micromamba`.
The tool still has to be popular and well maintained.

Tier 4 backends depend on a language runtime or toolchain on the user's machine.
[npm](/dev-tools/backends/npm.html) tools need Node.js to run,
[gems](/dev-tools/backends/gem.html) depend on the Ruby that installed them,
[pypi](/dev-tools/backends/pypi.html) needs uv or pipx,
[cargo](/dev-tools/backends/cargo.html) and [go](/dev-tools/backends/go.html)
build tools with their toolchains, and [dotnet](/dev-tools/backends/dotnet.html)
needs a .NET SDK. Go and Rust tools usually ship single binaries, so prefer a
tier 1 or 2 backend for them. Get explicit agreement from @jdx before you
submit an entry that uses a tier 4 backend. Older entries write `pipx:`, which
mise reads as `pypi:`.

New `asdf` and `vfox` entries are rejected for supply-chain security reasons.
The [`ubi`](/dev-tools/backends/ubi.html) backend is deprecated: mise warns
when it uses it, and mise 2027.1.0 removes it.

[`http`](/dev-tools/backends/http.html) is not in a tier. It is accepted for a
widely used tool that publishes stable download URLs but no packslips and no
GitHub or GitLab releases, as long as a
[`version_list_url`](/dev-tools/backends/http.html#version-list-url) lets
`mise ls-remote` list its versions. The tiers do not cover `forgejo`, `s3`,
`spm` or `spinel`; ask in a [Discussion](https://github.com/jdx/mise/discussions)
before you submit an entry that uses one. `core:` entries point at
[core tools](/core-tools.html), which are built into mise and not contributed
through the registry.

## Entry format {#registry-format}

Each tool is one file, `registry/<name>.toml`, and the file name is the
shorthand. Keys are in alphabetical order because taplo sorts them:

```toml
aliases = ["your-tool-cli"]
backends = [
  "packslip:github.com/owner/repo",
  "aqua:owner/repo",
  "github:owner/repo",
]
bins = ["your-tool"]
description = "One line about the tool"
os = ["linux", "macos"]
test = { cmd = "your-tool --version", expected = "{{version}}" }
url = "https://your-tool.dev"
version_order = "semver"
```

| Key               | Required        | Meaning                                                                                              |
| ----------------- | --------------- | ---------------------------------------------------------------------------------------------------- |
| `backends`        | yes             | Backends in order of preference; see [backend entries](#backend-entries)                             |
| `version_order`   | yes             | `semver` or `source`; see [version order](#version-order)                                            |
| `description`     | expected        | One line, shown by `mise search` and on the [registry page](/registry.html)                          |
| `test`            | for new entries | The installation check; see [tool tests](#tool-testing)                                              |
| `bins`            | often           | Executable names, used to create shims before the tool is installed; see [executables](#executables) |
| `aliases`         | no              | Other names that resolve to this entry                                                               |
| `os`              | no              | Operating systems the tool supports (`linux`, `macos`, `windows`); mise skips the tool elsewhere     |
| `url`             | no              | The project's homepage, when the link inferred from the backends is missing or wrong                 |
| `deprecated`      | no              | Why the tool should not be used and what to use instead; mise warns when it installs the tool        |
| `idiomatic_files` | no              | Version files the tool reads; see [idiomatic version files](#idiomatic-version-files)                |
| `detect`          | no              | Files that make the interactive `mise edit` add the tool when they exist in the current directory    |
| `overrides`       | no              | Tools whose executables this one shadows on `PATH`, as `npm` shadows the npm bundled with `node`     |

`mise run lint` checks the file against
[`schema/mise-registry-tool.json`](https://github.com/jdx/mise/blob/main/schema/mise-registry-tool.json),
and the build fails on an entry it cannot read.

### Backend entries {#backend-entries}

List only backends that support the tool: `packslip` needs signed release
manifests, and `aqua` needs an entry in the aqua registry. mise uses the first
listed backend that is enabled, runs on the platform, and covers the requested
version; users can still pick another with an explicit identifier such as
`aqua:owner/repo`. See
[how backend selection works](/dev-tools/backends/#how-backend-selection-works).

<span id="backend-priority"></span>

A backend can be a table instead of a string. `full` is the identifier, and the
other keys narrow or configure it:

- `platforms`: where this backend applies. Use an OS (`linux`, `macos`,
  `windows`), an architecture (`x64`, `arm64`), or both (`linux-x64`,
  `macos-arm64`). Windows on arm64 also matches `x64` and `windows-x64`.
- `options`: backend [tool options](/dev-tools/#tool-options), such as
  `allow_builds` for npm or `version_list_url` and per-platform `url` for http.
  `mise tool <name>` lists the options a shorthand applies.
- `min_version` and `max_version`: the version range the backend serves; see
  [minimum backend versions](#minimum-backend-versions).
- `attestations_since`: the first version whose GitHub release assets all carry
  attestations; see [required attestations](#required-attestations).

Long tables read better as `[[backends]]` sections. This is
`registry/twg.toml`:

```toml
description = "Command line interface to the Atlassian Teamwork Graph and Cloud services"
os = ["linux", "macos", "windows"]
test = { cmd = "twg --version", expected = "{{version}}" }
version_order = "source"

bins = ["twg"]
[[backends]]
full = "http:twg"

[backends.options]
bin = "twg"
checksum_url = "https://teamwork-graph.atlassian.com/cli/SHA256SUMS-v{{ version }}"
url = 'https://teamwork-graph.atlassian.com/cli/twg-{{ os(macos="darwin") }}-{{ arch() }}-v{{ version }}'
# Per-version manifest published alongside the binaries; .version tracks the latest stable release
version_json_path = ".version"
version_list_url = "https://teamwork-graph.atlassian.com/cli/manifest.json"

[backends.options.platforms.windows-arm64]
bin = "twg.exe"
url = "https://teamwork-graph.atlassian.com/cli/twg-windows-arm64-v{{ version }}.exe"

[backends.options.platforms.windows-x64]
bin = "twg.exe"
url = "https://teamwork-graph.atlassian.com/cli/twg-windows-x64-v{{ version }}.exe"
```

Add `npm` as a fallback after a non-npm backend only if the package installs
with dependency build scripts off, or list the packages whose scripts it needs
in `allow_builds`, as `registry/nub.toml` does. See
[build scripts](/dev-tools/backends/npm.html#build-scripts-and-supply-chain-checks).

### Version order {#version-order}

Every entry sets `version_order`. Use `semver` only when the tool's stable
releases consistently use strict `MAJOR.MINOR.PATCH` versions. Use `source`
for date versions, two-part versions, channels, refs, tool-specific formats,
mixed histories, or whenever you are unsure. The `aqua`, `github`, `gitlab`,
`forgejo` and `http` backends order versions by it; other backends order
versions themselves, and the field still records the policy for the tool.

### Executables {#executables}

Set `bins` to the tool's executable names so mise can create shims for
[lazy tools](/dev-tools/shims.html#lazy-tools) before it downloads the tool.
When `packslip` or another non-aqua backend comes first, mise cannot infer the
names, so list them.

When `aqua` comes first, the build takes the command names from the aqua
registry's file metadata. Leave `bins` out when that list is right. Set it when
the shorthand needs a different set, such as commands that only a fallback
backend provides.

### Project URL {#project-url}

The registry page links each tool name to a project URL inferred from the first
backend that has one: the repository for `aqua`, `github` and similar backends,
or the package page for `npm`, `cargo` and other package registries. `http` and
`packslip` have no inferable URL. Set `url` when no backend gives a link or the
link is wrong, such as for a tool published from a monorepo. It must be a
homepage or repository, not a download URL. `mise tool` and
`mise registry --json` show it too.

### Deprecated tools {#deprecated-tools}

Set `deprecated` to the reason a tool should no longer be used and what to use
instead, such as when upstream replaced its CLI. The entry keeps working, and
mise shows the message as a warning whenever it installs the tool.

## Minimum backend versions {#minimum-backend-versions}

When a backend supports only newer releases, set `min_version` on it. hk
publishes packslip manifests starting at 1.58.1:

```toml
backends = [
  { full = "packslip:github.com/jdx/hk", min_version = "1.58.1" },
  "aqua:jdx/hk",
]
bins = ["hk"]
version_order = "semver"
```

The minimum is inclusive and must be a complete semantic version, and the entry
must use `version_order = "semver"`. `mise use hk@1.57` and `mise use hk@1.58.0`
select aqua, while `mise use hk@1.58.1` selects packslip. A prefix that spans
the boundary, such as `1.58`, keeps the preferred backend. `latest`, channels,
and unresolved aliases keep the normal backend order; aliases are checked again
after they resolve.

Selection still respects platform support and disabled backends. An explicit
backend identifier, a backend override, and the backend recorded in a matching
lockfile entry take precedence. A failed download or signature check does not
fall back to the next backend. A backend without `min_version` has no lower
bound.

### Maximum backend versions {#maximum-backend-versions}

When a backend serves only older releases, such as a frozen 1.x line published
separately from later majors, set `max_version` on it:

```toml
backends = [
  { full = "aqua:example/tool-next", min_version = "2.0.0" },
  { full = "aqua:example/tool-legacy", max_version = "2.0.0" },
]
bins = ["tool"]
version_order = "semver"
```

The maximum is exclusive and follows the same rules as `min_version`. A backend
can set both, as long as `min_version` is lower. Here `tool@1` and `tool@1.9.9`
select `tool-legacy`, while `tool@2` and `tool@2.0.0` select `tool-next`. A
prefix entirely at or above the boundary, such as `2`, skips the backend; one
that spans it keeps the preferred backend. Prereleases of the boundary version
sort below it, so an exact request for `tool@2.0.0-rc.1` selects `tool-legacy`,
while the prefix `2` still selects `tool-next`.

A locked backend stays in use only for the versions it serves. If the lockfile
records `tool-legacy` and the config moves to `tool@2`, mise selects `tool-next`
instead of asking the legacy backend for a release it does not publish.

## Required attestations {#required-attestations}

When a project publishes
[GitHub artifact attestations](https://docs.github.com/actions/security-for-github-actions/using-artifact-attestations)
for its release assets, set `attestations_since` on its `github:` backend to the
first version whose assets all carry them:

```toml
backends = [
  { full = "github:aubepkg/aube", attestations_since = "2.2.5" },
]
version_order = "semver"
```

From that version on, mise refuses to install or lock an asset, including
`additional_asset_patterns` assets, unless a GitHub attestation for it
verifies. Finding none fails as a possible downgrade instead of skipping
verification, also when the shared mise-versions cache or a lockfile reported
the missing attestation. Other provenance, such as SLSA, does not count.
Earlier versions, versions that are not semantic versions (`nightly`, `1.0`,
`2024.01.15`), and users who turn off
[`github_attestations`](/configuration/settings.html#github_attestations) or
[`github.github_attestations`](/configuration/settings.html#github.github_attestations)
are unaffected.

The value must be a complete semantic version. The tool's `version_order` can be
anything: mise compares only the version being installed with the boundary.

Either kind of GitHub attestation counts, as long as it names the tool's
repository:

- A build provenance attestation from the project's workflow
  (`actions/attest-build-provenance`). Check it with
  `gh attestation verify <file> --repo owner/repo`.
- The release attestation that GitHub creates for every asset of an immutable
  release. Check it with `gh release verify-asset <tag> <file> --repo owner/repo`.

Check every asset of the boundary release and of the release before it. The
boundary is the first release where every asset passes, and every later
semantic version must pass too. Watch for projects that publish backports out
of order: if a patch to an older line was the first attested release, a newer
line released before it would wrongly be required to have attestations.

## Idiomatic version files {#idiomatic-version-files}

An entry can let users read the tool's version from the
[idiomatic version files](/dev-tools/versions.html#idiomatic-version-files) the
tool already uses. A file name uses mise's plain-text parser:

```toml
backends = ["aqua:owner/repo"]
idiomatic_files = [".your-tool-version"]
```

For a structured or tool-specific file, use a table with the parser fields that
the [http backend's version listing](/dev-tools/backends/http.html#version-list-url)
supports:

```toml
idiomatic_files = [
  { path = "your-tool.json", version_json_path = ".toolchain.version" },
  { path = "your-tool.conf", version_regex = 'version\s*=\s*"([^"]+)"' },
]
```

- `version_regex`: extracts every match, using the first capture group when
  there is one.
- `version_json_path`: extracts values with mise's jq-like JSON path syntax.
- `version_expr`: extracts or post-processes versions with an
  [expr-lang](https://expr-lang.org/) expression. The file's contents are in
  `body`, and versions found by `version_regex` or `version_json_path` are in
  `versions`.

mise runs these parsers itself; they cannot run shell commands.

Extract only a value that states the version the project is built with, such as
an exact version, or a config-format major that is deliberately tied to the CLI
major. Do not extract a minimum compatible version: a floor such as
`cmake_minimum_required` or `package.json`'s `engines` says what a consumer
needs, and resolving it pins users to the oldest supported release (see
[which fields mise reads](/dev-tools/versions.html#which-fields-mise-reads)).
Do not extract unrelated project versions, dependency versions, lockfile schema
revisions, or generic `version` fields that do not constrain the tool.

To retire an existing file that reads a floor, add `deprecated = "<reason>"` to
its table. It keeps resolving, and mise warns users to move the version into
`mise.toml`.

List every file name the tool officially reads, including documented nested
paths such as `.config/tool.yml`. When suffixes overlap, mise uses the most
specific matching path. Users still have to enable idiomatic files for the tool:

```sh
mise settings add idiomatic_version_file_enable_tools your-tool
```

## aqua registry recipes {#aqua-registry-recipes}

An `aqua:` backend installs from a recipe in the
[aqua registry](https://github.com/aquaproj/aqua-registry), not from anything
in mise's `registry/`. When a recipe is wrong, fix it upstream. Each mise
release bundles the aqua registry's latest `main`, so an upstream fix reaches
users with the next mise release, or sooner with
[`registry_floating`](/configuration/settings.html#registry_floating). To try a
recipe change before it merges, point
[`aqua.registries`](/dev-tools/backends/aqua.html#custom-registry) at a local
`registry.yaml`.

Two recipe problems come up often:

- A platform is missing from `supported_envs`. Compare the recipe's
  `supported_envs` with the publisher's release assets. If a matching asset
  exists, add the platform to the recipe. Adding a platform name cannot make an
  incompatible binary run.
- Versions show a tag prefix, such as `atlascli/1.2.3`. That happens when the
  recipe selects tags with a `version_filter` expression like
  `Version startsWith "atlascli/"`. Put the prefix in `version_prefix` instead:
  mise strips it from the versions it shows and adds it back when it needs the
  tag. Use `version_filter` only to exclude unrelated releases. Versions do not
  need three parts.

## Tool tests {#tool-testing}

Every new entry needs a `test`. The registry workflow's
[`validate-new-tools` job](https://github.com/jdx/mise/blob/main/.github/workflows/registry-impl.yml)
fails a pull request that adds a tool without one.

```toml
test = { cmd = "your-tool --version", expected = "your-tool {{version}}" }
```

[`mise test-tool`](/cli/test-tool.html) installs the latest version of the tool
with the [minimum release age](/configuration/settings.html#minimum_release_age)
turned off. It finds the first word of `cmd` among the tool's executables and
runs the command with `sh -c` (`cmd /C` on Windows), with standard error merged
into standard output. The test passes when the command exits 0 and its output
contains `expected`.

`expected` is a plain substring, not a pattern. mise renders it as a template
first, and <code v-pre>{{version}}</code> becomes the version it installed, not
a wildcard. Use <code v-pre>{{version}}</code> when the command prints that
version; otherwise pick another stable part of the output.

If `cmd` needs other mise-managed tools on `PATH`, list them in `tools`. Only
`mise test-tool` uses them; they do not change how the tool installs.

```toml
test = { cmd = "gradle -V", expected = "Gradle", tools = ["java"] }
```

Test with your build, because the registry is compiled in. `mise test-tool`
first deletes the tool's installs, cache and downloads, so point it at
throwaway directories if you have versions of the tool you want to keep:

```sh
mise run build
MISE_DATA_DIR=/tmp/mise-test/data MISE_CACHE_DIR=/tmp/mise-test/cache \
  target/debug/mise test-tool your-tool
```

Pass several names to test several tools, and `--raw` to see the installer's
output directly. `--all-config` tests the registry tools in your config files.
`--all` tests the whole registry and takes hours. CI runs `mise test-tool` on
Linux for every entry a pull request adds or changes, with a build of that
pull request.
