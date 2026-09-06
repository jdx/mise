# Tool Stubs

A tool stub is an executable file that records how to obtain and run one tool.
Commit it to a repository so a command such as `./bin/py` selects the intended
runtime and forwards its arguments. mise installs the tool on first execution;
a normal project `mise install` does not discover and install arbitrary stub files.

This feature is inspired by [dotslash](https://github.com/facebook/dotslash), which pioneered the concept of executable files with embedded configuration for portable tool execution.

## Overview

A tool stub is an executable file that begins with a shebang line pointing to `mise tool-stub` and contains TOML configuration specifying which tool to execute and how to execute it. When the stub runs, mise installs the specified tool version (if needed) and executes it with the provided arguments.

A stub requires `mise` on `PATH`, unless generated with the optional bootstrap
wrapper. It can name a backend or provide HTTP download URLs. A backend stub
uses that backend's version resolution; an HTTP stub records the artifact location
directly.

For a machine-wide catalogue of ordinary tools, prefer
[`lazy = true` in `[tools]`](/dev-tools/shims.html#lazy-tools). Standalone tool
stub scripts remain useful when the executable file itself should carry a
portable, self-contained tool definition.

## Tool (non-http) Stubs

Create a `bin` directory, then save this file as `bin/py`:

```toml [bin/py]
#!/usr/bin/env -S mise tool-stub

tool = "python"
version = "3.14"
bin = "python"
```

On Unix, make it executable and run it:

```sh
chmod +x ./bin/py
./bin/py --version
./bin/py -c 'import sys; print(sys.executable)'
```

The stub's name is `py`, but it runs the installed `python` executable. Arguments
following the stub path are forwarded to Python. For an exact runtime, use a
concrete version or [lock the stub](#locking-a-stub).

::: info Why use `env -S`?
The `-S` flag tells `env` to split the command line on spaces, so multiple arguments can be passed to the interpreter. This is necessary because shebangs on Unix systems traditionally support only a single argument after the interpreter path. `env -S mise tool-stub` makes the shebang work by splitting it into `env` → `mise` → `tool-stub`.
:::

## Configuration Fields

A stub contains a tool declaration, not an entire `mise.toml`. Put fields at the
top level; do not wrap them in `[tools]`. Backend-specific installation options
are passed to the selected backend, while `tool`, `version`, `bin`, `os`,
`install_env`, and embedded `lock` data control the stub itself.

### Optional Fields

- `tool` - Explicit tool name or backend specification (e.g., "python", "github:cli/cli"). If omitted, a top-level or platform-specific URL selects the HTTP backend; otherwise mise uses the stub filename as the tool name.
- `version` - The version request (defaults to `latest`)
- `bin` - The binary name to execute within the tool (defaults to the stub filename)

## HTTP Stubs

A top-level URL applies to every platform on which the stub is run. Use it only
for an artifact compatible with all of those machines. The URLs below are
placeholders; use the generator to record real download metadata:

```toml
#!/usr/bin/env -S mise tool-stub
url = "https://example.com/releases/1.0.0/tool.tar.gz"
```

For OS- or architecture-specific binaries, provide a platform table instead:

```toml
#!/usr/bin/env -S mise tool-stub
[platforms.linux-x64]
url = "https://example.com/releases/1.0.0/tool-linux-x64.tar.gz"

[platforms.macos-arm64]
url = "https://example.com/releases/1.0.0/tool-macos-arm64.tar.gz"
```

### Platform-Specific Binary Paths

Set `bin` relative to the installed directory, after any archive root directory
has been stripped. The generator accounts for that extraction layout. An explicit
`--bin` must name the path that will remain after extraction.

Use platform-specific `bin` fields when layouts or executable names differ:

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

The tool stub generator detects when platforms have different binary paths and generates platform-specific `bin` fields when needed, or a single global `bin` field when all platforms share the same binary structure.

::: tip
Tool stubs default to the HTTP backend when download URLs are present and no
`tool` field selects another backend.
See the [HTTP backend documentation](/dev-tools/backends/http) for full details on configuring HTTP-based tools.
:::

## Generating Tool Stubs (http)

While you can create tool stubs manually, mise provides a [`mise generate tool-stub`](/cli/generate/tool-stub) command that generates stubs for HTTP-based tools.

::: tip Incremental Building
When using platform-specific URLs, the tool stub generator appends new platforms to existing stub files rather than overwriting them. This lets you build cross-platform tool stubs incrementally by running the command multiple times with different platforms.
:::

### Basic Generation

Generate a tool stub for a tool distributed via HTTP:

```bash
mise generate tool-stub ./bin/gh --url "https://github.com/cli/cli/releases/download/v2.96.0/gh_2.96.0_linux_amd64.tar.gz"
```

This will:

- Download the archive and record a checksum of those bytes
- Extract it to auto-detect the binary path
- Generate an executable stub with download and execution metadata

A generated checksum detects later changes to the artifact. It does not by itself
authenticate the publisher of the initial download. Review the URL and obtain it
from a source you trust before committing the stub.

### Platform-Specific Generation

For tools with different URLs per platform, you can generate all platforms at once:

```bash
mise generate tool-stub ./bin/rg \
  --platform-url linux-x64:https://github.com/BurntSushi/ripgrep/releases/download/14.0.3/ripgrep-14.0.3-x86_64-unknown-linux-musl.tar.gz \
  --platform-url macos-arm64:https://github.com/BurntSushi/ripgrep/releases/download/14.0.3/ripgrep-14.0.3-aarch64-apple-darwin.tar.gz
```

**Auto-Platform Detection**: If the URL contains platform information, you can omit the platform prefix and let mise auto-detect it:

```bash
# Auto-detect platform from URL (detects as 'macos-arm64')
mise generate tool-stub ./bin/node \
  --platform-url https://nodejs.org/dist/v22.17.1/node-v22.17.1-darwin-arm64.tar.gz

# Auto-detect platform from URL (detects as 'linux-x64')
mise generate tool-stub ./bin/node \
  --platform-url https://nodejs.org/dist/v22.17.1/node-v22.17.1-linux-x64.tar.gz
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

The generator merges new platforms into the existing `[platforms]` table.
Re-specifying a platform updates that entry, so inspect the diff before committing.
Use explicit platform prefixes when a filename is ambiguous.

### Generation Options

- `--version VERSION` - Specify the tool version (defaults to "latest")
- `--bin PATH` - Override the auto-detected binary path
- `--platform-url PLATFORM:URL` - Add a platform-specific URL (can be repeated)
- `--platform-url URL` - Add a platform-specific URL, auto-detecting the platform from the URL filename
- `--platform-bin PLATFORM:PATH` - Set a platform-specific binary path
- `--checksum-algorithm ALGORITHM` - Generate `blake3` (default) or `sha256` checksums
- `--skip-download` - Generate without checksums or binary detection; review the binary path and run `--fetch` before relying on integrity checks
- `--lock` - Resolve and embed lockfile data (pinned version + platform URLs/checksums) into an existing stub
- `--fetch` - Fetch missing checksums and sizes for an existing stub file

`--checksum-algorithm` cannot be combined with `--lock` or `--skip-download`, because those modes do not calculate checksums.

For consumers such as Bazel that require SHA256 checksums, select that algorithm when generating the stub:

```bash
mise generate tool-stub ./bin/tool \
  --url "https://example.com/tool.tar.gz" \
  --checksum-algorithm sha256
```

The selected algorithm also applies to missing checksums populated by `--fetch`. Existing checksums are preserved.

### Supported Archive Formats

The generator automatically detects and extracts various archive formats:

- `.tar.gz` / `.tgz` (gzip compressed tarballs)
- `.tar.xz` / `.txz` (xz compressed tarballs)
- `.tar.bz2` / `.tbz2` (bzip2 compressed tarballs)
- `.tar.zst` / `.tzst` (zstd compressed tarballs)
- `.zip` (zip archives)
- `.7z` (7-zip archives)

### Generated Stub Example

Inspect the generated file rather than entering a checksum or size by hand:

```sh
cat ./bin/gh
```

The file contains the URL, executable path, checksum, and size discovered from
the archive. `version` may be omitted when it has the default value `latest`.
For an HTTP stub, that label does not make a versioned URL track newer releases;
update the URL and regenerate its metadata when upgrading.

The output filename becomes the tool name. Set `--bin` if auto-detection selects
the wrong executable, especially when an archive contains several commands.

## Examples

### Basic Node.js Stub

```toml
#!/usr/bin/env -S mise tool-stub
# Node.js tool stub

tool = "node"
version = "24"
bin = "node"
```

### Python with Custom Binary Name

```toml
#!/usr/bin/env -S mise tool-stub
# Python tool accessible as 'py'

tool = "python"
version = "3.14"
bin = "python"
```

### GitHub Release Backend

```toml
#!/usr/bin/env -S mise tool-stub
# GitHub CLI tool

tool = "github:cli/cli"
version = "latest"
bin = "gh"
```

### Locked Tool Stub

Lock a backend stub to record a concrete version and the platform download
metadata its backend can provide. The generator writes this under `[lock]`;
the top-level `tool` still selects the backend, and `version` becomes the
resolved version.

Stored URLs can avoid release discovery on later installs. They do not remove
private-download authentication or every backend's verification and policy
requests. Review the generated platforms and checksums; a backend that cannot
provide a URL cannot supply the same download shortcut.

#### Locking a Stub

```bash
# Create a stub with a fuzzy version
mise generate tool-stub ./bin/node --version 24

# Lock it to pin the exact version and add platform URLs/checksums
mise generate tool-stub ./bin/node --lock
```

This resolves the version, fetches URLs for all common platforms (linux-x64, linux-arm64, macos-x64, macos-arm64, windows-x64), and writes them into a `[lock]` section in the stub.

#### Bumping a Locked Version

To bump the version of a locked stub, pass `--version` along with `--lock`:

```bash
# Select Node.js 26 and regenerate the locked metadata
mise generate tool-stub ./bin/node --lock --version 26
```

### HTTP Backend with Platform Support

```toml
#!/usr/bin/env -S mise tool-stub
# Custom HTTP tool with platform-specific downloads

version = "1.0.0"

[platforms.linux-x64]
url = "https://releases.example.com/v{{version}}/tool-linux-x64.tar.gz"

[platforms.macos-arm64]
url = "https://releases.example.com/v{{version}}/tool-macos-arm64.tar.gz"
```

## Usage

### Direct Execution

Make the stub executable and run it directly:

```bash
chmod +x ./bin/my-tool
./bin/my-tool --version
```

#### On Windows

Windows cannot execute a shebang script, so `mise generate tool-stub` writes a `.cmd` launcher
beside the stub. Run the stub by name and Windows picks it up through `PATHEXT`:

```powershell
.\bin\my-tool.cmd --version
```

The launcher is generated whenever the stub could run on Windows — either it lists a
`[platforms.windows-*]` entry, or it names no platforms at all. A stub that ships only for, say,
Linux and macOS does not get one, and neither does a stub whose own name already ends in `.cmd`,
`.bat` or `.exe`. The launcher is written on every platform, not just Windows, so a stub generated on Linux
and committed to a repository still works for someone who clones it on Windows.

If a stub later stops shipping for Windows, regenerating it removes the launcher, so it cannot keep
running against platforms the stub no longer declares. Only a launcher mise generated is removed —
one you wrote yourself is left alone.

The extension-less stub is kept as well: Git Bash and Cygwin run it through the shebang, the same
way [shims](/dev-tools/shims) place both an extension-less script and a native launcher on Windows.

### Via mise Command

Run the stub through the [`mise tool-stub`](/cli/tool-stub) command—useful for debugging when something isn't working:

```bash
mise tool-stub ./bin/my-tool --version
```

## Caching

Tool stubs cache lookups to reduce the overhead mise adds when running them:

- Binary paths are cached based on the stub file path and modification time
- The cache is invalidated automatically when the stub file changes
- Missing binaries trigger cache cleanup automatically

Each invocation still passes through mise. For repeated calls inside a script,
consider preparing the environment once with `mise exec` and calling the tool
directly from that script.

## Pruning

Executing a stub tracks it in `~/.local/state/mise/tracked-stubs`, the same way
config files are tracked when they are used. [`mise prune`](/cli/prune) treats
tool versions referenced by a tracked stub as needed and will not delete them,
just like versions required by a tracked config file.

A stub must have been executed at least once on the machine for its tool to be
protected. If the stub file is later deleted, its tool versions become prunable
again (unless something else needs them).

## Alternative: Creating Simple Stubs with `mise x`

For basic use cases, you can create simple stubs with the [`mise x`](/cli/exec) command instead of writing TOML configuration:

```bash
# Create bin directory
mkdir -p ./bin

# Create a simple Node.js stub
cat > ./bin/node << 'EOF'
#!/usr/bin/env bash
exec mise x node@24 -- node "$@"
EOF
chmod +x ./bin/node

# Create a Python stub with specific version
cat > ./bin/python << 'EOF'
#!/usr/bin/env bash
exec mise x python@3.14 -- python "$@"
EOF
chmod +x ./bin/python
```

The command after `--` is essential: `mise x node@24` selects the runtime,
and `node "$@"` names the executable and preserves the caller's arguments.
These wrappers require Bash and mise and do not embed artifact metadata. Use
the TOML stub format when you need platform mappings or embedded lock data.
