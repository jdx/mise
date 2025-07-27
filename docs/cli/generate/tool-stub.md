# `mise generate tool-stub`

- **Usage**: `mise generate tool-stub [FLAGS] <OUTPUT>`
- **Source code**: [`src/cli/generate/tool_stub.rs`](https://github.com/jdx/mise/blob/main/src/cli/generate/tool_stub.rs)

[experimental] Generate a tool stub for HTTP-based tools

This command generates tool stubs that can automatically download and execute
tools from HTTP URLs. It can detect checksums, file sizes, and binary paths
automatically by downloading and analyzing the tool.

## Arguments

### `<OUTPUT>`

Output file path for the tool stub

## Flags

### `--version <VERSION>`

Version of the tool

### `-u --url <URL>`

URL for downloading the tool

Example: <https://github.com/owner/repo/releases/download/v2.0.0/tool-linux-x64.tar.gz>

### `--platform-url… <PLATFORM_URL>`

Platform-specific URLs in the format platform:url

Examples: --platform-url linux-x64:https://... --platform-url darwin-arm64:https://...

### `--platform-bin… <PLATFORM_BIN>`

Platform-specific binary paths in the format platform:path

Examples: --platform-bin windows-x64:tool.exe --platform-bin linux-x64:bin/tool

### `-b --bin <BIN>`

Binary path within the extracted archive

If not specified and the archive is downloaded, will only auto-detect if an exact filename match is found

### `--skip-download`

Skip downloading for checksum and binary path detection (faster but less informative)

### `--http <HTTP>`

HTTP backend type to use

Examples:

```
Generate a tool stub for a single URL:
$ mise generate tool-stub ./bin/gh --url "https://github.com/cli/cli/releases/download/v2.336.0/gh_2.336.0_linux_amd64.tar.gz"

Generate a tool stub with platform-specific URLs:
$ mise generate tool-stub ./bin/rg \
    --platform-url linux-x64:https://github.com/BurntSushi/ripgrep/releases/download/14.0.3/ripgrep-14.0.3-x86_64-unknown-linux-musl.tar.gz \
    --platform-url darwin-arm64:https://github.com/BurntSushi/ripgrep/releases/download/14.0.3/ripgrep-14.0.3-aarch64-apple-darwin.tar.gz

Generate with platform-specific binary paths:
$ mise generate tool-stub ./bin/tool \
    --platform-url linux-x64:https://example.com/tool-linux.tar.gz \
    --platform-url windows-x64:https://example.com/tool-windows.zip \
    --platform-bin windows-x64:tool.exe

Generate without downloading (faster):
$ mise generate tool-stub ./bin/tool --url "https://example.com/tool.tar.gz" --skip-download
```
