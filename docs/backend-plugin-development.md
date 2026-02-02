# Backend Plugin Development

Backend plugins in mise use enhanced backend methods to manage multiple tools using the `plugin:tool` format. These plugins are perfect for package managers, tool families, and custom installations that need to manage multiple related tools.

## What are Backend Plugins?

Backend plugins extend the standard vfox plugin system with enhanced backend methods. They support:

- **Multiple Tools**: One plugin can manage multiple tools. For example, `vfox-npm` is the plugin which could install different types of tools like `prettier`, `eslint`, and other npm packages
- **Cross-Platform Support**: Works on Windows, macOS, and Linux
- **Flexible Architecture**: Modern plugin system with dedicated backend methods for enhanced functionality

## Plugin Architecture

Backend plugins are generally a git repository but can also be a directory (via `mise link`).

Backend plugins are implemented in [Luau](https://luau.org/), a fast, small, safe, gradually typed embeddable scripting language derived from Lua. They use three main backend methods implemented as individual files:

- `hooks/backend_list_versions.luau` - Lists available versions for a tool
- `hooks/backend_install.luau` - Installs a specific version of a tool
- `hooks/backend_exec_env.luau` - Sets up environment variables for a tool

## Type Definitions

Create a `lib/types.luau` file for shared type definitions:

```lua
--!strict
export type BackendListVersionsResult = {
    versions: { string },
}

export type BackendInstallResult = {}

export type BackendExecEnvResult = {
    env_vars: { { key: string, value: string } },
}

export type BackendListVersionsContext = {
    tool: string,
}

export type BackendInstallContext = {
    tool: string,
    version: string,
    install_path: string,
}

export type BackendExecEnvContext = {
    tool: string,
    version: string,
    install_path: string,
}

export type PluginType = {
    BackendListVersions: (self: PluginType, ctx: BackendListVersionsContext) -> BackendListVersionsResult,
    BackendInstall: (self: PluginType, ctx: BackendInstallContext) -> BackendInstallResult,
    BackendExecEnv: (self: PluginType, ctx: BackendExecEnvContext) -> BackendExecEnvResult,
}

export type CmdModule = {
    exec: (command: string, opts: { cwd: string?, env: { [string]: string }? }?) -> string,
}

export type JsonModule = {
    encode: (value: any) -> string,
    decode: (str: string) -> any,
}

export type FileModule = {
    read: (path: string) -> string?,
    exists: (path: string) -> boolean,
    join_path: (...string) -> string,
}

return nil
```

### .luaurc Configuration

Create a `.luaurc` file in your plugin root:

```json
{
  "languageMode": "strict",
  "globals": ["PLUGIN", "OS_TYPE", "ARCH_TYPE"],
  "aliases": {
    "@lib": "lib"
  }
}
```

## Backend Methods

### BackendListVersions

Lists available versions for a tool:

```lua
--!strict
-- hooks/backend_list_versions.luau
local Types = require("@lib/types")

local plugin = PLUGIN :: Types.PluginType

function plugin:BackendListVersions(ctx: Types.BackendListVersionsContext): Types.BackendListVersionsResult
    local tool = ctx.tool
    local versions: { string } = {}

    -- Your logic to fetch versions for the tool
    -- Example: query an API, parse a registry, etc.

    return { versions = versions }
end
```

> [!WARNING]
> **Version sorting**: The versions returned by `BackendListVersions` should be in ascending order (oldest to newest), sorted semantically (version `3.10.0` should not come before `3.2.0`). Mise does not apply any additional sorting to the versions returned by this method.

### BackendInstall

Installs a specific version of a tool:

```lua
--!strict
-- hooks/backend_install.luau
local Types = require("@lib/types")

local plugin = PLUGIN :: Types.PluginType

function plugin:BackendInstall(ctx: Types.BackendInstallContext): Types.BackendInstallResult
    local tool = ctx.tool
    local version = ctx.version
    local install_path = ctx.install_path

    -- Your logic to install the tool
    -- Example: download files, extract archives, etc.

    return {}
end
```

### BackendExecEnv

Sets up environment variables for a tool:

```lua
--!strict
-- hooks/backend_exec_env.luau
local Types = require("@lib/types")
local file = require("file") :: Types.FileModule

local plugin = PLUGIN :: Types.PluginType

function plugin:BackendExecEnv(ctx: Types.BackendExecEnvContext): Types.BackendExecEnvResult
    local install_path = ctx.install_path

    -- Your logic to set up environment variables
    -- Example: add bin directories to PATH

    return {
        env_vars = {
            { key = "PATH", value = file.join_path(install_path, "bin") },
        },
    }
end
```

## Creating a Backend Plugin

### Using the Template Repository

Use the dedicated [mise-backend-plugin-template](https://github.com/jdx/mise-backend-plugin-template) for creating backend plugins:

```bash
# Option 1: Use GitHub's template feature (recommended)
# Visit https://github.com/jdx/mise-backend-plugin-template
# Click "Use this template" to create your repository

# Option 2: Clone and modify
git clone https://github.com/jdx/mise-backend-plugin-template my-backend-plugin
cd my-backend-plugin
rm -rf .git
git init
```

The template includes:

- Complete backend plugin structure with all required hooks
- Type definitions in `lib/types.luau`
- Modern development tooling (`.luaurc`, stylua, luau-analyze, actionlint)
- Comprehensive documentation and examples
- CI/CD setup with GitHub Actions
- Multiple implementation patterns for different backend types

### 1. Plugin Structure

Create a directory with this structure:

```
my-backend-plugin/
├── metadata.luau                   # Plugin metadata
├── .luaurc                         # Luau type checking configuration
├── hooks/
│   ├── backend_list_versions.luau  # BackendListVersions hook
│   ├── backend_install.luau        # BackendInstall hook
│   └── backend_exec_env.luau       # BackendExecEnv hook
└── lib/
    └── types.luau                  # Type definitions
```

### 2. Basic metadata.luau

```lua
-- metadata.luau
PLUGIN = {
    name = "vfox-npm",
    version = "1.0.0",
    description = "Backend plugin for npm packages",
    author = "Your Name",
}
```

## Real-World Example: vfox-npm

Here's the complete implementation of the vfox-npm plugin that manages npm packages:

### metadata.luau

```lua
-- metadata.luau
PLUGIN = {
    name = "vfox-npm",
    version = "1.0.0",
    description = "Backend plugin for npm packages",
    author = "jdx",
}
```

### hooks/backend_list_versions.luau

```lua
--!strict
-- hooks/backend_list_versions.luau
local Types = require("@lib/types")
local cmd = require("cmd") :: Types.CmdModule
local json = require("json") :: Types.JsonModule

local plugin = PLUGIN :: Types.PluginType

function plugin:BackendListVersions(ctx: Types.BackendListVersionsContext): Types.BackendListVersionsResult
    local result = cmd.exec(`npm view {ctx.tool} versions --json`)
    local versions = json.decode(result)

    return { versions = versions }
end
```

### hooks/backend_install.luau

```lua
--!strict
-- hooks/backend_install.luau
local Types = require("@lib/types")
local cmd = require("cmd") :: Types.CmdModule

local plugin = PLUGIN :: Types.PluginType

function plugin:BackendInstall(ctx: Types.BackendInstallContext): Types.BackendInstallResult
    local tool = ctx.tool
    local version = ctx.version
    local install_path = ctx.install_path

    -- Install the package directly using npm install
    local npm_cmd = `npm install {tool}@{version} --no-package-lock --no-save --silent`
    cmd.exec(npm_cmd, { cwd = install_path })

    -- If we get here, the command succeeded
    return {}
end
```

### hooks/backend_exec_env.luau

```lua
--!strict
-- hooks/backend_exec_env.luau
local Types = require("@lib/types")
local file = require("file") :: Types.FileModule

local plugin = PLUGIN :: Types.PluginType

function plugin:BackendExecEnv(ctx: Types.BackendExecEnvContext): Types.BackendExecEnvResult
    return {
        env_vars = {
            { key = "PATH", value = file.join_path(ctx.install_path, "node_modules", ".bin") },
        },
    }
end
```

## Usage Example

The plugin name doesn't have to match the repository name. The backend prefix will match whatever name the backend plugin was installed as.

```bash
# Install the plugin
mise plugin install vfox-npm https://github.com/jdx/vfox-npm

# List available versions
mise ls-remote vfox-npm:prettier

# Install a specific version
mise install vfox-npm:prettier@3.0.0

# Use in a project
mise use vfox-npm:prettier@latest

# Execute the tool
mise exec -- prettier --help
```

> **Tip**: This naming flexibility could potentially be used to have a very complex plugin backend that would behave differently based on what it was named. For example, you could install the same plugin with different names to configure different behaviors or access different tool registries.

## Context Variables

Backend plugins receive context through the `ctx` parameter passed to each hook function:

### BackendListVersions Context

| Variable   | Description   | Example      |
| ---------- | ------------- | ------------ |
| `ctx.tool` | The tool name | `"prettier"` |

### BackendInstall and BackendExecEnv Context

| Variable           | Description            | Example                                                           |
| ------------------ | ---------------------- | ----------------------------------------------------------------- |
| `ctx.tool`         | The tool name          | `"prettier"`                                                      |
| `ctx.version`      | The requested version  | `"3.0.0"`                                                         |
| `ctx.install_path` | Installation directory | `"/home/user/.local/share/mise/installs/vfox-npm/prettier/3.0.0"` |

## Testing Your Plugin

### Local Development

```bash
# Link your plugin for development
mise plugin link my-plugin /path/to/my-plugin

# Test listing versions
mise ls-remote my-plugin:some-tool

# Test installation
mise use my-plugin:some-tool@1.0.0

# Test execution
mise exec -- some-tool --version
```

### Debug Mode

Use debug mode to see detailed plugin execution:

```bash
mise --debug install my-plugin:some-tool@1.0.0
```

## Best Practices

### Error Handling

Provide meaningful error messages:

```lua
--!strict
-- hooks/backend_list_versions.luau
local Types = require("@lib/types")
local cmd = require("cmd") :: Types.CmdModule
local json = require("json") :: Types.JsonModule

local plugin = PLUGIN :: Types.PluginType

function plugin:BackendListVersions(ctx: Types.BackendListVersionsContext): Types.BackendListVersionsResult
    local tool = ctx.tool

    -- Validate tool name
    if not tool or tool == "" then
        error("Tool name cannot be empty")
    end

    -- Execute command with error checking
    local ok, result = pcall(cmd.exec, `npm view {tool} versions --json 2>/dev/null`)
    if not ok or not result or result:match("npm ERR!") then
        error(`Failed to fetch versions for {tool}: {result or "no output"}`)
    end

    -- Parse JSON response
    local success, npm_versions = pcall(json.decode, result)
    if not success or not npm_versions then
        error(`Failed to parse versions for {tool}`)
    end

    -- Return versions or error if none found
    local versions: { string } = {}
    if type(npm_versions) == "table" then
        for i = #npm_versions, 1, -1 do
            table.insert(versions, npm_versions[i])
        end
    end

    if #versions == 0 then
        error(`No versions found for {tool}`)
    end

    return { versions = versions }
end
```

### Regex Parsing

Parse versions with regex:

```lua
--!strict
local function parse_version(version_string: string): string
    -- Remove prefixes like 'v' or 'release-'
    return version_string:gsub("^v", ""):gsub("^release%-", "")
end
```

### Path Handling

Use cross-platform path handling:

```lua
--!strict
local Types = require("@lib/types")
local file = require("file") :: Types.FileModule

-- Use the built-in file.join_path for cross-platform path joining
local bin_path = file.join_path(install_path, "bin")
```

### Cross-Platform Commands

Handle different operating systems:

```lua
--!strict
local Types = require("@lib/types")
local cmd = require("cmd") :: Types.CmdModule

local function create_dir(path: string)
    local mkdir_cmd = if OS_TYPE == "windows" then "mkdir" else "mkdir -p"
    cmd.exec(`{mkdir_cmd} {path}`)
end
```

## Advanced Features

### Conditional Installation

Different installation logic based on tool or version:

```lua
--!strict
-- hooks/backend_install.luau
local Types = require("@lib/types")
local cmd = require("cmd") :: Types.CmdModule

local plugin = PLUGIN :: Types.PluginType

function plugin:BackendInstall(ctx: Types.BackendInstallContext): Types.BackendInstallResult
    local tool = ctx.tool
    local version = ctx.version
    local install_path = ctx.install_path

    -- Create install directory
    cmd.exec(`mkdir -p {install_path}`)

    if tool == "special-tool" then
        -- Special installation logic
        local npm_cmd = `npm install {tool}@{version} --no-package-lock --no-save --silent`
        cmd.exec(npm_cmd, { cwd = install_path })
    else
        -- Default installation logic
        local npm_cmd = `npm install {tool}@{version} --no-package-lock --no-save --silent`
        cmd.exec(npm_cmd, { cwd = install_path })
    end

    return {}
end
```

### Environment Detection

The following globals are automatically available in all plugin hooks:

```lua
--!strict
-- hooks/backend_install.luau
local Types = require("@lib/types")

local plugin = PLUGIN :: Types.PluginType

function plugin:BackendInstall(ctx: Types.BackendInstallContext): Types.BackendInstallResult
    -- Platform-specific installation
    if OS_TYPE == "darwin" then
        -- macOS installation logic
    elseif OS_TYPE == "linux" then
        -- Linux installation logic
    elseif OS_TYPE == "windows" then
        -- Windows installation logic
    end

    return {}
end
```

Available globals:

- `OS_TYPE`: Operating system type (`"windows"`, `"linux"`, `"darwin"`)
- `ARCH_TYPE`: Architecture (`"amd64"`, `"arm64"`, `"386"`, etc.)
- `PLUGIN`: The plugin object for defining hook methods

### Multiple Environment Variables

Set multiple environment variables:

```lua
--!strict
-- hooks/backend_exec_env.luau
local Types = require("@lib/types")
local file = require("file") :: Types.FileModule

local plugin = PLUGIN :: Types.PluginType

function plugin:BackendExecEnv(ctx: Types.BackendExecEnvContext): Types.BackendExecEnvResult
    -- Add node_modules/.bin to PATH for npm-installed binaries
    local bin_path = file.join_path(ctx.install_path, "node_modules", ".bin")
    return {
        env_vars = {
            { key = "PATH", value = bin_path },
            { key = `{string.upper(ctx.tool)}_HOME`, value = ctx.install_path },
            { key = `{string.upper(ctx.tool)}_VERSION`, value = ctx.version },
        },
    }
end
```

## Performance Optimization

### Caching

TODO: We need caching support for [Shared Lua modules](plugin-lua-modules.md).

## Next Steps

- [Start with the backend plugin template](https://github.com/jdx/mise-backend-plugin-template)
- [Learn about Tool Plugin Development](tool-plugin-development.md)
- [Explore available Lua modules](plugin-lua-modules.md)
- [Publishing your plugin](plugin-publishing.md)
- [View the vfox-npm plugin source](https://github.com/jdx/vfox-npm)
