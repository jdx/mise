# asdf (Legacy) Plugins

::: warning
asdf plugins are considered legacy. **New asdf and vfox plugins are not accepted into the [mise registry](https://github.com/jdx/mise/blob/main/registry/) for supply-chain security reasons** — for registry submissions use [packslip](/dev-tools/backends/packslip.html) (preferred when the project publishes packslips), [aqua](/dev-tools/backends/aqua.html), [github](/dev-tools/backends/github.html), or [gitlab](/dev-tools/backends/gitlab.html) instead.

If you are writing a private/custom plugin (not for registry submission), prefer [vfox plugins](/dev-tools/backends/vfox.html) over asdf — they're written in Lua, work cross-platform (including Windows), and have access to built-in modules. See the [feature comparison](/dev-tools/backends/asdf.html#feature-comparison-asdf-vs-vfox) and [hook migration table](/dev-tools/backends/asdf.html#hook-migration-asdf-to-vfox) for details.
:::

mise maintains compatibility with the asdf plugin ecosystem through its asdf backend. These plugins are considered legacy because they have limitations compared to mise's modern plugin system.

## What are asdf (Legacy) Plugins?

asdf plugins are shell script-based plugins that follow the asdf plugin specification. They were the original way to extend tool management in the asdf ecosystem and are now supported by mise for backward compatibility.

## Limitations

asdf plugins have several limitations compared to mise's modern plugin system:

- **Platform Support**: Require Unix shell utilities; the asdf backend is disabled by default on Windows
- **Performance**: Shell script execution is slower than mise's native backends
- **Features**: Limited compared to modern backends like aqua, github, or tool/backend plugins
- **Maintenance**: Harder to maintain and debug
- **Execution scope**: Plugin scripts run with your permissions. Lua plugins can also run
  commands and access files; neither plugin format is an OS sandbox.

## When to Use asdf (Legacy) Plugins

Only use asdf plugins when:

- The tool is not available through modern backends (aqua, github, etc.)
- You need compatibility with existing asdf workflows
- The tool requires complex shell-based installation logic that can't be handled by modern backends

**For new tools, consider these alternatives first:**

1. [packslip backend](dev-tools/backends/packslip.md) - Preferred for signed release manifests
2. [aqua backend](dev-tools/backends/aqua.md) - Curated metadata for tools without packslips
3. [github backend](dev-tools/backends/github.md) - Simple GitHub releases
4. [gitlab backend](dev-tools/backends/gitlab.md) - Tools released through GitLab
5. [Language package managers](dev-tools/backends/) - npm, pipx, cargo, gem, etc.
6. [backend plugins](backend-plugin-development.md) - Enhanced plugins with backend methods
7. [tool plugins](tool-plugin-development.md) - Hook-based cross-platform plugins

## Installing asdf (Legacy) Plugins

### From the Registry

Some registry entries retain asdf alternatives, but a shorthand may prefer another backend.
Select asdf explicitly when you need to test or maintain that implementation:

```bash
# Select the asdf implementation explicitly
mise use asdf:mise-plugins/mise-postgres@17

# The postgres shorthand currently prefers vfox instead
mise registry postgres
```

### From Git Repository

```bash
# Install plugin directly from repository
mise plugin install <plugin-name> <git-url>

# Example: PostgreSQL plugin
mise plugin install postgres https://github.com/mise-plugins/mise-postgres
```

### Manual Installation

```bash
# Add plugin manually
mise plugin add postgres https://github.com/mise-plugins/mise-postgres

# Install tool version
mise install postgres@17.0

# Use the tool
mise use postgres@17.0
```

An installed plugin with that name takes precedence over the registry shorthand when its
backend is enabled. Use the full `asdf:owner/repo` identifier above to select an implementation
without relying on an installed short-name plugin.

## Plugin Structure

asdf plugins follow this directory structure:

```
plugin-name/
├── bin/
│   ├── list-all          # List all available versions
│   ├── download          # Separate download phase [optional]
│   ├── install           # Install the tool
│   ├── latest-stable     # Get latest stable version [optional]
│   ├── help.overview     # Plugin description [optional]
│   ├── help.deps         # Plugin dependencies [optional]
│   ├── help.config       # Plugin configuration [optional]
│   ├── help.links        # Plugin links [optional]
│   ├── list-legacy-filenames  # Legacy version files [optional]
│   ├── parse-legacy-file # Parse legacy version files [optional]
│   ├── post-plugin-add   # Post plugin addition hook [optional]
│   ├── post-plugin-update # Post plugin update hook [optional]
│   ├── pre-plugin-remove # Pre plugin removal hook [optional]
│   └── exec-env          # Set execution environment [optional]
├── lib/                  # Shared library code [optional]
└── README.md
```

## Required Scripts

Provide `bin/list-all` and `bin/install`. The separate `bin/download` hook is optional;
without it, the install hook is responsible for obtaining the source or binary. Mark
scripts executable and write diagnostics to stderr so version output stays machine-readable.

### bin/list-all

Lists all available versions of the tool:

```bash
#!/usr/bin/env bash
set -euo pipefail
# Illustrative version list, ordered oldest to newest by the publisher's rules.
printf '%s\n' 1.0.0 1.1.0 1.10.0
```

For real plugins, query the publisher's release source and parse structured metadata with
a suitable parser. Do not scrape JSON with `grep` or assume every tool uses SemVer. Preserve
meaningful release order; `sort -V` is not portable to macOS and does not understand channels.

### bin/download

Downloads the tool source/binary:

```bash
#!/usr/bin/env bash
set -euo pipefail

# Input variables from mise
# ASDF_INSTALL_TYPE (version or ref)
# ASDF_INSTALL_VERSION (version number or git ref)
# ASDF_INSTALL_PATH (where to install)
# ASDF_DOWNLOAD_PATH (where to download)

version="$ASDF_INSTALL_VERSION"
download_path="$ASDF_DOWNLOAD_PATH"

# Download logic here
mkdir -p "$download_path"
curl -fSL -o "$download_path/archive.tar.gz" \
  "https://github.com/owner/repo/archive/v${version}.tar.gz"
```

### bin/install

Installs the tool. This source-build sketch assumes the archive contains a Makefile with
an `install` target accepting `PREFIX`, and that build dependencies are already available:

```bash
#!/usr/bin/env bash
set -euo pipefail

# Input variables from mise
# ASDF_INSTALL_TYPE (version or ref)
# ASDF_INSTALL_VERSION (version number or git ref)
# ASDF_INSTALL_PATH (where to install)
# ASDF_DOWNLOAD_PATH (where source is downloaded)

install_path="$ASDF_INSTALL_PATH"
download_path="$ASDF_DOWNLOAD_PATH"

# Extract and install
cd "$download_path"
tar -xzf archive.tar.gz --strip-components=1
make install PREFIX="$install_path"
```

## Optional Scripts

### bin/exec-env

Sets environment variables when the tool runs:

```bash
#!/usr/bin/env bash

# Set environment variables
export TOOL_HOME="$ASDF_INSTALL_PATH"
export PATH="$ASDF_INSTALL_PATH/bin:$PATH"
```

### bin/latest-stable

Gets the latest stable version:

```bash
#!/usr/bin/env bash
# Return a version from bin/list-all according to this tool's stable-release policy.
printf '%s\n' 1.10.0
```

### bin/list-legacy-filenames

Lists legacy version file names:

```bash
#!/usr/bin/env bash
echo ".example-version"
```

Enable idiomatic version files for the tool through
[`idiomatic_version_file_enable_tools`](/configuration/settings.html#idiomatic_version_file_enable_tools).
Do not return `.tool-versions`: mise already parses that multi-tool format itself.

### bin/parse-legacy-file

Parses a legacy version file:

```bash
#!/usr/bin/env bash
head -n 1 "$1"
```

## Environment Variables

Hook inputs depend on the phase. Installation hooks receive the version and path values;
update hooks receive the previous and new Git refs:

- `ASDF_INSTALL_TYPE` - `version` or `ref`
- `ASDF_INSTALL_VERSION` - Version number or git ref
- `ASDF_INSTALL_PATH` - Installation directory
- `ASDF_DOWNLOAD_PATH` - Download directory
- `ASDF_PLUGIN_PATH` - Plugin directory
- `ASDF_PLUGIN_PREV_REF` - Previous git ref (for updates)
- `ASDF_PLUGIN_POST_REF` - New git ref (for updates)

## Best Practices

### Error Handling

```bash
#!/usr/bin/env bash
set -euo pipefail  # Exit on error, undefined vars, pipe failures

# Check dependencies
command -v curl >/dev/null 2>&1 || {
  echo "Error: curl is required" >&2
  exit 1
}
```

### Cross-Platform Compatibility

```bash
#!/usr/bin/env bash

# Detect platform
case "$(uname -s)" in
  Darwin*) platform="darwin" ;;
  Linux*)  platform="linux" ;;
  *)       echo "Unsupported platform" >&2; exit 1 ;;
esac

case "$(uname -m)" in
  x86_64) arch="amd64" ;;
  arm64|aarch64) arch="arm64" ;;
  *)      echo "Unsupported architecture" >&2; exit 1 ;;
esac
```

### Version Parsing

Normalize a publisher prefix only when it is part of that tool's convention. Keep
non-numeric versions and channels intact; a shared SemVer parser is not appropriate.

```bash
#!/usr/bin/env bash

# Remove this example publisher's prefix
parse_version() {
  local version="$1"
  # Remove 'v' prefix if present
  version="${version#v}"
  echo "$version"
}
```

## Testing Plugins

### Local Development

```bash
# Link plugin for development
mise plugin link my-plugin /path/to/local/plugin

# Test basic functionality
mise ls-remote my-plugin
mise use my-plugin@1.0.0
mise exec -- my-plugin --version
```

### Debugging

```bash
# Enable debug mode
export MISE_DEBUG=1

# Or use --verbose flag
mise install --verbose my-plugin@1.0.0
```

## Example Plugin

This self-contained local fixture demonstrates the minimum interface without network
requests or a compiler. Create these two executable files under `my-plugin/bin/`:

```bash
#!/usr/bin/env bash
# bin/list-all
set -euo pipefail
printf '%s\n' 1.0.0
```

```bash
#!/usr/bin/env bash
# bin/install
set -euo pipefail
mkdir -p "$ASDF_INSTALL_PATH/bin"
cat > "$ASDF_INSTALL_PATH/bin/my-plugin" <<'SCRIPT'
#!/usr/bin/env sh
printf '%s\n' 'my-plugin 1.0.0'
SCRIPT
chmod +x "$ASDF_INSTALL_PATH/bin/my-plugin"
```

Test from a separate project directory:

```sh
chmod +x /path/to/my-plugin/bin/list-all /path/to/my-plugin/bin/install
mise plugin link my-plugin /path/to/my-plugin
mise ls-remote my-plugin
mise use my-plugin@1.0.0
mise exec -- my-plugin --version
```

Replace the fixture installer with your real download, verification, extraction, or build
steps. Keep `bin/exec-env` cheap: it can run while constructing the shell environment.

## Migration Path

Consider migrating from asdf plugins to modern alternatives:

1. **Check for [signed packslip releases](/dev-tools/backends/packslip.html), then whether the tool is available in [aqua registry](https://github.com/aquaproj/aqua-registry)**
2. **Use [github backend](dev-tools/backends/github.md) for simple GitHub releases**
3. **Create a [mise plugin](tool-plugin-development.md) for complex tools** - use the [mise-tool-plugin-template](https://github.com/jdx/mise-tool-plugin-template) for a quick start
4. **Use language-specific package managers** (npm, pipx, cargo, gem)

## Community Resources

- **[asdf Plugin List](https://github.com/asdf-vm/asdf-plugins)** - Official asdf plugin registry
- **[mise-plugins Organization](https://github.com/mise-plugins)** - Community-maintained plugins
- **[Plugin Template (asdf)](https://github.com/asdf-vm/asdf-plugin-template)** - Template for creating asdf plugins
- **[Plugin Template (mise)](https://github.com/jdx/mise-tool-plugin-template)** - Modern template for creating mise plugins with Lua

## Security Considerations

asdf plugins execute arbitrary shell scripts, which poses security risks:

- **Only install plugins from trusted sources**
- **Review plugin code before installation**
- **Avoid plugins with complex installation scripts when possible**
- **Consider using modern backends for better security**

## Next Steps

- [Explore modern backends](dev-tools/backends/) for better alternatives
- [Learn about backend plugins](backend-plugin-development.md) for enhanced functionality
- [Learn about tool plugins](tool-plugin-development.md) for cross-platform support
- [Check the registry](registry.md) for available tools
