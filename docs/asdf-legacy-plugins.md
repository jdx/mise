# asdf (Legacy) Plugins

mise maintains compatibility with the asdf plugin ecosystem through its asdf backend. These plugins are considered legacy because they have limitations compared to mise's modern plugin system.

## What are asdf (Legacy) Plugins?

asdf plugins are shell script-based plugins that follow the asdf plugin specification. They were the original way to extend tool management in the asdf ecosystem and are now supported by mise for backward compatibility.

## Limitations

asdf plugins have several limitations compared to mise's modern plugin system:

- **Platform Support**: Only work on Linux and macOS (no Windows support)
- **Performance**: Shell script execution is slower than mise's native backends
- **Features**: Limited compared to modern backends like aqua, ubi, or tool/backend plugins
- **Maintenance**: Harder to maintain and debug
- **Security**: Less secure than sandboxed modern backends

## When to Use asdf (Legacy) Plugins

Only use asdf plugins when:

- The tool is not available through modern backends (aqua, ubi, etc.)
- You need compatibility with existing asdf workflows
- The tool requires complex shell-based installation logic that can't be handled by modern backends

**For new tools, consider these alternatives first:**

1. [aqua backend](dev-tools/backends/aqua.md) - Preferred for GitHub releases
2. [ubi backend](dev-tools/backends/ubi.md) - Simple GitHub/GitLab releases
3. [Language package managers](dev-tools/backends/) - npm, pipx, cargo, gem, etc.
4. [backend plugins](backend-plugin-development.md) - Enhanced plugins with backend methods
5. [tool plugins](tool-plugin-development.md) - Hook-based cross-platform plugins

## Installing asdf (Legacy) Plugins

### From the Registry

Most popular asdf plugins are available through mise's registry:

```bash
# Install from registry shorthand
mise use postgres@15

# This is equivalent to
mise use asdf:mise-plugins/mise-postgres@15
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
mise install postgres@15.0.0

# Use the tool
mise use postgres@15.0.0
```

## Plugin Structure

asdf plugins follow this directory structure:

```
plugin-name/
├── bin/
│   ├── list-all          # List all available versions
│   ├── download          # Download source code/binary
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

### bin/list-all

Lists all available versions of the tool:

```bash
#!/usr/bin/env bash
# List all available versions
curl -s https://api.github.com/repos/owner/repo/releases |
  grep '"tag_name":' |
  sed -E 's/.*"([^"]+)".*/\1/' |
  sort -V
```

### bin/download

Downloads the tool source/binary:

```bash
#!/usr/bin/env bash
set -e

# Input variables from mise
# ASDF_INSTALL_TYPE (version or ref)
# ASDF_INSTALL_VERSION (version number or git ref)
# ASDF_INSTALL_PATH (where to install)
# ASDF_DOWNLOAD_PATH (where to download)

version="$ASDF_INSTALL_VERSION"
download_path="$ASDF_DOWNLOAD_PATH"

# Download logic here
curl -Lo "$download_path/archive.tar.gz" \
  "https://github.com/owner/repo/archive/v${version}.tar.gz"
```

### bin/install

Installs the tool:

```bash
#!/usr/bin/env bash
set -e

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

Set environment variables when executing tools:

```bash
#!/usr/bin/env bash

# Set environment variables
export TOOL_HOME="$ASDF_INSTALL_PATH"
export PATH="$ASDF_INSTALL_PATH/bin:$PATH"
```

### bin/latest-stable

Get the latest stable version:

```bash
#!/usr/bin/env bash
curl -s https://api.github.com/repos/owner/repo/releases/latest |
  grep '"tag_name":' |
  sed -E 's/.*"([^"]+)".*/\1/'
```

### bin/list-legacy-filenames

List legacy version file names:

```bash
#!/usr/bin/env bash
echo ".tool-version"
echo ".tool-versions"
```

### bin/parse-legacy-file

Parse legacy version files:

```bash
#!/usr/bin/env bash
cat "$1" | head -n 1
```

## Environment Variables

asdf plugins have access to these environment variables:

- `ASDF_INSTALL_TYPE` - `version` or `ref`
- `ASDF_INSTALL_VERSION` - Version number or git ref
- `ASDF_INSTALL_PATH` - Installation directory
- `ASDF_DOWNLOAD_PATH` - Download directory
- `ASDF_PLUGIN_PATH` - Plugin directory
- `ASDF_PLUGIN_PREV_REF` - Previous git ref (for updates)
- `ASDF_PLUGIN_POST_REF` - New git ref (for updates)
- `ASDF_CMD_FILE` - Path to executable being run

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
  arm64)  arch="arm64" ;;
  *)      echo "Unsupported architecture" >&2; exit 1 ;;
esac
```

### Version Parsing

```bash
#!/usr/bin/env bash

# Parse semantic version
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
mise plugin add my-plugin /path/to/local/plugin

# Test basic functionality
mise list-all my-plugin
mise install my-plugin@1.0.0
mise which my-plugin
```

### Debugging

```bash
# Enable debug mode
export MISE_DEBUG=1

# Or use --verbose flag
mise install --verbose my-plugin@1.0.0
```

## Example Plugin

Here's a minimal example for a fictional tool:

```bash
#!/usr/bin/env bash
# bin/list-all
curl -s "https://api.github.com/repos/example/tool/releases" |
  grep '"tag_name":' |
  sed -E 's/.*"v([^"]+)".*/\1/' |
  sort -V
```

```bash
#!/usr/bin/env bash
# bin/download
set -e
version="$ASDF_INSTALL_VERSION"
platform=$(uname -s | tr '[:upper:]' '[:lower:]')
arch=$(uname -m)

url="https://github.com/example/tool/releases/download/v${version}/tool-${platform}-${arch}.tar.gz"
curl -fSL "$url" -o "$ASDF_DOWNLOAD_PATH/tool.tar.gz"
```

```bash
#!/usr/bin/env bash
# bin/install
set -e
cd "$ASDF_DOWNLOAD_PATH"
tar -xzf tool.tar.gz
cp tool "$ASDF_INSTALL_PATH/bin/"
chmod +x "$ASDF_INSTALL_PATH/bin/tool"
```

## Migration Path

Consider migrating from asdf plugins to modern alternatives:

1. **Check if tool is available in [aqua registry](https://aquaproj.github.io/aqua-registry/)**
2. **Use [ubi backend](dev-tools/backends/ubi.md) for simple GitHub releases**
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
