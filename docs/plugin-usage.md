# Using Plugins

mise supports plugins that extend its functionality, allowing you to install tools that aren't available in the standard registry. This is particularly useful for:

- Installing tools from private repositories
- Using experimental or niche tools
- Creating custom tool installations for your team

## What Are Plugins?

Plugins are extensions that can install and manage tools not included in mise's built-in registry. They are written in Lua and come in two main types:

### Backend Plugins

Backend plugins use enhanced backend methods and support the `plugin:tool` format:

- **Multiple Tools**: A single plugin can manage multiple tools
- **Enhanced Methods**: Backend methods for listing, installing, and environment setup
- **Format**: Use the `plugin:tool` format (e.g., `vfox-npm:prettier`)

### Tool Plugins

Tool plugins use the traditional hook-based approach:

- **Single Tool**: Each plugin manages one tool
- **Hook-based**: Use hooks like `PreInstall`, `PostInstall`, `Available`, etc.
- **Format**: Use the tool name directly (e.g., `my-tool`)

Both types:

- Install tools from any source (npm packages, GitHub releases, custom builds)
- Set up environment variables and PATH entries
- Handle version management and listing
- Work across all platforms (Windows, macOS, Linux)

## Installing Plugins

### From a Git Repository

```bash
# Install a plugin from a repository
mise plugin install <plugin-name> <repository-url>

# Example: Installing the vfox-npm plugin
mise plugin install vfox-npm https://github.com/jdx/vfox-npm
```

### From Local Directory

```bash
# Link a local plugin for development
mise plugin link <plugin-name> /path/to/plugin/directory
```

## Using Plugins (Advanced)

Once a plugin is installed, you can use it with the `plugin:tool` format:

```bash
# Install a specific tool using the plugin
mise install vfox-npm:prettier@latest

# Use the tool
mise use vfox-npm:prettier@3.0.0

# Execute the tool
mise exec vfox-npm:prettier -- --version

# List available versions
mise ls-remote vfox-npm:prettier
```

## Plugin:Tool Format

The `plugin:tool` format allows a single plugin to manage multiple tools. This is particularly useful for:

- **Package managers**: Install different npm packages, Python packages, etc.
- **Tool families**: Manage related tools from the same ecosystem
- **Custom builds**: Install different variants of the same tool

### Example: npm packages

```bash
# Install different npm packages using the same plugin
mise install vfox-npm:prettier@latest
mise install vfox-npm:eslint@8.0.0
mise install vfox-npm:typescript@latest

# Use them in your project
mise use vfox-npm:prettier@latest vfox-npm:eslint@8.0.0
```

## Managing Plugins

### List installed plugins

```bash
# Show all plugins
mise plugins ls

# Show plugin URLs
mise plugins ls --urls
```

### Update plugins

```bash
# Update a specific plugin
mise plugin update vfox-npm

# Update all plugins
mise plugin update --all
```

### Remove plugins

```bash
# Remove a plugin
mise plugin remove vfox-npm

# This will also remove all tools installed by the plugin
```

## Configuration

Plugins can be configured in your `mise.toml` file:

```toml
[plugins]
vfox-npm = "https://github.com/jdx/vfox-npm"

[tools]
"vfox-npm:prettier" = "latest"
"vfox-npm:eslint" = "8.0.0"
```

## Finding Plugins

While mise doesn't have a centralized registry for community plugins, you can find them:

- **GitHub**: Search for repositories with "vfox-" prefix
- **Community**: Check mise community discussions and Discord
- **Company internal**: Your organization may have private plugins

## Plugin Examples

### vfox-npm (Example Plugin)

The `vfox-npm` plugin demonstrates how to create a plugin that installs npm packages:

```bash
# Install the plugin
mise plugin install vfox-npm https://github.com/jdx/vfox-npm

# Install tools
mise install vfox-npm:prettier@latest
mise install vfox-npm:eslint@latest

# Use them
mise use vfox-npm:prettier@latest
mise exec vfox-npm:prettier -- --check .
```

::: info
This is just an example plugin for testing. mise already has built-in npm support that you should use instead: `mise install npm:prettier@latest`
:::

## Backend Plugins (Advanced)

Backend plugins use enhanced backend methods that provide better performance and support for the `plugin:tool` format:

- **BackendListVersions**: Lists available versions of a tool
- **BackendInstall**: Installs a specific version
- **BackendExecEnv**: Sets up environment variables

This architecture allows plugins to manage multiple tools efficiently while providing a consistent interface.

## Tool Plugins (Advanced)

Tool plugins use the traditional hook-based approach:

- **Available**: Lists available versions
- **PreInstall/PostInstall**: Installation hooks
- **EnvKeys**: Environment variable setup
- **Parse**: Version parsing and validation

Both architectures provide a flexible plugin system that can handle diverse installation and management needs.

## Security Considerations

::: danger
When using plugins, be aware that:

- **Plugins execute arbitrary code** during installation and use
- **Only install plugins from trusted sources**
- **Review plugin code** before installation when possible
- **Use version pinning** to avoid unexpected updates like [`mise.lock`](/dev-tools/mise-lock.md)
:::

## Troubleshooting

### Plugin installation fails

```bash
# Check if the repository URL is correct
mise plugin install vfox-npm https://github.com/jdx/vfox-npm

# Check plugin directory
ls ~/.local/share/mise/plugins/
```

### Tool installation fails

```bash
# Check plugin logs
mise install vfox-npm:prettier@latest --verbose

# Verify plugin is installed
mise plugins ls
```

### Environment issues

```bash
# Check if PATH is set correctly
mise exec vfox-npm:prettier env | grep PATH

# Verify tool is installed
ls ~/.local/share/mise/installs/vfox-npm/prettier/
```

## Next Steps

- [Learn how to create backend plugins](backend-plugin-development.md)
- [Learn how to create tool plugins](tool-plugin-development.md)
- [Explore built-in backends](dev-tools/backends/)
- [Check the community registry](registry.md)
