1
### `matching_regex`

Like [`matching`](#matching), but the asset name must match the given regular expression. Use this when a substring isn't selective enough. The match is case-sensitive; use an inline `(?i)` flag for case-insensitive matching.

```toml
[tools]
"github:oxc-project/oxc" = { version = "apps_v1.69.0", matching_regex = "^oxlint-", rename_exe = "oxlint" }
```

If both `matching` and `matching_regex` are set, an asset must satisfy **both** (logical AND)
to remain a candidate.

### `version_prefix`

Specifies a custom version prefix for release tags. By default, mise handles the common `v` prefix (e.g., `v1.0.0`), but some repositories use different prefixes like `release-`, `version-`, or no prefix at all.

When `version_prefix` is configured, mise will:

- Filter available versions with the prefix and strip it
- Add the prefix when searching for releases
- Try both prefixed and non-prefixed versions during installation

```toml
[tools]
"github:user/repo" = { version = "latest", version_prefix = "release-" }
```

**Examples:**

- With `version_prefix = "release-"`:
  - User specifies `1.0.0` → mise searches for `release-1.0.0` tag
  - Available versions show as `1.0.0` (prefix stripped)
- With `version_prefix = ""` (empty string):
  - User specifies `1.0.0` → mise searches for `1.0.0` tag (no prefix)
  - Useful for repositories that don't use any prefix

### Platform-specific Asset Patterns

For different asset patterns per platform:

```toml
[tools."github:cli/cli"]
version = "latest"

[tools."github:cli/cli".platforms]
linux-x64 = { asset_pattern = "gh_*_linux_x64.tar.gz" }
macos-arm64 = { asset_pattern = "gh_*_macOS_arm64.tar.gz" }
```

### Multiple Assets from the Same Release

There are two distinct cases:

- If the assets are parts of one installation, use
  [`additional_asset_patterns`](#additional_asset_patterns). The supplemental archives
  are overlaid into the same install directory.
- If the assets are independent tools that should have separate install directories,
  define one tool alias per binary and point each alias at the same
  `github:owner/repo` backend.

Prefer [`matching`](#matching) (or [`matching_regex`](#matching_regex)): it narrows the
candidate set while **keeping platform autodetection**, so one config works on every
OS/arch. This is the right choice when the per-platform asset names can't be templated
portably (e.g. Rust target-triples like `oxlint-aarch64-apple-darwin.tar.gz`).

The example below installs both `oxlint` and `oxfmt` from the single
`oxc-project/oxc` release. Note that each `matching` value must be specific enough to
select **only** the intended binary — if one binary's name were a substring of the
other's, use [`matching_regex`](#matching_regex) with an anchor (e.g. `"^oxlint-"`)
instead (see the [`matching`](#matching) caveat).

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

::: warning
Aliases are not an overlay mechanism. Each alias creates a separate install directory.
Use them for independent binaries such as `oxlint` and `oxfmt`; use
`additional_asset_patterns` when both archives must compose one runnable tool.
:::

If the binary isn't named the way you want to invoke it, add
[`rename_exe`](#rename_exe) (renames the executable extracted from an archive) or
[`bin`](#bin) (selects/renames the binary, including a single bare non-archive binary).

Use [`asset_pattern`](#asset_pattern) instead only when you need full manual control and
can name the asset portably (it replaces autodetection, so any <code v-pre>{{ os() }}</code>/<code v-pre>{{ arch() }}</code>
templating must cover every platform you target):

```toml
[tools.tool-a]
version = "latest"
asset_pattern = "tool-a-*"

[tools.tool-b]
version = "latest"
asset_pattern = "tool-b-*"
```

### `checksum`

Verify the downloaded file with a checksum:

```toml
[tools."github:owner/repo"]
version = "1.0.0"
asset_pattern = "tool-1.0.0-x64.tar.gz"
checksum = "sha256:a1b2c3d4e5f6789..."
```

_Instead of specifying the checksum here, you can use [mise.lock](/dev-tools/mise-lock) to manage checksums._

### Platform-specific Checksums

```toml
[tools."github:cli/cli"]
version = "latest"

[tools."github:cli/cli".platforms]
linux-x64 = {
  asset_pattern = "gh_*_linux_x64.tar.gz",
  checksum = "sha256:a1b2c3d4e5f6789...",
}
macos-arm64 = {
  asset_pattern = "gh_*_macOS_arm64.tar.gz",
  checksum = "sha256:b2c3d4e5f6789...",
}
```

### `size`

Verify the downloaded asset size:

```toml
[tools]
"github:cli/cli" = { version = "latest", size = "12345678" }
```

### `strip_components`

Number of directory components to strip when extracting archives:

```toml
[tools]
"github:cli/cli" = { version = "latest", strip_components = 1 }
```

::: info
If `strip_components` is not explicitly set, mise will automatically detect when to apply `strip_components = 1`. This happens when the extracted archive contains exactly one directory at the root level and no files. This is common with tools like ripgrep that package their binaries in a versioned directory (e.g., `ripgrep-14.1.0-x86_64-unknown-linux-musl/rg`). The auto-detection ensures the binary is placed directly in the install path where mise expects it.
:::

### `bin`

Rename the downloaded binary to a specific name. This is useful when downloading single binaries that have platform-specific names:

```toml
[tools."github:docker/compose"]
version = "2.29.1"
bin = "docker-compose"  # Rename the downloaded binary to docker-compose
```

::: info
When downloading single binaries (not archives), mise automatically removes OS/arch suffixes from the filename. For example, `docker-compose-linux-x86_64` becomes `docker-compose` automatically. Use the `bin` option only when you need a specific custom name.
:::

### `rename_exe`

Rename the executable after extraction from an archive. This is useful when the archive contains a binary with a platform-specific name that you want to rename:

```toml
[tools."github:yt-dlp/yt-dlp"]
version = "latest"
asset_pattern = "yt-dlp_linux.zip"
rename_exe = "yt-dlp"  # Rename the extracted binary to yt-dlp
```

The string form renames the tool's primary binary (matched against the repo name). When an archive ships **multiple** binaries that you want to expose under clean names, use the table form instead — each key is a source name (an exact file name or a glob), and each value is the new name:

```toml
[tools."github:DanielGavin/ols"]
version = "latest"
# archive contains ols-x86_64-unknown-linux-gnu and odinfmt-x86_64-unknown-linux-gnu
rename_exe = { "ols-*" = "ols", "odinfmt-*" = "odinfmt" }
```

Both binaries are renamed and become available on PATH. Missing sources are skipped with a warning, and the executable bit is restored for archives (such as ZIPs) that drop it.

::: tip
Use `rename_exe` for archives where the binary inside has a different name than desired. Use `bin` for single binary downloads (non-archives).
:::

### `no_app`

Skip macOS .app bundle assets during autodetection and prefer standalone CLI binaries instead. This is useful when a repository provides both a macOS .app bundle (often an Xcode extension or GUI application) and a standalone command-line tool:

```toml
[tools."github:nicklockwood/SwiftFormat"]
version = "latest"
rename_exe = "swiftformat"
no_app = true  # Skip SwiftFormat.for.Xcode.app.zip, use swiftformat.zip instead
```

When `no_app = true`:

- Assets containing `.app.` (e.g., `Tool.app.zip`, `Tool.for.Xcode.app.zip`) are penalized during autodetection
- Standalone archives (e.g., `tool.zip`, `tool-macos.tar.gz`) are preferred
- This is mainly useful for macOS asset selection; non-macOS `.app.` assets are already penalized by platform matching
- Only affects autodetection; explicit `asset_pattern` values are used as-is

::: info
Without this option, mise's autodetection might select .app bundles on macOS, which can be problematic if the bundle contains a GUI application or Xcode extension rather than a standalone CLI tool.
:::

### `bin_path`

::: v-pre
Specify the directory containing binaries within the extracted archive, or where to place the downloaded file. This supports Tera templating with `{{ version }}` and the `{{ os() }}` / `{{ arch() }}` functions:
:::

```toml
[tools."github:cli/cli"]
version = "latest"
bin_path = "cli-{{ version }}/bin" # expands to cli-1.0.0/bin
```

Both take keyword arguments that remap the value mise would emit (`linux`, `macos`,
`windows` for `os()`; `x64`, `arm64` for `arch()`), for when upstream names the directory
differently:

```toml
[tools."github:pizlonator/fil-c"]
version = "latest"
# expands to filc-0.681-linux-x86_64/build/bin
bin_path = 'filc-{{ version }}-{{ os() }}-{{ arch(x64="x86_64", arm64="aarch64") }}/build/bin'
```

::: tip
Use a single-quoted TOML string when the template contains double quotes, as above.
:::

::: v-pre
There are no bare `{{ os }}` / `{{ arch }}` variables, and no `{{ x86_64_arch }}`-style
aliases — `{{ arch(x64="x86_64", arm64="aarch64") }}` is how you get those names.
:::

**Binary path lookup order:**

1. If `bin_path` is specified, use that directory
2. If `bin_path` is not set, look for a `bin/` directory in the install path
3. If the install path root contains an executable file, use the install path root
4. If no `bin/` directory exists, search subdirectories for `bin/` directories
5. If no `bin/` directories are found, searches immediate subdirectories for any executable files. If an executable is found directly within a subdirectory, that entire subdirectory is considered a binary path.
6. If no executables are found, use the root of the extracted directory

### `filter_bins`

List of binaries to symlink into a filtered `.mise-bins` directory. This is useful when the tool comes with extra binaries that you do not want to expose on PATH.

```toml
[tools]
"github:jgm/pandoc" = { version = "latest", filter_bins = "pandoc" }
"github:owner/repo" = { version = "latest", filter_bins = ["tool", "helper"] }
```

When enabled:

- A `.mise-bins` subdirectory is created with symlinks only to the specified binaries
- Other binaries (like `pandoc-lua` or `pandoc-server`) are not exposed on PATH

### `api_url`

For GitHub Enterprise or self-hosted GitHub instances, specify the API URL. mise uses this URL for release listing and release asset lookup, and may also use it to download assets when browser download URLs are not reachable or when using custom/private instances:

```toml
[tools]
"github:myorg/mytool" = { version = "latest", api_url = "https://github.mycompany.com/api/v3" }
```

### `github_attestations`

By default, mise checks GitHub Artifact Attestations when they are available for a
GitHub release asset. Set `github_attestations = false` on a single tool to skip
that check while keeping GitHub attestation verification enabled globally:

```toml
[tools]
"github:myorg/mytool" = { version = "latest", github_attestations = false }
```

Use this as a temporary escape hatch for a specific tool if GitHub's attestation
service or trusted-root data is causing installs to fail. Other verification
paths, such as checksums and SLSA provenance, still run when they are configured
and available. If `mise.lock` already records `github-attestations` provenance
for the tool, re-run `mise lock` after disabling this option so the lockfile no
longer requires a verifier that the tool config has turned off.

### `prerelease`

By default, releases flagged `prerelease: true` on GitHub are excluded from `mise ls-remote` and from `latest` resolution. Set `prerelease = true` to include them:

```toml
[tools]
"github:myorg/mytool" = { version = "latest", prerelease = true }
```

When set:

- Pre-release tags (e.g. `v1.0.0-rc1`, `v0.1.2-dev.86`) appear in `mise ls-remote`.
- `latest` resolves to the newest version across stable **and** pre-releases, rather than taking the GitHub `/releases/latest` shortcut (which returns whichever release the repo owner has marked as "Latest" — usually the newest non-prerelease, but it can be any release they've pinned via the API).
- Fuzzy version queries (e.g. `1.2`) match pre-release tags under that prefix.

Useful for repositories whose active releases are all pre-releases (e.g. internal tools shipping continuous dev builds), or when you need to track a project's release candidates. Draft releases are always excluded. Has no effect on GitLab.

## Self-hosted GitHub

If you are using a self-hosted GitHub instance, set the `api_url` tool option. For authentication, see [GitHub Tokens](/dev-tools/github-tokens.html#github-enterprise).

## Supported GitHub Syntax

- **GitHub shorthand for latest release version:** `github:cli/cli`
- **GitHub shorthand for specific release version:** `github:cli/cli@2.40.1`

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>

<Settings child="github" :level="3" />
