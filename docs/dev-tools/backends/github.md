# GitHub Backend

You may install GitHub release assets directly using the `github` backend. This backend downloads release assets from GitHub repositories and is ideal for tools that distribute pre-built binaries through GitHub releases.

The code for this is inside of the mise repository at [`./src/backend/github.rs`](https://github.com/jdx/mise/blob/main/src/backend/github.rs).

## Usage

The following installs the latest version of ripgrep from GitHub releases
and sets it as the active version on PATH:

```sh
$ mise use -g github:BurntSushi/ripgrep
$ rg --version
ripgrep 14.1.1
```

The version will be set in `~/.config/mise/config.toml` with the following format:

```toml
[tools]
"github:BurntSushi/ripgrep" = "latest"
```

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `github` backend—these
go in `[tools]` in `mise.toml`.

### `asset_pattern`

Specifies the pattern to match against release asset names. This is useful when there are multiple assets for your OS/arch combination.

```toml
[tools]
"github:cli/cli" = { version = "latest", asset_pattern = "gh_*_linux_x64.tar.gz" }
```

### Platform-specific Asset Patterns

You can specify different asset patterns for different platforms:

```toml
[tools]
"github:cli/cli" = { 
  version = "latest",
  platforms_linux_x64_asset_pattern = "gh_*_linux_x64.tar.gz",
  platforms_macos_arm64_asset_pattern = "gh_*_macOS_arm64.tar.gz"
}
```

### `checksum`

Verify the downloaded file with a checksum:

```toml
[tools."github:owner/repo"]
version = "1.0.0"
asset_pattern = "tool-1.0.0-x64.tar.gz"
checksum = "sha256:a1b2c3d4e5f6789..."
```

*Instead of specifying the checksum here, you can use [mise.lock](/dev-tools/mise-lock) to manage checksums.*

### Platform-specific Checksums

You can specify different checksums for different platforms:

```toml
[tools]
"github:cli/cli" = { 
  version = "latest",
  platforms_linux_x64_checksum = "sha256:a1b2c3d4e5f6789...",
  platforms_macos_arm64_checksum = "sha256:b2c3d4e5f6789..."
}
```

### `size`

Verify the downloaded asset size:

```toml
[tools]
"github:cli/cli" = { 
  version = "latest", 
  size = "12345678" 
}
```

### `strip_components`

Number of directory components to strip when extracting archives:

```toml
[tools]
"github:cli/cli" = { 
  version = "latest", 
  strip_components = 1 
}
```

### `bin_path`

Specify the directory containing binaries within the extracted archive:

```toml
[tools]
"github:cli/cli" = { 
  version = "latest", 
  bin_path = "bin" 
}
```

**Binary path lookup order:**

1. If `bin_path` is specified, use that directory
2. If `bin_path` is not set, look for a `bin/` directory in the install path
3. If no `bin/` directory exists, search subdirectories for `bin/` directories
4. If no `bin/` directories are found, use the root of the extracted directory

### `api_url`

For GitHub Enterprise or self-hosted GitHub instances, specify the API URL:

```toml
[tools]
"github:myorg/mytool" = { 
  version = "latest", 
  api_url = "https://github.mycompany.com/api/v3" 
}
```

## Self-hosted GitHub

If you are using a self-hosted GitHub instance, set the `api_url` tool option and optionally the `MISE_GITHUB_ENTERPRISE_TOKEN` environment variable for authentication:

```sh
export MISE_GITHUB_ENTERPRISE_TOKEN="your-token"
```

## Supported GitHub Syntax

- **GitHub shorthand for latest release version:** `github:cli/cli`
- **GitHub shorthand for specific release version:** `github:cli/cli@2.40.1`

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="github" :level="3" />

::: warning
The GitHub backend is experimental and requires the `mise.experimental` setting to be enabled:

```sh
mise settings set experimental true
```

:::
