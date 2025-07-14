# GitLab Backend

You may install GitLab release assets directly using the `gitlab` backend. This backend downloads release assets from GitLab repositories and is ideal for tools that distribute pre-built binaries through GitLab releases.

The code for this is inside of the mise repository at [`./src/backend/gitlab.rs`](https://github.com/jdx/mise/blob/main/src/backend/gitlab.rs).

## Usage

The following installs the latest version of gitlab-runner from GitLab releases
and sets it as the active version on PATH:

```sh
$ mise use -g gitlab:gitlab-org/gitlab-runner
$ gitlab-runner --version
gitlab-runner 16.8.0
```

The version will be set in `~/.config/mise/config.toml` with the following format:

```toml
[tools]
"gitlab:gitlab-org/gitlab-runner" = { version = "latest", asset_pattern = "gitlab-runner-linux-x64" }
```

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `gitlab` backend—these
go in `[tools]` in `mise.toml`.

### Asset Autodetection

When no `asset_pattern` is specified, mise automatically selects the best asset for your platform. The system scores assets based on:

- **OS compatibility** (linux, macos, windows)
- **Architecture compatibility** (x64, arm64, x86, arm)
- **Libc variant** (gnu or musl, for Linux)
- **Archive format preference** (tar.gz, zip, etc.)
- **Build type** (avoids debug/test builds)

For most tools, you can simply install without specifying patterns:

```sh
mise install gitlab:user/repo
```

::: tip
The autodetection logic is implemented in [`src/backend/asset_detector.rs`](https://github.com/jdx/mise/blob/main/src/backend/asset_detector.rs), which is shared by both the GitHub and GitLab backends.
:::

### `asset_pattern`

Specifies the pattern to match against release asset names. This is useful when there are multiple assets for your OS/arch combination or when you need to override autodetection.

```toml
[tools."gitlab:gitlab-org/gitlab-runner"]
version = "latest"
asset_pattern = "gitlab-runner-linux-x64"
```

### `version_prefix`

Specifies a custom version prefix for release tags. By default, mise handles the common `v` prefix (e.g., `v1.0.0`), but some repositories use different prefixes like `release-`, `version-`, or no prefix at all.

When `version_prefix` is configured, mise will:
- Strip the prefix when listing available versions
- Add the prefix when searching for releases
- Try both prefixed and non-prefixed versions during installation

```toml
[tools]
"gitlab:user/repo" = { version = "latest", version_prefix = "release-" }
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
[tools."gitlab:gitlab-org/gitlab-runner"]
version = "latest"

[tools."gitlab:gitlab-org/gitlab-runner".platforms]
linux-x64 = { asset_pattern = "gitlab-runner-linux-x64" }
macos-arm64 = { asset_pattern = "gitlab-runner-macos-arm64" }
```

### `checksum`

Verify the downloaded file with a checksum:

```toml
[tools."gitlab:owner/repo"]
version = "1.0.0"
asset_pattern = "tool-1.0.0-x64.tar.gz"
checksum = "sha256:a1b2c3d4e5f6789..."
```

*Instead of specifying the checksum here, you can use [mise.lock](/dev-tools/mise-lock) to manage checksums.*

### Platform-specific Checksums

```toml
[tools."gitlab:gitlab-org/gitlab-runner"]
version = "latest"

[tools."gitlab:gitlab-org/gitlab-runner".platforms]
linux-x64 = { asset_pattern = "gitlab-runner-linux-x64", checksum = "sha256:a1b2c3d4e5f6789..." }
macos-arm64 = { asset_pattern = "gitlab-runner-macos-arm64", checksum = "sha256:b2c3d4e5f6789..." }
```

### `size`

Verify the downloaded asset size:

```toml
[tools]
"gitlab:gitlab-org/gitlab-runner" = { version = "latest", size = "12345678" }
```

### Platform-specific Size

You can specify different sizes for different platforms:

```toml
[tools."gitlab:gitlab-org/gitlab-runner"]
version = "latest"

[tools."gitlab:gitlab-org/gitlab-runner".platforms]
linux-x64 = { size = "12345678" }
macos-arm64 = { size = "9876543" }
```

### `strip_components`

Number of directory components to strip when extracting archives:

```toml
[tools]
"gitlab:gitlab-org/gitlab-runner" = { version = "latest", strip_components = 1 }
```

::: info
If `strip_components` is not explicitly set, mise will automatically detect when to apply `strip_components = 1`. This happens when the extracted archive contains exactly one directory at the root level and no files. This is common with tools like ripgrep that package their binaries in a versioned directory (e.g., `ripgrep-14.1.0-x86_64-unknown-linux-musl/rg`). The auto-detection ensures the binary is placed directly in the install path where mise expects it.
:::

### `bin_path`

Specify the directory containing binaries within the extracted archive, or where to place the downloaded file. This supports templating with `{name}`, `{version}`, `{os}`, `{arch}`, and `{ext}`:

```toml
[tools."gitlab:gitlab-org/gitlab-runner"]
version = "latest"
bin_path = "{name}-{version}/bin" # expands to gitlab-runner-1.0.0/bin
```

**Binary path lookup order:**

1. If `bin_path` is specified, use that directory
2. If `bin_path` is not set, look for a `bin/` directory in the install path
3. If no `bin/` directory exists, search subdirectories for `bin/` directories
4. If no `bin/` directories are found, use the root of the extracted directory

### `api_url`

For self-hosted GitLab instances, specify the API URL:

```toml
[tools]
"gitlab:myorg/mytool" = { version = "latest", api_url = "https://gitlab.mycompany.com/api/v4" }
```

## Self-hosted GitLab

If you are using a self-hosted GitLab instance, set the `api_url` tool option and optionally the `MISE_GITLAB_ENTERPRISE_TOKEN` environment variable for authentication:

```sh
export MISE_GITLAB_ENTERPRISE_TOKEN="your-token"
```

## Supported GitLab Syntax

- **GitLab shorthand for latest release version:** `gitlab:gitlab-org/gitlab-runner`
- **GitLab shorthand for specific release version:** `gitlab:gitlab-org/gitlab-runner@16.8.0`

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="gitlab" :level="3" />
