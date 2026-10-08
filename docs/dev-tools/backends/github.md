---
description: "Install prebuilt tools from GitHub releases, with the asset options gitlab and forgejo share."
---

# github backend

The `github` backend installs a tool from the files attached to a GitHub
repository's releases. mise picks the asset for your OS and architecture,
verifies it, and extracts it. The [gitlab](/dev-tools/backends/gitlab.html) and
[forgejo](/dev-tools/backends/forgejo.html) backends share this implementation
and accept the options on this page.

If the tool has a [registry](/registry.html) shorthand, such as `ripgrep`, prefer
it: the registry entry already names a backend and the options the tool needs.
Use `github:owner/repo` for a tool without a shorthand, or to choose this
backend yourself.

## Usage

Install ripgrep in the current project, then run it:

```sh
mise use github:BurntSushi/ripgrep
mise exec -- rg --version
```

This writes the following to `mise.toml`. Add `-g` to `mise use` for a global
tool.

```toml
[tools]
"github:BurntSushi/ripgrep" = "latest"
```

List the available versions and pin one:

```sh
mise ls-remote github:BurntSushi/ripgrep
mise use github:BurntSushi/ripgrep@14.1.1
```

A version is the release tag without its leading `v`; see
[`version_prefix`](#version-prefix) for other tag formats.

To pass [tool options](#tool-options) on the command line, append them in
brackets. Quote the argument so the shell does not expand the brackets:

```sh
mise use 'github:oxc-project/oxc[matching=oxlint,rename_exe=oxlint]@apps_v1.69.0'
```

mise writes the options into the tool's entry in `mise.toml`. Public releases
need no token. For private repositories, GitHub Enterprise Server or rate-limit
errors, see [GitHub tokens](/dev-tools/github-tokens.html).

## How mise picks an asset {#asset-autodetection}

For each install, mise downloads one release asset:

1. If a [`url`](#platform-specific-urls) is set for the platform, mise downloads
   that URL and selects no asset.
2. If [`asset_pattern`](#asset-pattern) is set, mise downloads the asset whose
   name matches it.
3. Otherwise mise keeps the assets that pass [`matching`](#matching) and
   [`matching_regex`](#matching-regex), if set, and scores each one. The score
   rewards a match for your OS, CPU architecture and C library (glibc or musl on
   Linux, MSVC on Windows), archive formats it extracts well, and names
   that start with the repository name. It penalizes debug and test builds,
   checksum and signature files, and macOS `.app` bundles on other systems or
   with [`no_app`](#no-app). The highest score wins, and a tie goes to the
   shortest name.

Most tools install without any option:

```sh
mise install github:user/repo
```

If mise picks the wrong asset, narrow the candidates with
[`matching`](#matching) or name the asset with
[`asset_pattern`](#asset-pattern). When nothing matches, the error lists the
release's assets.

mise then verifies the download. It checks the digest GitHub reports for the
asset, or a checksum file published in the same release, and on public GitHub
it checks [GitHub artifact attestations](#github-attestations) when the release
has them. See [Verification](#verification).

## Tool options

Set these options on the tool's entry in `[tools]`, or inline as
`github:owner/repo[key=value]`. The `gitlab` and `forgejo` backends accept all
of them except the GitHub-only verification options; the
[gitlab](/dev-tools/backends/gitlab.html#differences-from-the-github-backend)
and [forgejo](/dev-tools/backends/forgejo.html#differences-from-the-github-backend)
pages list what behaves differently there. Options that every backend accepts,
such as `postinstall` and `os`, are described in
[tool options](/dev-tools/#tool-options).

| Option                                                                                       | Use it to                                                          |
| -------------------------------------------------------------------------------------------- | ------------------------------------------------------------------ |
| [`asset_pattern`](#asset-pattern)                                                            | Name the asset to download                                         |
| [`matching`](#matching), [`matching_regex`](#matching-regex)                                 | Narrow the assets that autodetection chooses from                  |
| [`no_app`](#no-app)                                                                          | Prefer a command-line archive over a macOS `.app` bundle           |
| [`additional_asset_patterns`](#additional-asset-patterns)                                    | Extract more archives from the same release into the install       |
| [`version_prefix`](#version-prefix)                                                          | Read versions from tags with a prefix other than `v`               |
| [`prerelease`](#prerelease)                                                                  | Include prereleases                                                |
| [`version_order`](#version-order)                                                            | Order versions by semantic version                                 |
| [`url`](#platform-specific-urls)                                                             | Download a URL instead of a release asset on one platform          |
| [`format`](#format)                                                                          | Set the archive format                                             |
| [`strip_components`](#strip-components)                                                      | Remove leading directories when extracting                         |
| [`bin_path`](#bin-path)                                                                      | Name the directory that holds the executables                      |
| [`bin`](#bin)                                                                                | Name a single-file download                                        |
| [`rename_exe`](#rename-exe)                                                                  | Rename executables extracted from an archive                       |
| [`filter_bins`](#filter-bins)                                                                | Put only some executables on `PATH`                                |
| [`checksum`](#checksum), [`size`](#size)                                                     | Pin the artifact's digest and size                                 |
| [`github_attestations`](#github-attestations)                                                | Skip GitHub artifact attestation checks for one tool (GitHub only) |
| [`slsa_signer_identity`, `slsa_signer_issuer`](#slsa-signer-identity-and-slsa-signer-issuer) | Verify SLSA provenance (GitHub only)                               |
| [`api_url`](#api-url)                                                                        | Use GitHub Enterprise Server or another forge host                 |

### Selecting assets

#### `asset_pattern` {#asset-pattern}

The name of the asset to download, with `*` matching any run of characters and
`?` matching one character. The pattern must match the whole name, and when it
matches several assets, mise picks the shortest name. It replaces
autodetection, so a pattern that names one platform, as below, works only on
that platform:

```toml
[tools]
"github:cli/cli" = { version = "latest", asset_pattern = "gh_*_linux_amd64.tar.gz" }
```

For a configuration that works everywhere, set the pattern
[per platform](#per-platform-options) or use [`matching`](#matching). The
pattern is a [template](#templates), so it can include the version, OS and
architecture.

#### `matching`

Keeps only assets whose names contain this text, then lets autodetection pick
your OS and architecture from what is left. Use it when a release ships several
tools for each platform, such as `oxlint-*` and `oxfmt-*` archives: one
configuration then works on every platform.

```toml
[tools]
"github:oxc-project/oxc" = { version = "apps_v1.69.0", matching = "oxlint", rename_exe = "oxlint" }
```

The test is a case-sensitive substring, so `matching = "tool"` also keeps
`tool-extras-*` assets. Use [`matching_regex`](#matching-regex) with an anchor
for an exact prefix. If no asset for your platform passes the filter, the
install fails with an error that names the filter. When `asset_pattern` is also
set, mise uses it and ignores `matching` and `matching_regex` without a warning.
SLSA provenance lookups use the same filter, so each binary of a multi-binary
release is verified against its own provenance file.

#### `matching_regex` {#matching-regex}

Like [`matching`](#matching), but the asset name must match a regular
expression. The match is case-sensitive; start the expression with `(?i)` to
ignore case. When both options are set, an asset must pass both. An invalid
expression fails the install.

```toml
[tools]
"github:oxc-project/oxc" = { version = "apps_v1.69.0", matching_regex = "^oxlint-", rename_exe = "oxlint" }
```

#### `no_app` {#no-app}

On macOS, prefers a standalone archive over a `.app` bundle, such as an Xcode
extension, during autodetection. mise already avoids `.app` bundles on other
systems. It has no effect when `asset_pattern` is set.

```toml
[tools."github:nicklockwood/SwiftFormat"]
version = "latest"
rename_exe = "swiftformat"
no_app = true # use swiftformat.zip, not SwiftFormat.for.Xcode.app.zip
```

#### `additional_asset_patterns` {#additional-asset-patterns}

Downloads more archives from the same release and extracts them into the primary
asset's install directory, in the order listed. Use it when a project splits one
installation across a base archive and add-on archives. For example, Ollama's
ROCm support for Linux on x64 is an archive laid over the normal Ollama archive:

```toml
[tools."github:ollama/ollama"]
version = "latest"

[tools."github:ollama/ollama".platforms]
linux-x64 = {
  additional_asset_patterns = ["ollama-linux-amd64-rocm.tar.zst"],
}
```

Each pattern uses the same wildcards and [templates](#templates) as
`asset_pattern`. If a pattern matches several assets, mise picks the shortest
name, so make each pattern specific to one archive. The value is an array or a
comma-separated string.

The extra assets must be archives. mise extracts them without the primary
asset's `strip_components`, `bin` and `rename_exe`, and a file in a later
archive replaces the same path from an earlier one. `mise lock` records the URL
and checksum of every extra archive, and `mise install --locked` fails when one
is missing from the lockfile.

### Choosing versions

#### `version_prefix` {#version-prefix}

The text in front of the version in release tags. Without it, mise removes a
leading `v` from each tag, and on GitHub it also removes a prefix that repeats
the repository name, as in the tag `tectonic@0.15.0`. Set it when a repository
uses another prefix:

```toml
[tools]
"github:user/repo" = { version = "latest", version_prefix = "release-" }
```

With `version_prefix = "release-"`, mise lists only tags that start with
`release-`, shows the tag `release-1.0.0` as `1.0.0`, and installs that tag for
`mise use github:user/repo@1.0.0`. Set `version_prefix = ""` to keep tags
exactly as published, including a leading `v`.

#### `prerelease`

By default, mise leaves releases that GitHub marks as prereleases out of
`mise ls-remote`, `latest` and version prefixes. Set `prerelease = true` to
include them:

```toml
[tools]
"github:myorg/mytool" = { version = "latest", prerelease = true }
```

`latest` then resolves to the newest release, prereleases included, instead of
the release GitHub marks as Latest, and a prefix such as `1.2` also matches
prerelease tags under it. Use it for a repository whose active releases are all
prereleases, or to follow release candidates. Draft releases are never listed.

To include prereleases for every tool, set the
[`prereleases`](/configuration/settings.html#prereleases) setting
(`MISE_PRERELEASES=1`). For a single listing, run `mise ls-remote --prerelease`.

#### `version_order` {#version-order}

By default, `mise ls-remote` lists releases in the order they were published.
Set `version_order = "semver"` when a repository publishes backports after
newer releases, so that the list and prefixes such as `1.2` follow semantic
version order:

```toml
[tools]
"github:owner/tool" = { version = "1.2", version_order = "semver" }
```

`latest` still resolves to the release GitHub or Forgejo marks as latest when
there is one. See [version ordering](/dev-tools/versions.html#version-ordering).

### Per-platform options

Set an option under `platforms.<os>-<arch>` to use it on that platform only. A
platform value overrides the top-level one. The key uses mise's names, `linux`,
`macos` or `windows` and `x64` or `arm64`; mise also accepts `darwin`, `amd64`,
`x86_64` and `aarch64`.

```toml
[tools."github:cli/cli"]
version = "2.100.0"

[tools."github:cli/cli".platforms]
linux-x64 = {
  asset_pattern = "gh_*_linux_amd64.tar.gz",
  checksum = "sha256:REPLACE_WITH_THE_64_HEX_DIGIT_DIGEST",
}
macos-arm64 = {
  asset_pattern = "gh_*_macOS_arm64.zip",
  checksum = "sha256:REPLACE_WITH_THE_64_HEX_DIGIT_DIGEST",
}
```

These options can be set per platform: `asset_pattern`,
`additional_asset_patterns`, `url`, `no_app`, `format`, `strip_components`,
`bin_path`, `bin`, `rename_exe`, `filter_bins`, `checksum` and `size`. The other
options apply to every platform.

#### `url` {#platform-specific-urls}

Set `platforms.<os>-<arch>.url` to download that URL instead of selecting a
release asset. The URL is a [template](#templates):

::: v-pre
`{{ version }}` is the resolved version, even when you request `latest`.
:::

```toml
[tools."github:owner/repo"]
version = "latest"
platforms.macos-arm64.url = "https://downloads.example.com/tool/{{ version }}/tool-macos-arm64.tar.gz"
```

Include any tag prefix, such as `v`, in the URL yourself. This backend ignores a
top-level `url`, so set it under each platform. mise extracts the download but
never builds it, so the URL must point to a prebuilt archive or binary. You can
lay release assets over it with
[`additional_asset_patterns`](#additional-asset-patterns). When every platform
you support sets `url`, mise lists releases even if they have no assets; see
[How versions are listed](#version-listing).

### Extracting and naming executables

#### `format`

The archive format, such as `tar.gz`, `tar.xz` or `zip`. Set it when an asset
name has no extension or a misleading one; otherwise mise detects the format
from the name. Use `raw` for a file that is not an archive.

```toml
[tools]
"github:owner/repo" = { version = "1.0.0", asset_pattern = "tool-linux-x64", format = "tar.gz" }
```

#### `strip_components` {#strip-components}

The number of leading directories to remove when extracting an archive:

```toml
[tools]
"github:cli/cli" = { version = "latest", strip_components = 1 }
```

When neither `strip_components` nor `bin_path` is set, mise removes one level by
itself if the archive holds a single top-level directory and no files, as
ripgrep's archives do (`ripgrep-14.1.1-x86_64-unknown-linux-musl/rg`).

#### `bin_path` {#bin-path}

The directory, relative to the install directory, that holds the executables.
Set it when mise does not find them. mise applies it after `strip_components`,
and setting it turns off the automatic stripping.

::: v-pre
For an archive laid out as `tool-1.0.0/bin/tool`, set `strip_components = 1`
and `bin_path = "bin"`, as below, or keep the outer directory with
`bin_path = "tool-{{ version }}/bin"`.
:::

```toml
[tools."github:cli/cli"]
version = "latest"
strip_components = 1
bin_path = "bin" # after the archive's outer directory is removed
```

`bin_path` is a [template](#templates), for when the directory name includes the
version, OS or architecture:

```toml
[tools."github:pizlonator/fil-c"]
version = "latest"
# such as filc-0.681-linux-x86_64/build/bin
strip_components = 0
bin_path = 'filc-{{ version }}-{{ os() }}-{{ arch(x64="x86_64", arm64="aarch64") }}/build/bin'
```

When `bin_path` is not set, mise puts these directories on `PATH`, using the
first rule that applies:

1. `bin/` in the install directory, if it exists.
2. `Contents/MacOS` in the install directory, for a `.app` bundle whose outer
   directory was stripped.
3. The install directory, if it directly contains an executable file.
4. Every immediate subdirectory that qualifies: a `*.app` directory contributes
   its `Contents/MacOS`, and any other subdirectory contributes its `bin/`, or
   itself if it directly contains an executable.
5. The install directory.

When [`filter_bins`](#filter-bins) is set, only its `.mise-bins` directory goes
on `PATH`.

#### Naming the executable

| Download                            | Option                      | Example                                                     |
| ----------------------------------- | --------------------------- | ----------------------------------------------------------- |
| A single binary file                | [`bin`](#bin)               | `bin = "mytool"`                                            |
| An archive with one executable      | [`rename_exe`](#rename-exe) | `rename_exe = "yt-dlp"`                                     |
| An archive with several executables | `rename_exe` as a table     | `rename_exe = { "ols-*" = "ols", "odinfmt-*" = "odinfmt" }` |

#### `bin`

The name to give a downloaded single-file binary. It can include a directory,
such as `bin/mytool`. mise already removes OS and architecture suffixes from
single-binary downloads, so `docker-compose-linux-x86_64` installs as
`docker-compose` with no option. Set `bin` only when you want another name:

```toml
[tools."github:owner/repo"]
version = "1.0.0"
bin = "mytool" # install the downloaded file as mytool
```

#### `rename_exe` {#rename-exe}

Renames an executable after an archive is extracted. The string form renames
the tool's main executable: the one named after the repository, else one whose
name contains the repository name, else the first executable mise finds.

```toml
[tools."github:yt-dlp/yt-dlp"]
version = "latest"
asset_pattern = "yt-dlp_linux.zip"
rename_exe = "yt-dlp"
```

When an archive ships several executables that you want under plain names, use
the table form. Each key is an exact file name or a glob, and each value is the
new name:

```toml
[tools."github:DanielGavin/ols"]
version = "latest"
# the archive holds ols-x86_64-unknown-linux-gnu and odinfmt-x86_64-unknown-linux-gnu
rename_exe = { "ols-*" = "ols", "odinfmt-*" = "odinfmt" }
```

mise warns about a key that matches nothing, and restores the executable bit
that some archives, such as ZIP files, drop.

#### `filter_bins` {#filter-bins}

Puts only the named executables on `PATH`. mise links them into a `.mise-bins`
directory inside the install and puts only that directory on `PATH`, so other
executables in the archive, such as `pandoc-lua` and `pandoc-server`, stay
hidden. The value is an array or a comma-separated string, and mise warns about
a name it cannot find.

```toml
[tools]
"github:jgm/pandoc" = { version = "latest", filter_bins = "pandoc" }
"github:owner/repo" = { version = "latest", filter_bins = ["tool", "helper"] }
```

### Verification

When GitHub reports a digest for the asset, or the release publishes a checksum
file that lists it, mise checks the download against it. When lockfiles are
enabled, mise records the checksum in [mise.lock](/dev-tools/mise-lock.html)
and checks later installs against it. On public GitHub mise also checks GitHub
artifact attestations. The options below pin values by hand, add SLSA
provenance checks, or turn the attestation check off.

#### `checksum`

Usually you do not need this: [`mise lock`](/dev-tools/mise-lock.html) records
the URL and checksum for every platform. Set `checksum` to pin one artifact by
hand, as `<algorithm>:<hash>` with an algorithm such as `sha256`, `sha512` or
`blake3`. A checksum describes one version and one asset, so pin the version,
and set it [per platform](#per-platform-options) when you support several:

```toml
[tools."github:owner/repo"]
version = "1.0.0"
asset_pattern = "tool-1.0.0-x64.tar.gz"
checksum = "sha256:REPLACE_WITH_THE_64_HEX_DIGIT_DIGEST"
```

#### `size`

The expected size of the asset in bytes. The install fails when the download has
a different size. On the `github`, `gitlab` and `forgejo` backends, mise checks
`size` only when `checksum` is also set, so set the two together. A size check
catches truncated downloads, but it does not replace a checksum.

```toml
[tools]
"github:owner/repo" = { version = "1.0.0", checksum = "sha256:REPLACE_WITH_THE_64_HEX_DIGIT_DIGEST", size = "12345678" }
```

#### `github_attestations` {#github-attestations}

By default, mise checks GitHub artifact attestations when a release asset has
them. Set `github_attestations = false` to skip that check for one tool while
keeping it on for the others:

```toml
[tools]
"github:myorg/mytool" = { version = "latest", github_attestations = false }
```

Use it as a temporary workaround when GitHub's attestation service or trusted
root data makes installs fail. Checksums, and SLSA provenance when it is
configured, are still checked. If `mise.lock` records `github-attestations`
provenance for the tool, run `mise lock` again after setting this option:
otherwise the install fails, because the lockfile requires a check the tool has
turned off. The [`github.github_attestations`](/configuration/settings.html#github.github_attestations)
setting turns the check off for every tool. This option is GitHub only.

#### `slsa_signer_identity` and `slsa_signer_issuer` {#slsa-signer-identity-and-slsa-signer-issuer}

To verify SLSA provenance published with a release, set the certificate
identity and the OIDC issuer that you expect for the release workflow:

```toml
[tools."github:myorg/mytool"]
version = "latest"
slsa_signer_identity = "https://github.com/myorg/mytool/.github/workflows/release.yml@refs/tags/v{{ version }}"
slsa_signer_issuer = "https://token.actions.githubusercontent.com"
```

::: v-pre
The identity must match the workflow ref in the certificate exactly. It is a
[template](#templates), so `{{ version }}` is the resolved version.
:::

Without both options, mise skips SLSA provenance and uses the other checks. A
lockfile entry that records a checksum and SLSA provenance installs without
verifying again, unless
[`locked_verify_provenance`](/configuration/settings.html#locked_verify_provenance)
is set; that setting also makes missing signer options an error. These options
are GitHub only.

### Templates

`asset_pattern`, `additional_asset_patterns`, `url`, `bin_path` and
`slsa_signer_identity` are [Tera templates](/templates.html). Write values in
double braces:

::: v-pre

- `{{ version }}`: the resolved version, without the tag prefix
- `{{ os() }}`: `linux`, `macos` or `windows`
- `{{ arch() }}`: `x64` or `arm64`

`os` and `arch` are functions, so include the parentheses. Each takes keyword
arguments that rename a value, for a project that uses other names:
`{{ os(macos="darwin") }}` or `{{ arch(x64="x86_64", arm64="aarch64") }}`. Use
a single-quoted TOML string when the template contains double quotes, as in the
`bin_path` example above.

:::

#### Single-brace placeholders <Badge type="danger" text="deprecated" />

::: v-pre
Single-brace placeholders such as `{version}` and `{x86_64_arch}` are
deprecated. mise has warned about them since 2026.3.0 and will stop accepting
them in 2027.3.0. Replace them as follows:

| Deprecated      | Replacement                                 |
| --------------- | ------------------------------------------- |
| `{version}`     | `{{ version }}`                             |
| `{os}`          | `{{ os() }}`                                |
| `{arch}`        | `{{ arch() }}`                              |
| `{darwin_os}`   | `{{ os(macos="darwin") }}`                  |
| `{amd64_arch}`  | `{{ arch(x64="amd64") }}`                   |
| `{x86_64_arch}` | `{{ arch(x64="x86_64", arm64="aarch64") }}` |
| `{gnu_arch}`    | `{{ arch(x64="x86_64") }}`                  |

:::

## Installing several tools from one release {#multiple-assets-from-the-same-release}

If the assets make up one installation, such as a base archive and an add-on,
use [`additional_asset_patterns`](#additional-asset-patterns). If they are
separate tools, give each one a [tool alias](/dev-tools/aliases.html) that
points at the same repository, and select its asset with `matching`:

```toml
[tool_alias]
oxlint = "github:oxc-project/oxc"
oxfmt = "github:oxc-project/oxc"

[tools.oxlint]
version = "apps_v1.69.0"
matching = "oxlint"
rename_exe = "oxlint"

[tools.oxfmt]
version = "apps_v1.69.0"
matching = "oxfmt"
rename_exe = "oxfmt"
```

Each alias is a separate tool with its own version and install directory. If one
binary's name is part of another's, use `matching_regex` with an anchor, such as
`^oxlint-`, instead of `matching`.

Two entries for the same `github:owner/repo` with different `matching` values do
not work: mise treats them as one tool, uses the first entry's options, and
never installs the other binary.

## GitHub Enterprise Server {#self-hosted-github}

Set `api_url` to your server's API, and provide a token for that host as
described in [GitHub tokens](/dev-tools/github-tokens.html#github-enterprise).
mise lists releases, looks up assets and downloads them through that API.
GitHub artifact attestations are not checked for a custom `api_url`, because
GitHub Enterprise Server does not serve them.

### `api_url` {#api-url}

The base URL of the API. It defaults to `https://api.github.com` here,
`https://gitlab.com/api/v4` on the `gitlab` backend and
`https://codeberg.org/api/v1` on the `forgejo` backend.

```toml
[tools]
"github:myorg/mytool" = { version = "latest", api_url = "https://github.mycompany.com/api/v3" }
```

## How versions are listed {#version-listing}

`mise ls-remote` lists the repository's releases that have at least one asset
attached. mise installs from release assets and never from GitHub's generated
source archives, so a release without assets has nothing to install. A release
whose assets do not cover your platform is still listed, so the version list is
the same on every machine and cross-platform lockfiles work. Draft releases are
never listed, and prereleases only with [`prerelease`](#prerelease).

If every platform you support sets [`url`](#platform-specific-urls), mise lists
releases whether or not they have assets. A `url` for only some platforms also
lists asset-less releases everywhere, and installing one of them on a platform
without a `url` fails.

mise reads one page of 100 releases. It reads more, up to three pages, only
while it has not found a stable release with assets. Set
`MISE_LIST_ALL_VERSIONS=1` to read every page. For public repositories, mise
reads release lists from [mise-versions](https://mise-versions.jdx.dev) before
it calls the GitHub API, unless
[`use_versions_host`](/configuration/settings.html#use_versions_host) is off.

## Settings

The other settings in the `github` group choose and create tokens; see
[GitHub, GitLab, and Forgejo tokens](/dev-tools/github-tokens.html).

<script setup>
import Settings from '/components/settings.vue';
</script>

<Settings child="github" :keys="['github_attestations', 'slsa']" :level="3" />

Implementation: [`src/backend/github.rs`](https://github.com/jdx/mise/blob/main/src/backend/github.rs).
