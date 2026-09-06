# HTTP Backend

The `http` backend installs a binary, script, or archive from a direct download
URL. Use it when the publisher has no supported release backend or when you host
your own artifacts. Prefer HTTPS URLs and record an expected checksum.

The code for this is inside the mise repository at [`./src/backend/http.rs`](https://github.com/jdx/mise/blob/main/src/backend/http.rs).

## Usage

Replace the example URL with an artifact for your platform, then install it in
the current project:

```sh
mise use 'http:my-tool[url=https://example.com/releases/my-tool-v1.0.0.tar.gz]@1.0.0'
mise exec -- my-tool --version
```

This records the URL and version in `mise.toml`. Add `-g` for a global tool:

```toml
[tools]
"http:my-tool" = { version = "1.0.0", url = "https://example.com/releases/my-tool-v1.0.0.tar.gz" }
```

A fixed URL needs a concrete version label. `latest` does not discover releases
from a download URL: add [`version_list_url`](/dev-tools/backends/http.html#version-list-url) to enable
`mise ls-remote` and automatic version selection. Updating the version alone does
not change a static URL; use a URL template for versioned artifacts.

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
  version = "0.26.3",
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

If you slip and use something like `darwin-aarch64`, mise will try to figure out what
you meant and do the right thing anyway.
:::

### `checksum`

Provide the full expected digest from a trusted source. The following value
is a placeholder and must be replaced before installation:

```toml
[tools."http:my-tool"]
version = "1.0.0"
url = "https://example.com/releases/my-tool-v1.0.0.tar.gz"
checksum = "sha256:REPLACE_WITH_THE_64_HEX_DIGIT_DIGEST"
```

_Instead of specifying the checksum here, you can use [mise.lock](/dev-tools/mise-lock) to manage checksums._

### Platform-specific Checksums

Replace each placeholder with the digest for that platform's artifact:

```toml
[tools."http:my-tool"]
version = "1.0.0"

[tools."http:my-tool".platforms]
macos-x64 = {
  url = "https://example.com/releases/my-tool-v1.0.0-macos-x64.tar.gz",
  checksum = "sha256:REPLACE_WITH_THE_64_HEX_DIGIT_DIGEST",
}
macos-arm64 = {
  url = "https://example.com/releases/my-tool-v1.0.0-macos-arm64.tar.gz",
  checksum = "sha256:REPLACE_WITH_THE_64_HEX_DIGIT_DIGEST",
}
linux-x64 = {
  url = "https://example.com/releases/my-tool-v1.0.0-linux-x64.tar.gz",
  checksum = "sha256:REPLACE_WITH_THE_64_HEX_DIGIT_DIGEST",
}
```

### `checksum_url`

URL of a published checksum source. When set, [`mise lock`](/dev-tools/mise-lock)
resolves checksums for every target platform — including platforms other than
the one you are running on — **without downloading the artifacts**. This lets a
single machine produce a complete, cross-platform lockfile.

`checksum_url` is a template (it supports <code v-pre>{{ version }}</code>, <code v-pre>{{ os() }}</code>, and <code v-pre>{{ arch() }}</code>,
and can be set per platform via `platforms.<key>.checksum_url`). It may point at any
of:

- an **individual checksum file** (e.g. `<artifact>.sha256`), which may contain
  just the hash or `<hash>  <filename>`;
- a **SHASUMS**-style file listing `<hash>  <filename>` for many platforms (the row
  is matched by the artifact's filename);
- a **manifest** (e.g. JSON), combined with `checksum_expr` below.

For individual and SHASUMS checksum files, the algorithm is detected from the
file's name (`*.sha512`, `SHA512SUMS`, `*.md5`, `*.b3`, defaulting to sha256).

```toml
# Individual checksum file (one per artifact)
[tools."http:my-tool"]
version = "1.0.0"
url = "https://example.com/releases/my-tool-{{ version }}-{{ os() }}-{{ arch() }}.tar.gz"
checksum_url = "https://example.com/releases/my-tool-{{ version }}-{{ os() }}-{{ arch() }}.tar.gz.sha256"

# SHASUMS (one file lists every platform)
[tools."http:other-tool"]
version = "1.0.0"
url = 'https://example.com/{{ version }}/other_{{ version }}_{{ os(macos="darwin") }}_{{ arch(x64="amd64") }}.zip'
checksum_url = 'https://example.com/{{ version }}/other_{{ version }}_SHASUMS'
```

### `checksum_expr`

When the checksum lives in a manifest (rather than a plain checksum file), use
`checksum_expr` to extract it. The manifest body fetched from `checksum_url` is
evaluated with [expr-lang](https://expr-lang.org). The following variables are
available: `body` (the raw manifest), `version`, `os`, `arch`, `url` (the
resolved artifact URL for the target), and `filename`.

The expression must evaluate to a qualified `algo:hash` **string** (e.g.
`sha256:<hash>`, `sha512:<hash>`). Build the prefix in the expression: prepend a
literal when the algorithm is fixed (`"sha256:" + entry.hash`), or read it from
the manifest when it varies (`entry.algo + ":" + entry.hash`).

```toml
[tools."http:my-tool"]
version = "1.10.0"
checksum_url = "https://example.com/versions.json"
# Match the file whose url equals the resolved artifact url, return sha256:<hash>
checksum_expr = '"sha256:" + filter(fromJSON(body)[version + ""].files, { #.url == url })[0].sha256'

[tools."http:my-tool".platforms]
linux-x64 = { url = "https://example.com/my-tool-{{ version }}-linux-x86_64.tar.gz" }
macos-arm64 = { url = "https://example.com/my-tool-{{ version }}-macos-arm64.tar.gz" }
```

::: tip expr-lang gotchas
The predicate placeholder must be written as `{ #... }` **with a space** after
`{`, because `{#` is the Tera comment delimiter. To index a map by a runtime
value, force evaluation with `[version + ""]` — a bare `[version]` is treated as
the literal key `"version"`.
:::

### `size`

Check the expected byte count. These numbers illustrate the syntax; use the
actual artifact sizes. A size check does not replace a checksum:

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
When both `strip_components` and `bin_path` are unset, mise automatically applies `strip_components = 1` when the extracted archive contains exactly one directory at the root level and no files. This is common with tools like ripgrep that package their binaries in a versioned directory (e.g., `ripgrep-14.1.0-x86_64-unknown-linux-musl/rg`). Auto-detection ensures the binary is placed directly in the install path where mise expects it.
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
When downloading single binaries (not archives), mise automatically removes OS/arch suffixes from the filename. For example, `docker-compose-linux-x86_64` becomes `docker-compose`. Use the `bin` option only when you need a specific custom name.
:::

### `rename_exe`

Rename the executable inside an extracted archive to a specific name. This is useful when archives contain binaries with platform-specific names or when installing kubectl plugins that need specific naming:

```toml
[tools."http:openunison-cli"]
version = "1.0.0"
url = "https://nexus.tremolo.io/repository/openunison-cli/openunison-cli-v{{version}}-linux.zip"
rename_exe = "kubectl-openunison-cli"  # Rename extracted binary for kubectl plugin
```

mise searches for the first executable in the extracted directory (or in `bin_path` if specified) and renames it to the given name.

To rename **multiple** binaries from one archive, use the table form — each key is a source name (an exact file name or a glob) and each value is the new name:

```toml
[tools."http:mytool"]
version = "1.0.0"
url = "https://example.com/mytool-v{{version}}-linux.zip"
rename_exe = { "mytool-*" = "mytool", "myhelper-*" = "myhelper" }
```

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
If `format` is not specified, mise automatically detects the format from the final URL after HTTP redirects, falling back to the configured URL. This allows an extensionless download endpoint to redirect to an archive such as `.tar.gz`. An explicit `format` always takes precedence, so use it when neither URL has a useful extension or when you need to override the detected format.
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

Fetch available versions from a remote URL. This lets `mise ls-remote` list versions for HTTP-based tools:

```toml
[tools."http:my-tool"]
version = "1.0.0"
url = "https://example.com/releases/my-tool-v{{version}}.tar.gz"
version_list_url = "https://example.com/releases/versions.txt"
```

The version list URL can return data in any of these formats:

- **Plain text**: A single version number (e.g., `2.0.53`)
- **Line-separated**: One version per line
- **JSON array of strings**: `["1.0.0", "1.1.0", "2.0.0"]`
- **JSON array of objects**: `[{"version": "1.0.0"}, {"tag_name": "v2.0.0"}]`
- **JSON object with versions array**: `{"versions": ["1.0.0", "2.0.0"]}`

Version prefixes like `v` are automatically stripped.

mise preserves the order returned by the version source. By default, version
resolution treats the last matching entry as the latest one. If the source
returns semantic versions in another order, set `version_order = "semver"` to
order `mise ls-remote` and select versions by semantic precedence. See
[Version ordering](/dev-tools/#version-ordering) for the complete ordering
contract.

GitHub's releases API, for example, returns releases newest first. Opt into
semantic ordering when using it as an HTTP version source:

```toml
[tools."http:my-tool"]
version = "latest"
version_order = "semver"
url = "https://example.com/my-tool-{{ version }}.tar.gz"
version_list_url = "https://api.github.com/repos/owner/my-tool/releases"
version_json_path = ".[].tag_name"
```

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
```

```toml
# Nested versions array
version_json_path = ".data.versions[]"
```

```toml
# Release info objects
version_json_path = ".releases[].info.version"
```

```toml
# Filter for stable releases only (e.g., Flutter)
version_json_path = ".releases[?channel=stable].version"
```

The filter syntax `[?field=value]` filters JSON arrays before extraction. This is useful for APIs that return multiple release channels (stable, beta, dev) when you only want one of them.

### `version_expr`

Extract versions using an [expr-lang](https://expr-lang.org/) expression. This is the most flexible option for complex version extraction:

```toml
[tools."http:my-tool"]
version = "latest"
url = "https://example.com/releases/my-tool-v{{ version }}.tar.gz"
version_list_url = "https://example.com/versions.txt"
version_expr = 'split(body, "\n")'
```

The expression receives the HTTP response body as the `body` variable and
should return an array of version strings. It also receives `versions`, which
contains values already extracted by `version_regex` or `version_json_path` and
is empty when no earlier extractor produced values.

Example expressions:

```toml
# Split newline-separated versions
version_expr = 'split(body, "\n")'
```

```toml
# Split and filter empty lines
version_expr = 'filter(split(body, "\n"), # != "")'
```

```toml
# Parse JSON and extract object keys (useful for HashiCorp-style JSON)
# e.g., {"versions": {"1.0.0": {}, "2.0.0": {}}}
version_expr = 'keys(fromJSON(body).versions)'
```

```toml
# Sort versions with mise's version-aware comparator
version_expr = 'fromJSON(body) | map({ trimPrefix(#.tag_name, "v") }) | sortVersions()'
```

The [expr-lang](https://expr-lang.org/) library provides built-in functions
including:

- **`fromJSON(string)`**: Parse a JSON string into a value
- **`toJSON(value)`**: Convert a value to a JSON string
- **`keys(map)`**: Get the keys of an object/map as an array
- **`values(map)`**: Get the values of an object/map as an array
- **`len(value)`**: Get the length of a string, array, or map
- **`filter(array, predicate)`** and **`map(array, predicate)`**: Filter or transform array values
- **`sort(array)`** and **`reverse(array)`**: Reorder values lexically
- **`int(value)`**, **`float(value)`**, and **`string(value)`**: Convert compatible values

mise adds **`sortVersions(array)`** for version-aware ordering. Prefer
`version_order = "semver"` when the discovered versions follow semantic
versioning; use `sortVersions()` when the expression itself needs a sorted
intermediate value.

::: tip
`version_expr` is the final extraction step, so its result becomes the version
list. Use the `versions` variable to post-process values produced by
`version_regex` or `version_json_path`.
:::

### `bin_path`

Use paths relative to the extracted install root. Setting `bin_path` disables
automatic root stripping. For an archive shaped like
`my-tool-1.0.0/bin/my-tool`, explicitly strip the outer directory as below, or
leave it in place and use `bin_path = "my-tool-{{ version }}/bin"`.

Specify the directory containing binaries within the extracted archive, or where to place the downloaded file. This supports templating with <code v-pre>{{version}}</code>:

```toml
[tools."http:my-tool"]
version = "1.0.0"
url = "https://example.com/releases/my-tool-v1.0.0.tar.gz"
strip_components = 1
bin_path = "bin"
```

**Binary path lookup order:**

1. If `bin_path` is specified, use that directory
2. If `bin_path` is not set, look for a `bin/` directory in the install path
3. If no `bin/` directory exists, search subdirectories for `bin/` directories
4. If no `bin/` directories are found, use the root of the extracted directory

## Caching Behavior

The HTTP backend caches downloads to save disk space and speed up installation:

### Cache Location

For normal user installations, downloaded and extracted files are cached in
`$MISE_DATA_DIR/http-tarballs/` instead of being stored separately for each tool
installation. By default:

- **Linux**: `~/.local/share/mise/http-tarballs/`
- **macOS**: `~/.local/share/mise/http-tarballs/`

Explicit `mise install --system`, `mise install --shared`, and `mise
install-into` installations are extracted directly into their destination.
They do not use this persistent extraction cache, so the resulting installation
is self-contained and does not link to the installing user's home directory.

### Cache Key Generation

Cache keys are derived from the file content so that identical downloads are shared across tools:

1. **File content**: mise calculates a Blake3 hash of the downloaded file, independently of its expected verification checksum.
2. **Extraction options**: options that change the extracted result, including root stripping, renaming, and relevant format or launcher choices, also affect the key.

Example cache directory structure:

```
~/.local/share/mise/http-tarballs/
├── 71f774faa03daf1a58cc3339f8c73e6557348c8e0a2f3fb8148cc26e26bad83f/
│   ├── bin/my-tool
│   └── metadata.json
└── 1c2af379bdf1fed266bc44b49271e2df5b0dafae09f1cc744b3505ec50c84719_strip_1/
    ├── my-tool
    └── metadata.json
```

### Symlinked Installations

Normal user installations are symlinks to the cached extracted content:

```bash
~/.local/share/mise/installs/http-my-tool/1.0.0 → ~/.local/share/mise/http-tarballs/71f774...
```

This approach provides several benefits:

- **Space efficiency**: Normal user installs share identical tarballs across tools
- **Faster installations**: Reusing extracted content avoids repeated extraction; a download may still be needed to identify its content
- **Consistency**: The same file and extraction options reuse the same cached content

System, shared, and install-into destinations contain real files rather than
these symlinks. This avoids leaving hidden cache entries behind after an
uninstall and keeps shared installations independent of a specific user's data
directory.

### Cache Metadata

Each cache entry includes a `metadata.json` file with information about the cached content:

```json
{
  "url": "https://example.com/releases/my-tool-v1.0.0.tar.gz",
  "checksum": "sha256:REPLACE_WITH_THE_64_HEX_DIGIT_DIGEST",
  "size": 1024000,
  "extracted_at": 1703001234,
  "platform": "macos-arm64"
}
```

### Cache Management

Normal HTTP installations store their cache in
`$MISE_DATA_DIR/http-tarballs/`. It is intentionally outside `MISE_CACHE_DIR`,
so `mise cache clear` does not remove content that installed symlinks still
reference.

System, shared, and install-into destinations do not create a persistent HTTP
extraction cache.
