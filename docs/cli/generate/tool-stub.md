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

### `-p --platform… <PLATFORM>`

Platform-specific URLs in the format platform:url

Examples: --platform linux-x64:https://... --platform darwin-arm64:https://...

### `-b --bin <BIN>`

Binary path within the extracted archive

If not specified, will attempt to auto-detect the binary

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
    --platform linux-x64:https://github.com/BurntSushi/ripgrep/releases/download/14.0.3/ripgrep-14.0.3-x86_64-unknown-linux-musl.tar.gz \
    --platform darwin-arm64:https://github.com/BurntSushi/ripgrep/releases/download/14.0.3/ripgrep-14.0.3-aarch64-apple-darwin.tar.gz

Generate without downloading (faster):
$ mise generate tool-stub ./bin/tool --url "https://example.com/tool.tar.gz" --skip-download
```
