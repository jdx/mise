# HTTP Backend <Badge type="warning" text="experimental" />

You may install tools directly from HTTP URLs using the `http` backend. This backend downloads files from any HTTP/HTTPS URL and is ideal for tools that distribute pre-built binaries or archives through direct download links.

The code for this is inside of the mise repository at [`./src/backend/http.rs`](https://github.com/jdx/mise/blob/main/src/backend/http.rs).

## Usage

The following installs a tool from a direct HTTP URL:

```sh
mise use -g http:my-tool[url=https://example.com/releases/my-tool-v1.0.0.tar.gz]@1.0.0
```

The version will be set in `~/.config/mise/config.toml` with the following format:

```toml
[tools]
"http:my-tool" = { version = "1.0.0", url = "https://example.com/releases/my-tool-v1.0.0.tar.gz" }
```

## Supported HTTP Syntax

- **HTTP with URL parameter:** `http:my-tool[url=https://example.com/releases/my-tool-v1.0.0.tar.gz]@1.0.0`

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `http` backend—these
go in `[tools]` in `mise.toml`.

### `url` (Required)

Specifies the HTTP URL to download the tool from. The URL supports templating with `{{version}}`:

```toml
[tools]
"http:my-tool" = { version = "1.0.0", url = "https://example.com/releases/my-tool-v{{version}}.tar.gz" }
```

You can also use static URLs without templating:

```toml
[tools]
"http:my-tool" = { version = "1.0.0", url = "https://example.com/releases/my-tool-v1.0.0.tar.gz" }
```

### Platform-specific URLs

For tools that need different downloads per platform, use the table format:

```toml
[tools."http:my-tool"]
version = "1.0.0"

[tools."http:my-tool".platforms]
macos-x64 = { url = "https://example.com/releases/my-tool-v1.0.0-macos-x64.tar.gz" }
macos-arm64 = { url = "https://example.com/releases/my-tool-v1.0.0-macos-arm64.tar.gz" }
linux-x64 = { url = "https://example.com/releases/my-tool-v1.0.0-linux-x64.tar.gz" }
```

::: tip
You can use either `macos` or `darwin`, and `x64` or `amd64` for platform keys. `macos` and `x64` are preferred in documentation and examples, but all variants are accepted.

OS/architecture values use mise's conventions: `linux`, `macos`, `windows` for operating systems and `x64`, `arm64` for architectures. For platform-specific URLs, use the appropriate platform key (e.g., `macos-x64`, `linux-arm64`) and specify the full URL for each platform.

If you mess up and use something like `darwin-aarch64` mise will try to figure out what
you meant and do the right thing anyhow.
:::

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

```toml
[tools."http:my-tool"]
version = "1.0.0"

[tools."http:my-tool".platforms]
macos-x64 = { url = "https://example.com/releases/my-tool-v1.0.0-macos-x64.tar.gz", checksum = "sha256:a1b2c3d4e5f6789..." }
macos-arm64 = { url = "https://example.com/releases/my-tool-v1.0.0-macos-arm64.tar.gz", checksum = "sha256:b2c3d4e5f6789..." }
linux-x64 = { url = "https://example.com/releases/my-tool-v1.0.0-linux-x64.tar.gz", checksum = "sha256:c3d4e5f6789..." }
```

### `size`

Verify the downloaded file size:

```toml
[tools."http:my-tool"]
version = "1.0.0"
url = "https://example.com/releases/my-tool-v1.0.0.tar.gz"
size = "12345678"
```

### Platform-specific Size

You can specify different sizes for different platforms:

```toml
[tools."http:my-tool"]
version = "1.0.0"

[tools."http:my-tool".platforms]
macos-x64 = { url = "https://example.com/releases/my-tool-v1.0.0-macos-x64.tar.gz", size = "12345678" }
macos-arm64 = { url = "https://example.com/releases/my-tool-v1.0.0-macos-arm64.tar.gz", size = "9876543" }
linux-x64 = { url = "https://example.com/releases/my-tool-v1.0.0-linux-x64.tar.gz", size = "11111111" }
```

### `strip_components`

Number of directory components to strip when extracting archives:

```toml
[tools."http:my-tool"]
version = "1.0.0"
url = "https://example.com/releases/my-tool-v1.0.0.tar.gz"
strip_components = 1
```

::: info
If `strip_components` is not explicitly set, mise will automatically detect when to apply `strip_components = 1`. This happens when the extracted archive contains exactly one directory at the root level and no files. This is common with tools like ripgrep that package their binaries in a versioned directory (e.g., `ripgrep-14.1.0-x86_64-unknown-linux-musl/rg`). The auto-detection ensures the binary is placed directly in the install path where mise expects it.
:::

### `bin_path`

Specify the directory containing binaries within the extracted archive, or where to place the downloaded file. This supports templating with `{{version}}`:

```toml
[tools."http:my-tool"]
version = "1.0.0"
url = "https://example.com/releases/my-tool-v1.0.0.tar.gz"
bin_path = "my-tool-{{version}}/bin" # expands to my-tool-1.0.0/bin
```

**Binary path lookup order:**

1. If `bin_path` is specified, use that directory
2. If `bin_path` is not set, look for a `bin/` directory in the install path
3. If no `bin/` directory exists, search subdirectories for `bin/` directories
4. If no `bin/` directories are found, use the root of the extracted directory

## Caching Behavior

The HTTP backend implements an intelligent caching system to optimize disk usage and installation speed:

### Cache Location

Downloaded and extracted files are cached in `$MISE_CACHE_DIR/http-tarballs/` instead of being stored separately for each tool installation. By default:
- **Linux**: `~/.cache/mise/http-tarballs/`
- **macOS**: `~/Library/Caches/mise/http-tarballs/`

### Cache Key Generation

Cache keys are generated based on the file content to ensure identical downloads are shared across tools:

1. **Blake3 hash of file content**: When no checksum is provided, mise calculates a Blake3 hash of the downloaded file
2. **Extraction options**: `strip_components` is included in the cache key since it affects the extracted structure

Example cache directory structure:
```
~/.cache/mise/http-tarballs/
├── 71f774faa03daf1a58cc3339f8c73e6557348c8e0a2f3fb8148cc26e26bad83f/
│   ├── extracted/
│   │   └── bin/my-tool
│   └── metadata.json
└── 1c2af379bdf1fed266bc44b49271e2df5b0dafae09f1cc744b3505ec50c84719_strip_1/
    ├── extracted/
    │   └── my-tool
    └── metadata.json
```

### Symlinked Installations

Tool installations are symlinks to the cached extracted content:

```bash
~/.local/share/mise/installs/http-my-tool/1.0.0 → ~/.cache/mise/http-tarballs/71f774.../extracted
```

This approach provides several benefits:

- **Space efficiency**: Multiple tools using the same tarball share a single cached copy
- **Faster installations**: Cache hits avoid re-downloading and re-extracting files
- **Consistency**: Identical file content always uses the same cache entry

### Cache Metadata

Each cache entry includes a `metadata.json` file with information about the cached content:

```json
{
  "url": "https://example.com/releases/my-tool-v1.0.0.tar.gz",
  "checksum": "sha256:a1b2c3d4e5f6789...",
  "size": 1024000,
  "extracted_at": 1703001234,
  "platform": "macos-arm64"
}
```

### Cache Management

The HTTP backend cache follows mise's standard cache management:

- Cache entries can be cleared with `mise cache clear`
- The cache directory respects the `MISE_CACHE_DIR` environment variable
- **Autopruner**: mise automatically cleans up unused cache entries after 30 days of inactivity
- Manual cleanup is available with `mise cache clear` if needed
