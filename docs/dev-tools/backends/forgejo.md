# Forgejo Backend

You can install release assets from Codeberg and other Forgejo-compatible instances directly using the `forgejo` backend. It downloads release assets from Forgejo repositories and is ideal for tools that distribute pre-built binaries through Forgejo releases.

By default, the Forgejo backend uses the public Codeberg instance at [https://codeberg.org](https://codeberg.org). For other Forgejo instances, including self-hosted ones, specify a custom API URL with the `api_url` tool option.

The code for this is inside the mise repository at [`src/backend/github.rs`](https://github.com/jdx/mise/blob/main/src/backend/github.rs).

## Usage

On Linux, install Forgejo Runner from its own Forgejo instance and check the
executable. Its releases publish Linux binaries; choose a project with matching
assets if you are installing on another operating system:

```sh
mise use 'forgejo:forgejo/runner[api_url=https://code.forgejo.org/api/v1,bin=forgejo-runner]'
mise exec -- forgejo-runner --version
```

Quote the complete tool argument so shell globbing does not interpret its square
brackets. This writes the following project configuration:

```toml
[tools]
"forgejo:forgejo/runner" = {
  version = "latest",
  api_url = "https://code.forgejo.org/api/v1",
  bin = "forgejo-runner",
}
```

Add `-g` to `mise use` for a global tool. This installs the runner executable;
registering it with a server and running it as a service are separate steps.
For Codeberg repositories, omit `api_url`.

## Authentication

For private repositories or higher API limits, mise supports several Forgejo token sources.

### Token priority

mise checks these sources in order and uses the first token found:

1. `MISE_FORGEJO_ENTERPRISE_TOKEN` (for non-`codeberg.org` hosts)
2. `MISE_FORGEJO_TOKEN`
3. `FORGEJO_TOKEN`
4. `credential_command` (if set)
5. `forgejo_tokens.toml` (per host)
6. `fj` CLI config (`keys.json`, if enabled)
7. `git credential fill` (if `forgejo.use_git_credentials=true`)

### Environment variables

```sh
export MISE_FORGEJO_TOKEN="forgejo-token"
```

For self-hosted Forgejo instances:

```sh
export MISE_FORGEJO_ENTERPRISE_TOKEN="forgejo-enterprise-token"
```

### Token file (`forgejo_tokens.toml`)

```toml
# ~/.config/mise/forgejo_tokens.toml
[tokens."codeberg.org"]
token = "forgejo-public-token"

[tokens."forgejo.mycompany.com"]
token = "forgejo-enterprise-token"
```

### `credential_command`

Set this in your **global** `~/.config/mise/config.toml`. Project configuration
cannot set `credential_command`. The command must print only the token to stdout:

```toml
[settings.forgejo]
credential_command = "op read 'op://Private/Forgejo Token/credential'"
```

mise executes this command with the configured default inline shell. The target hostname is available as `MISE_CREDENTIAL_HOST`, and the provider name (`forgejo`) is available as `MISE_CREDENTIAL_PROVIDER`. For compatibility, recognized sh-compatible shells (`ash`, `bash`, `dash`, `ksh`, `sh`, and `zsh`) also receive the hostname as `$1`/`${1}`.

:::: warning Planned deprecation
The legacy `$1`/`${1}` hostname argument is deprecated. Use `MISE_CREDENTIAL_HOST` instead. mise will start warning in `2026.11.0`, and `$1` compatibility will be removed in `2027.11.0`.
::::

### `fj` CLI integration

mise can read tokens from the [`fj` CLI](https://codeberg.org/forgejo-contrib/forgejo-cli) (`keys.json`) as a fallback. It checks:

1. `$XDG_DATA_HOME/forgejo-cli/keys.json` (defaults to `~/.local/share/forgejo-cli/keys.json`)
2. `~/Library/Application Support/forgejo-cli.forgejo-cli/keys.json` (macOS)
3. `~/Library/Application Support/Cyborus.forgejo-cli/keys.json` (legacy macOS location)

Disable this fallback with:

```toml
[settings.forgejo]
fj_cli_tokens = false
```

### `git credential fill` fallback

As a last resort, mise can query git credential helpers:

```toml
[settings.forgejo]
use_git_credentials = true
```

This uses `git credential fill` and supports credentials stored by helpers such as macOS Keychain.

### Debugging token resolution

Use `mise token forgejo` to see which token mise would use for a given host:

```sh
mise token forgejo
mise token forgejo forgejo.mycompany.com
```

Token diagnostics are masked by default. `--unmask` prints the actual credential;
use it only when you need the secret itself, and keep it out of shared logs.

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `forgejo` backend—these
go in `[tools]` in `mise.toml`.

### Asset Autodetection

When no `asset_pattern` is specified, mise automatically selects the best asset for your platform. It scores assets on:

- **OS compatibility** (linux, macos, windows)
- **Architecture compatibility** (x64, arm64, x86, arm)
- **Libc variant** (gnu or musl for Linux, msvc for Windows)
- **Archive format preference** (tar.gz, zip, etc.)
- **Build type** (avoids debug/test builds)

For most tools, you can install without specifying a pattern:

```sh
mise install forgejo:user/repo
```

::: tip
The autodetection logic is implemented in [`src/backend/asset_matcher.rs`](https://github.com/jdx/mise/blob/main/src/backend/asset_matcher.rs), which is shared by the GitHub, GitLab, and Forgejo backends.
:::

### `asset_pattern`

Specifies the pattern to match against release asset names. This is useful when there are multiple assets for your OS/arch combination or when you need to override autodetection.

```toml
[tools]
"forgejo:user/repo" = { version = "latest", asset_pattern = "tool_*_linux_x64.tar.gz" }
```

### `matching`

Narrows asset selection to names containing the given substring, **while keeping platform autodetection**. Unlike [`asset_pattern`](/dev-tools/backends/forgejo.html#asset-pattern) (which replaces autodetection entirely), `matching` only refines the candidate set — autodetection still chooses the correct OS/arch from the narrowed list, so a single config stays portable across platforms.

This is the option to reach for when a repository ships **multiple binaries as separate per-platform assets** and autodetection can't tell which one you want.

```toml
[tools]
# When a release ships several binaries per platform (e.g. `mytool-cli` and
# `mytool-server`), matching picks one on every OS/arch without hardcoding a
# platform-specific asset_pattern.
"forgejo:user/repo" = { version = "latest", matching = "mytool-cli" }
```

Tool options can also be passed inline on the command line using `[key=value]` syntax:

```sh
mise use "forgejo:user/repo[matching=mytool-cli]"
```

`matching` is a case-sensitive substring test, so a value that is also a substring of another asset's name (e.g. `matching = "tool"` when both `tool-*` and `tool-extras-*` are published) won't uniquely select your binary. Use [`matching_regex`](/dev-tools/backends/forgejo.html#matching-regex) with an anchor when you need a precise match.

If [`asset_pattern`](/dev-tools/backends/forgejo.html#asset-pattern) is also set, it takes precedence and `matching`/`matching_regex` are ignored — `asset_pattern` replaces autodetection entirely, so there is no candidate set left for them to narrow. They are ignored silently: when `asset_pattern` is set, a `matching_regex` is never consulted and an invalid one is not reported, since mise does not error on a superseded option.

### `matching_regex`

Like [`matching`](#matching), but the asset name must match the given regular expression. Use this when a substring isn't selective enough. The match is case-sensitive; use an inline `(?i)` flag for case-insensitive matching.

```toml
[tools]
"forgejo:user/repo" = { version = "latest", matching_regex = "^mytool-cli-" }
```

If both `matching` and `matching_regex` are set, an asset must satisfy **both** (logical AND)
to remain a candidate.

::: warning
`matching`/`matching_regex` are **not** part of the install path — it is keyed by the tool
name (`user/repo`, or a `tool_alias`) and version. To install two binaries from the same
release, give each its own [`tool_alias`](/dev-tools/backends/github.html#multiple-assets-from-the-same-release)
so they get distinct install directories; reusing the same `forgejo:user/repo` string with
different `matching` values resolves to the same directory and the second install overwrites
the first.
:::

### `version_prefix`

Specifies a custom version prefix for release tags. By default, mise handles the common `v` prefix (e.g., `v1.0.0`), but some repositories use different prefixes like `release-`, `version-`, or no prefix at all.

When `version_prefix` is configured, mise will:

- Filter available versions with the prefix and strip it
- Add the prefix when searching for releases
- Try both prefixed and non-prefixed versions during installation

```toml
[tools]
"forgejo:user/repo" = { version = "latest", version_prefix = "release-" }
```

**Examples:**

- With `version_prefix = "release-"`:
  - User specifies `1.0.0` → mise searches for the `release-1.0.0` tag
  - Available versions show as `1.0.0` (prefix stripped)
- With `version_prefix = ""` (empty string):
  - User specifies `1.0.0` → mise searches for the `1.0.0` tag (no prefix)
  - Useful for repositories that don't use any prefix

### `prerelease`

By default, releases flagged `prerelease: true` on Forgejo are excluded from `mise ls-remote` and from `latest` resolution. Set `prerelease = true` to include them:

```toml
[tools]
"forgejo:user/repo" = { version = "latest", prerelease = true }
```

When set:

- Pre-release tags (e.g. `v1.0.0-rc1`, `v0.1.2-dev.86`) appear in `mise ls-remote`.
- `latest` resolves to the newest version across stable and pre-releases, rather than taking the Forgejo `/repos/{owner}/{repo}/releases/latest` shortcut.
- Fuzzy version queries (e.g. `1.2`) match pre-release tags under that prefix.

Draft releases are always excluded.

### Platform-specific Asset Patterns

For different asset patterns per platform:

```toml
[tools."forgejo:user/repo"]
version = "latest"

[tools."forgejo:user/repo".platforms]
linux-x64 = { asset_pattern = "tool_*_linux_x64.tar.gz" }
macos-arm64 = { asset_pattern = "tool_*_macOS_arm64.tar.gz" }
```

### `checksum`

Set an expected digest for a **specific version and artifact**. Replace the
placeholder below with the full SHA-256 digest obtained from a trusted source:

```toml
[tools."forgejo:owner/repo"]
version = "1.0.0"
asset_pattern = "tool-1.0.0-x64.tar.gz"
checksum = "sha256:REPLACE_WITH_THE_64_HEX_DIGIT_DIGEST"
```

_Instead of specifying the checksum here, you can use [mise.lock](/dev-tools/mise-lock) to manage checksums._

### Platform-specific Checksums

Each platform needs its own digest. These values are placeholders; fill them
from the publisher before installing, or generate [mise.lock](/dev-tools/mise-lock.html).

```toml
[tools."forgejo:user/repo"]
version = "1.0.0"

[tools."forgejo:user/repo".platforms]
linux-x64 = {
  asset_pattern = "tool_*_linux_x64.tar.gz",
  checksum = "sha256:REPLACE_WITH_THE_64_HEX_DIGIT_DIGEST",
}
macos-arm64 = {
  asset_pattern = "tool_*_macOS_arm64.tar.gz",
  checksum = "sha256:REPLACE_WITH_THE_64_HEX_DIGIT_DIGEST",
}
```

### `size`

Optionally check the expected byte count. The number below is illustrative;
use the selected artifact's actual size and pin its version. A size check does
not authenticate the publisher or replace a checksum:

```toml
[tools]
"forgejo:user/repo" = { version = "1.0.0", size = "12345678" }
```

### `strip_components`

Number of directory components to strip when extracting archives:

```toml
[tools]
"forgejo:user/repo" = { version = "latest", strip_components = 1 }
```

::: info
When both `strip_components` and `bin_path` are unset, mise automatically applies `strip_components = 1` when the extracted archive contains exactly one directory at the root and no files. This is common with tools like ripgrep that package their binaries in a versioned directory (e.g., `mytool-14.1.0-x86_64-unknown-linux-musl/mytool`). The autodetection ensures the binary is placed directly in the install path where mise expects it.
:::

### `bin`

Rename the downloaded binary to a specific name. This is useful when downloading single binaries that have platform-specific names:

```toml
[tools."forgejo:user/repo"]
version = "2.29.1"
bin = "my-tool"  # Rename the downloaded binary to my-tool
```

::: info
When downloading single binaries (not archives), mise automatically removes OS/arch suffixes from the filename. For example, `docker-compose-linux-x86_64` becomes `docker-compose` automatically. Use the `bin` option only when you need a specific custom name.
:::

### `rename_exe`

Rename the executable after extraction from an archive. This is useful when the archive contains a binary with a platform-specific name:

```toml
[tools."forgejo:user/repo"]
version = "latest"
asset_pattern = "tool_linux.zip"
rename_exe = "tool"  # Rename the extracted binary to tool
```

::: tip
Use `rename_exe` for archives whose binary has a different name than you want. Use `bin` for single-binary downloads (non-archives).
:::

### `no_app`

Skip macOS .app bundle assets during autodetection and prefer standalone CLI binaries. This is useful when a repository provides both a macOS .app bundle (often an Xcode extension or GUI application) and a standalone command-line tool:

```toml
[tools."forgejo:user/repo"]
version = "latest"
no_app = true
```

When `no_app = true`:

- Assets containing `.app.` (e.g., `Tool.app.zip`, `Tool.for.Xcode.app.zip`) are penalized during autodetection
- Standalone archives are preferred
- The option is mainly useful for macOS asset selection; non-macOS `.app.` assets are already penalized by platform matching
- Only autodetection is affected; explicit `asset_pattern` values are used as-is

### `bin_path`

Paths are relative to the install directory **after** `strip_components` is
applied. Setting `bin_path` disables automatic root stripping. For an archive
shaped like `tool-VERSION/bin/tool`, either retain the outer directory and use
`bin_path = "tool-{{ version }}/bin"`, or set both `strip_components = 1` and
`bin_path = "bin"`.

::: v-pre
Specify the directory containing binaries within the extracted archive, or where to place the downloaded file. This supports Tera templating with `{{ version }}` and the `{{ os() }}` / `{{ arch() }}` functions:
:::

```toml
[tools."forgejo:user/repo"]
version = "latest"
strip_components = 0
bin_path = "tool-{{ version }}/bin" # retain the outer tool-1.0.0 directory
```

Both functions take keyword arguments that remap the value mise would emit (`linux`, `macos`,
`windows` for `os()`; `x64`, `arm64` for `arch()`), for cases where upstream names the directory
differently:

```toml
[tools."forgejo:user/repo"]
version = "latest"
# expands to tool-1.0.0-linux-x86_64/bin
strip_components = 0
bin_path = 'tool-{{ version }}-{{ os() }}-{{ arch(x64="x86_64", arm64="aarch64") }}/bin'
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
5. If no `bin/` directories are found, search immediate subdirectories for any executable files. If an executable is found directly within a subdirectory, that subdirectory is treated as a binary path.
6. If no executables are found, use the root of the extracted directory

### `filter_bins`

List of binaries to symlink into a filtered `.mise-bins` directory. This is useful when the tool comes with extra binaries that you do not want to expose on PATH.

```toml
[tools]
"forgejo:user/repo" = { version = "latest", filter_bins = "tool" }
"forgejo:user/other-repo" = { version = "latest", filter_bins = ["tool", "helper"] }
```

When enabled:

- A `.mise-bins` subdirectory is created with symlinks only to the specified binaries
- Other binaries (like `tool-helper` or `tool-server`) are not exposed on PATH

### `api_url`

For other Forgejo-compatible or self-hosted instances, specify the API URL. mise uses it to list releases and look up release assets, and may also use it to download assets when browser download URLs are not reachable or when using custom/private instances:

```toml
[tools]
"forgejo:user/repo" = { version = "latest", api_url = "https://forgejo.mycompany.com/api/v1" }
```

## Self-hosted Forgejo

If you are using a self-hosted Forgejo instance, set the `api_url` tool option and optionally the `MISE_FORGEJO_ENTERPRISE_TOKEN` environment variable for authentication:

```sh
export MISE_FORGEJO_ENTERPRISE_TOKEN="your-token"
```

## Supported Forgejo Syntax

- **Forgejo shorthand for latest release version:** `forgejo:user/repo`
- **Forgejo shorthand for specific release version:** `forgejo:user/repo@2.40.1`

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>

<Settings child="forgejo" :level="3" />
