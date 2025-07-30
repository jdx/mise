# Tool Stubs

Tool stubs allow you to create executable files with embedded TOML configuration for tool execution. They provide a convenient way to define tool versions, backends, and execution parameters directly within executable scripts. They are also a good way to have some tools in mise lazy-load since the tools are only fetched when called and not when calling something like `mise install`.

This feature is inspired by [dotslash](https://github.com/facebook/dotslash), which pioneered the concept of executable files with embedded configuration for portable tool execution.

## Overview

A tool stub is an executable file that begins with a shebang line pointing to `mise tool-stub` and contains TOML configuration specifying which tool to execute and how to execute it. When the stub is run, mise automatically installs the specified tool version (if needed) and executes it with the provided arguments.

Tool stubs can use any mise backend but because they default to http—and http backend tools have things like urls and don't require a version—the http stubs look a bit different than non-http stubs.

::: tip
Tool stubs are particularly useful for adding less-commonly used tools to your mise setup. Since tools are only installed when their stub is first executed, you can define many tools without the overhead of installing them all upfront. This is perfect for specialized tools, testing utilities, or project-specific binaries that you might not use every day.
:::

## Tool (non-http) Stubs

```bash
#!/usr/bin/env -S mise tool-stub
# Optional comment describing the tool

version = "1.0.0"
tool = "python"
bin = "python"
```

::: info Why use `env -S`?
The `-S` flag tells `env` to split the command line on spaces, allowing multiple arguments to be passed to the interpreter. This is necessary because shebangs on Unix systems traditionally only support a single argument after the interpreter path. Using `env -S mise tool-stub` allows the shebang to work correctly by splitting it into `env` → `mise` → `tool-stub`.
:::

## Configuration Fields

Tool stub configuration is essentially a subset of what can be done in `mise.toml` [tools] sections, with the addition of a `tool` field to specify which tool to use. All the same options available for tool configuration in `mise.toml` are supported in tool stubs.

### Optional Fields

- `tool` - Explicit tool name or backend specification (e.g., "python", "github:cli/cli"). This is the only field unique to tool stubs - it specifies which tool entry from the configuration to use. If omitted and a `url` field is present, defaults to the HTTP backend.
- `version` - The version of the tool to use
- `bin` - The binary name to execute within the tool (defaults to the stub filename)

## HTTP Stubs

For multi-platform tarballs:

```toml
#!/usr/bin/env -S mise tool-stub
url = "https://example.com/releases/1.0.0/tool-linux-x64.tar.gz"
```

For platform-specific tarballs:

```toml
#!/usr/bin/env -S mise tool-stub
[platforms.linux-x64]
url = "https://example.com/releases/1.0.0/tool-linux-x64.tar.gz"

[platforms.darwin-arm64]
url = "https://example.com/releases/1.0.0/tool-macos-arm64.tar.gz"
```

### Platform-Specific Binary Paths

Different platforms may have different binary structures or names. You can specify platform-specific `bin` fields when the binary path differs between platforms:

```toml
#!/usr/bin/env -S mise tool-stub
# Global bin field used when platforms have the same structure
bin = "bin/tool"

[platforms.linux-x64]
url = "https://example.com/tool-linux.tar.gz"
# Uses global bin field: "bin/tool"

[platforms.windows-x64]
url = "https://example.com/tool-windows.zip"
bin = "tool.exe"  # Platform-specific binary for Windows
```

The tool stub generator automatically detects when platforms have different binary paths and will generate platform-specific `bin` fields when needed, or use a global `bin` field when all platforms have the same binary structure.

::: tip
tool stubs default to the HTTP backend if no `tool` field is specified and a `url` field is present.
See the [HTTP backend documentation](/dev-tools/backends/http) for full details on configuring HTTP-based tools.
:::

## Generating Tool Stubs (http)

While you can manually create tool stubs with TOML configuration, mise provides a [`mise generate tool-stub`](/cli/generate/tool-stub) command to automatically create stubs for HTTP-based tools.

::: tip Incremental Building
When using platform-specific URLs, the tool stub generator will append new platforms to existing stub files rather than overwriting them. This allows you to incrementally build cross-platform tool stubs by running the command multiple times with different platforms.
:::

### Basic Generation

Generate a tool stub for a tool distributed via HTTP:

```bash
mise generate tool-stub ./bin/gh --url "https://github.com/cli/cli/releases/download/v2.336.0/gh_2.336.0_linux_amd64.tar.gz"
```

This will:
- Download the archive to detect checksums (for security)
- Extract it to auto-detect the binary path
- Generate an executable stub with complete TOML configuration

### Platform-Specific Generation

For tools with different URLs per platform, you can generate all platforms at once:

```bash
mise generate tool-stub ./bin/rg \
  --platform-url linux-x64:https://github.com/BurntSushi/ripgrep/releases/download/14.0.3/ripgrep-14.0.3-x86_64-unknown-linux-musl.tar.gz \
  --platform-url darwin-arm64:https://github.com/BurntSushi/ripgrep/releases/download/14.0.3/ripgrep-14.0.3-aarch64-apple-darwin.tar.gz
```

**Auto-Platform Detection**: If the URL contains platform information, you can omit the platform prefix and let mise auto-detect it:

```bash
# Auto-detect platform from URL (detects as 'macos-arm64')
mise generate tool-stub ./bin/node \
  --platform-url https://nodejs.org/dist/v22.17.1/node-v22.17.1-darwin-arm64.tar.gz

# Auto-detect platform from URL (detects as 'linux-x64')
mise generate tool-stub ./bin/node \
  --platform-url https://github.com/BurntSushi/ripgrep/releases/download/14.0.3/ripgrep-14.0.3-x86_64-unknown-linux-musl.tar.gz
```

Or build them incrementally by adding platforms one at a time:

```bash
# Start with Linux support (explicit platform)
mise generate tool-stub ./bin/rg \
  --platform-url linux-x64:https://github.com/BurntSushi/ripgrep/releases/download/14.0.3/ripgrep-14.0.3-x86_64-unknown-linux-musl.tar.gz

# Later, add macOS support using auto-detection (appends to existing file)
mise generate tool-stub ./bin/rg \
  --platform-url https://github.com/BurntSushi/ripgrep/releases/download/14.0.3/ripgrep-14.0.3-aarch64-apple-darwin.tar.gz

# Add Windows support using auto-detection (appends to existing file)
mise generate tool-stub ./bin/rg \
  --platform-url https://github.com/BurntSushi/ripgrep/releases/download/14.0.3/ripgrep-14.0.3-x86_64-pc-windows-msvc.zip
```

The generator will preserve existing configuration and merge new platforms into the `[platforms]` table. If you specify a platform that already exists, its URL will be updated.

### Generation Options

- `--version VERSION` - Specify tool version (defaults to "latest").
- `--bin PATH` - Override auto-detected binary path
- `--platform-url PLATFORM:URL` - Add platform-specific URL (can be used multiple times)
- `--platform-url URL` - Add platform-specific URL with auto-detected platform from URL filename
- `--platform-bin PLATFORM:PATH` - Set platform-specific binary path
- `--skip-download` - Skip downloading for faster generation (no checksums or binary detection)

### Supported Archive Formats

The generator automatically detects and extracts various archive formats:
- `.tar.gz` / `.tgz` (gzip compressed tarballs)
- `.tar.xz` / `.txz` (xz compressed tarballs)
- `.tar.bz2` / `.tbz2` (bzip2 compressed tarballs)
- `.tar.zst` / `.tzst` (zstd compressed tarballs)
- `.zip` (zip archives)
- `.7z` (7-zip archives, Windows only)

### Generated Stub Example

Running the generation command produces an executable stub like:

```bash
#!/usr/bin/env -S mise tool-stub

version = "latest"
bin = "bin/gh"
url = "https://github.com/cli/cli/releases/download/v2.336.0/gh_2.336.0_linux_amd64.tar.gz"
checksum = "blake3:a1b2c3d4e5f6..."
size = 12345678
```

The generator automatically:
- Calculates BLAKE3 checksums for integrity verification
- Detects file sizes
- Identifies the correct binary path within archives
- Uses the output filename as the tool name

## Examples

### Basic Node.js Stub

```bash
#!/usr/bin/env -S mise tool-stub
# Node.js v20 tool stub

tool = "node"
version = "20.0.0"
bin = "node"
```

### Python with Custom Binary Name

```bash
#!/usr/bin/env -S mise tool-stub
# Python tool accessible as 'py'

tool = "python"
version = "3.11"
bin = "python"
```

### GitHub Release Backend

```bash
#!/usr/bin/env -S mise tool-stub
# GitHub CLI tool

tool = "github:cli/cli"
version = "latest"
```

### HTTP Backend with Platform Support

```bash
#!/usr/bin/env -S mise tool-stub
# Custom HTTP tool with platform-specific downloads

version = "1.0.0"

[platforms.linux-x64]
url = "https://releases.example.com/v{{version}}/tool-linux-x64.tar.gz"

[platforms.darwin-arm64]
url = "https://releases.example.com/v{{version}}/tool-macos-arm64.tar.gz"
```

## Usage

### Direct Execution

Make the stub executable and run it directly:

```bash
chmod +x ./bin/my-tool
./bin/my-tool --version
```

### Via mise Command

Execute using the [`mise tool-stub`](/cli/tool-stub) command—useful for testing if something isn't working right:

```bash
mise tool-stub ./bin/my-tool --version
```

## Caching

Tool stubs implement intelligent caching which reduces the overhead mise has when running stubs:

- Binary paths are cached based on stub file path and modification time
- Cache is automatically invalidated when the stub file changes
- Missing binaries trigger cache cleanup automatically

Cached stubs have ~4ms of overhead.

## Alternative: Creating Simple Stubs with `mise x`

For basic use cases, you can quickly create simple tool stubs using the [`mise x`](/cli/exec) command as an alternative to writing TOML configuration manually:

```bash
# Create bin directory
mkdir -p ./bin

# Create a simple Node.js stub
cat > ./bin/node << 'EOF'
#!/usr/bin/env bash
exec mise x node@20 -- "$@"
EOF
chmod +x ./bin/node

# Create a Python stub with specific version
cat > ./bin/python << 'EOF'
#!/usr/bin/env bash
exec mise x python@3.11 -- "$@"
EOF
chmod +x ./bin/python
```

This approach is ideal for simple tool execution without the need for custom options, environment variables, or platform-specific settings. For more complex configurations, use the full TOML configuration format described above.
