# HTTP Backend <Badge type="warning" text="experimental" />

You may install tools directly from HTTP URLs using the `http` backend. This backend downloads files from any HTTP/HTTPS URL and is ideal for tools that distribute pre-built binaries or archives through direct download links.

The code for this is inside of the mise repository at [`./src/backend/http.rs`](https://github.com/jdx/mise/blob/main/src/backend/http.rs).

## Usage

The following installs a tool from a direct HTTP URL:

```sh
mise use -g http:my-tool@1.0.0[url=https://example.com/releases/my-tool-v1.0.0.tar.gz]
```

The version will be set in `~/.config/mise/config.toml` with the following format:

```toml
[tools]
"http:my-tool" = { version = "1.0.0", url = "https://example.com/releases/my-tool-v1.0.0.tar.gz" }
```

## Supported HTTP Syntax

- **HTTP with URL parameter:** `http:my-tool@1.0.0[url=https://example.com/releases/my-tool-v1.0.0.tar.gz]`

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `http` backend—these
go in `[tools]` in `mise.toml`.

### `url` (Required)

Specifies the HTTP URL to download the tool from:

```toml
[tools]
"http:my-tool" = { version = "1.0.0", url = "https://example.com/releases/my-tool-v1.0.0.tar.gz" }
```

### Platform-specific URLs

You can specify different URLs for different platforms:

```toml
[tools."http:my-tool"]
version = "1.0.0"
platforms_macos_x64_url = "https://example.com/releases/my-tool-v1.0.0-macos-x64.tar.gz"
platforms_macos_arm64_url = "https://example.com/releases/my-tool-v1.0.0-macos-arm64.tar.gz"
platforms_linux_x64_url = "https://example.com/releases/my-tool-v1.0.0-linux-x64.tar.gz"
```

> **Note:** You can use either `macos` or `darwin`, and `x64` or `amd64` for platform keys. `macos` and `x64` are preferred in documentation and examples, but all variants are accepted.

### `checksum`

Verify the downloaded file with a checksum:

```toml
[tools."http:my-tool"]
version = "1.0.0"
url = "https://example.com/releases/my-tool-v1.0.0.tar.gz"
checksum = "sha256:a1b2c3d4e5f6789..."
```

*Instead of specifying the checksum here, you can use [mise.lock](/dev-tools/mise-lock) to manage checksums.*

### Platform-specific Checksums

You can specify different checksums for different platforms:

```toml
[tools."http:my-tool"]
version = "1.0.0"
platforms_macos_x64_url = "https://example.com/releases/my-tool-v1.0.0-macos-x64.tar.gz"
platforms_macos_x64_checksum = "sha256:a1b2c3d4e5f6789..."
platforms_macos_arm64_url = "https://example.com/releases/my-tool-v1.0.0-macos-arm64.tar.gz"
platforms_macos_arm64_checksum = "sha256:b2c3d4e5f6789..."
platforms_linux_x64_url = "https://example.com/releases/my-tool-v1.0.0-linux-x64.tar.gz"
platforms_linux_x64_checksum = "sha256:a1b2c3d4e5f6789..."
```

### `size`

Verify the downloaded file size:

```toml
[tools."http:my-tool"]
version = "1.0.0"
url = "https://example.com/releases/my-tool-v1.0.0.tar.gz"
size = "12345678"
```

### `strip_components`

Number of directory components to strip when extracting archives:

```toml
[tools."http:my-tool"]
version = "1.0.0"
url = "https://example.com/releases/my-tool-v1.0.0.tar.gz"
strip_components = 1
```

### `bin_path`

Specify the directory containing binaries within the extracted archive, or where to place the downloaded file:

```toml
[tools."http:my-tool"]
version = "1.0.0"
url = "https://example.com/releases/my-tool-v1.0.0.tar.gz"
bin_path = "bin"
```

**Binary path lookup order:**

1. If `bin_path` is specified, use that directory
2. If `bin_path` is not set, look for a `bin/` directory in the install path
3. If no `bin/` directory exists, search subdirectories for `bin/` directories
4. If no `bin/` directories are found, use the root of the extracted directory
