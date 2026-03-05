# HTTP Backend

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

Specifies the HTTP URL to download the tool from. The URL supports templating with variables like `version`, `os()`, and `arch()`:

```toml
[tools]
"http:my-tool" = { version = "1.0.0", url = "https://example.com/releases/my-tool-v{{version}}.tar.gz" }
```

You can also use static URLs without templating:

```toml
[tools]
"http:my-tool" = { version = "1.0.0", url = "https://example.com/releases/my-tool-v1.0.0.tar.gz" }
```

#### Template Variables

The following template functions are available in URLs (use double curly braces, e.g., `version` becomes <code v-pre>{{version}}</code>):

- `version` - The tool version
- `os()` - Operating system: `macos`, `linux`, or `windows`
- `arch()` - Architecture: `x64` or `arm64`
- `os_family()` - OS family: `unix` or `windows`

The `os()` and `arch()` functions support remapping for tools that use different naming conventions:

```toml
[tools]
# HashiCorp tools use "darwin" instead of "macos" and "amd64" instead of "x64"
"http:sentinel" = {
  version = "latest",
  url = 'https://releases.hashicorp.com/sentinel/{{version}}/sentinel_{{version}}_{{os(macos="darwin")}}_{{arch(x64="amd64")}}.zip',
}
```

This produces URLs like:

- macOS arm64: `sentinel_0.26.3_darwin_arm64.zip`
- macOS x64: `sentinel_0.26.3_darwin_amd64.zip`
- Linux x64: `sentinel_0.26.3_linux_amd64.zip`

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

_Instead of specifying the checksum here, you can use [mise.lock](/dev-tools/mise-lock) to manage checksums._

### Platform-specific Checksums

```toml
[tools."http:my-tool"]
version = "1.0.0"

[tools."http:my-tool".platforms]
macos-x64 = {
  url = "https://example.com/releases/my-tool-v1.0.0-macos-x64.tar.gz",
  checksum = "sha256:a1b2c3d4e5f6789...",
}
macos-arm64 = {
  url = "https://example.com/releases/my-tool-v1.0.0-macos-arm64.tar.gz",
  checksum = "sha256:b2c3d4e5f6789...",
}
linux-x64 = {
  url = "https://example.com/releases/my-tool-v1.0.0-linux-x64.tar.gz",
  checksum = "sha256:c3d4e5f6789...",
}
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
macos-x64 = {
  url = "https://example.com/releases/my-tool-v1.0.0-macos-x64.tar.gz",
  size = "12345678",
}
macos-arm64 = {
  url = "https://example.com/releases/my-tool-v1.0.0-macos-arm64.tar.gz",
  size = "9876543",
}
linux-x64 = {
  url = "https://example.com/releases/my-tool-v1.0.0-linux-x64.tar.gz",
  size = "11111111",
}
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

### `bin`

Rename the downloaded binary to a specific name. This is useful when downloading single binaries that have platform-specific names:

```toml
[tools."http:docker-compose"]
version = "2.29.1"
url = "https://github.com/docker/compose/releases/download/v{{ version }}/docker-compose-linux-x86_64"
bin = "docker-compose"  # Rename from docker-compose-linux-x86_64 to docker-compose
```

::: info
When downloading single binaries (not archives), mise automatically removes OS/arch suffixes from the filename. For example, `docker-compose-linux-x86_64` becomes `docker-compose` automatically. Use the `bin` option only when you need a specific custom name.
:::

### `rename_exe`

Rename the executable inside an extracted archive to a specific name. This is useful when archives contain binaries with platform-specific names or when installing kubectl plugins that need specific naming:

```toml
[tools."http:openunison-cli"]
version = "1.0.0"
url = "https://nexus.tremolo.io/repository/openunison-cli/openunison-cli-v{{version}}-linux.zip"
rename_exe = "kubectl-openunison-cli"  # Rename extracted binary for kubectl plugin
```

This works by searching for the first executable in the extracted directory (or `bin_path` if specified) and renaming it to the specified name.

::: tip
Use `bin` for renaming single binary downloads, and `rename_exe` for renaming executables inside archives.
:::

### `format`

Explicitly specify the archive format when the URL lacks a file extension or has an incorrect extension:

```toml
[tools."http:my-tool"]
version = "1.0.0"
url = "https://example.com/releases/my-tool-v1.0.0"
format = "tar.xz"  # Explicitly specify the format
```

::: info
If `format` is not specified, mise will automatically detect the format from the file extension in the URL. Only use `format` when the URL doesn't have a proper extension or when you need to override the detected format.
:::

### Platform-specific Format

You can specify different formats for different platforms:

```toml
[tools."http:my-tool"]
version = "1.0.0"

[tools."http:my-tool".platforms]
macos-x64 = {
  url = "https://example.com/releases/my-tool-v1.0.0-macos-x64",
  format = "tar.xz",
}
linux-x64 = {
  url = "https://example.com/releases/my-tool-v1.0.0-linux-x64",
  format = "tar.gz",
}
windows-x64 = {
  url = "https://example.com/releases/my-tool-v1.0.0-windows-x64",
  format = "zip",
}
```

### `version_list_url`

Fetch available versions from a remote URL. This enables `mise ls-remote` to list available versions for HTTP-based tools:

```toml
[tools."http:my-tool"]
version = "1.0.0"
url = "https://example.com/releases/my-tool-v{{version}}.tar.gz"
version_list_url = "https://example.com/releases/versions.txt"
```

The version list URL can return data in multiple formats:

- **Plain text**: A single version number (e.g., `2.0.53`)
- **Line-separated**: One version per line
- **JSON array of strings**: `["1.0.0", "1.1.0", "2.0.0"]`
- **JSON array of objects**: `[{"version": "1.0.0"}, {"tag_name": "v2.0.0"}]`
- **JSON object with versions array**: `{"versions": ["1.0.0", "2.0.0"]}`

Version prefixes like `v` are automatically stripped.

### `version_regex`

Extract versions from the version list URL response using a regular expression:

```toml
[tools."http:my-tool"]
version = "1.0.0"
url = "https://example.com/releases/my-tool-v{{version}}.tar.gz"
version_list_url = "https://example.com/releases/"
version_regex = 'my-tool-v(\d+\.\d+\.\d+)\.tar\.gz'
```

The first capturing group is used as the version. If no capturing group is present, the entire match is used.

### `version_json_path`

Extract versions from JSON responses using a jq-like path expression:

```toml
[tools."http:my-tool"]
version = "1.0.0"
url = "https://example.com/releases/my-tool-v{{version}}.tar.gz"
version_list_url = "https://api.example.com/releases"
version_json_path = ".[].tag_name"
```

Supported path expressions:

- `.` - root value
- `.[]` - iterate over array elements
- `.[].field` - extract field from each array element
- `.field` - extract field from object
- `.field[]` - iterate over array in field
- `.field.subfield` - nested field access
- `.data.versions[]` - complex nested paths
- `.[?field=value]` - filter array elements where field equals value

Examples:

```toml
# GitHub releases API format
version_json_path = ".[].tag_name"

# Nested versions array
version_json_path = ".data.versions[]"

# Release info objects
version_json_path = ".releases[].info.version"

# Filter for stable releases only (e.g., Flutter)
version_json_path = ".releases[?channel=stable].version"
```

The filter syntax `[?field=value]` allows filtering JSON arrays before extraction. This is useful for APIs that return multiple release channels (stable, beta, dev) and you only want specific ones.

### `version_expr`

Extract versions using an [expr-lang](https://expr-lang.org/) expression. This provides the most flexibility for complex version extraction logic:

```toml
[tools."http:my-tool"]
version = "latest"
url = "https://example.com/releases/my-tool-v{{ version }}.tar.gz"
version_list_url = "https://example.com/versions.txt"
version_expr = 'split(body, "\n")'
```

The expression receives the HTTP response body as the `body` variable and should return an array of version strings.

Example expressions:

```toml
# Split newline-separated versions
version_expr = 'split(body, "\n")'

# Split and filter empty lines
version_expr = 'filter(split(body, "\n"), # != "")'

# Parse JSON and extract object keys (useful for HashiCorp-style JSON)
# e.g., {"versions": {"1.0.0": {}, "2.0.0": {}}}
version_expr = 'keys(fromJSON(body).versions)'
```

The [expr-lang](https://expr-lang.org/) library provides built-in functions including:

- **`fromJSON(string)`**: Parse a JSON string into a value
- **`toJSON(value)`**: Convert a value to a JSON string
- **`keys(map)`**: Get the keys of an object/map as an array
- **`values(map)`**: Get the values of an object/map as an array
- **`len(value)`**: Get the length of a string, array, or map

::: tip
`version_expr` takes precedence over `version_regex` and `version_json_path` if multiple are specified. Use it when the other options aren't flexible enough for your use case.
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
