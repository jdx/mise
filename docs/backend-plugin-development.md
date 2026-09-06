# Backend Plugin Development

::: tip
The [mise-backend-plugin-template](https://github.com/jdx/mise-backend-plugin-template) provides a ready-to-use starting point with LuaCATS type definitions, stylua formatting, and hk linting pre-configured.
:::

Backend plugins in mise use dedicated backend hooks to manage multiple tools with the `plugin:tool` format. They are well suited to package managers, tool families, and custom installations that manage several related tools.

## What are Backend Plugins?

Backend plugins extend the standard vfox plugin system with dedicated backend hooks. They support:

- **Multiple Tools**: One plugin can manage multiple tools. For example, `vfox-npm` can install `prettier`, `eslint`, and other npm packages
- **Cross-Platform Support**: Lua runs on Windows, macOS, and Linux; your installer must support each target
- **Flexible Architecture**: A modern plugin system with dedicated backend methods

## Plugin Architecture

Backend plugins are generally a git repository but can also be a directory (via `mise plugin link`).

Backend plugins are written in Lua (currently version 5.1). They use three main backend methods, each implemented in its own file:

- `hooks/backend_list_versions.lua` - Lists available versions for a tool
- `hooks/backend_install.lua` - Installs a specific version of a tool
- `hooks/backend_exec_env.lua` - Sets up environment variables for a tool

## Backend Methods

### BackendListVersions

Lists available versions for a tool:

```lua
function PLUGIN:BackendListVersions(ctx)
    local tool = ctx.tool
    local options = ctx.options
    local versions = {}

    -- Your logic to fetch versions for the tool
    -- Example: query an API, parse a registry, etc.
    -- Access custom options via options["key"] or options.key

    return {versions = versions}
end
```

> [!WARNING]
> Return versions **oldest to newest**, according to the tool's release policy. mise preserves
> that order. Do not assume SemVer: versions may be dates, prereleases, or channel names.
> This is the opposite of a tool plugin's `Available` hook, which returns newest first.

### BackendInstall

Installs a specific version of a tool:

```lua
function PLUGIN:BackendInstall(ctx)
    local tool = ctx.tool
    local version = ctx.version
    local install_path = ctx.install_path
    local download_path = ctx.download_path
    local options = ctx.options

    -- Your logic to install the tool
    -- Example: download files, extract archives, etc.
    -- Access custom options via options["key"] or options.key

    return {}
end
```

### BackendExecEnv

Returns environment entries for a selected installation. Implement this hook even when
there are no entries to add; return `{env_vars = {}}` in that case:

```lua
function PLUGIN:BackendExecEnv(ctx)
    local install_path = ctx.install_path
    local options = ctx.options

    -- Your logic to set up environment variables
    -- Example: add bin directories to PATH
    -- Access custom options via options["key"] or options.key

    return {
        env_vars = {
            {key = "PATH", value = install_path .. "/bin"}
        }
    }
end
```

## Creating a Backend Plugin

### Using the Template Repository

Use the dedicated [mise-backend-plugin-template](https://github.com/jdx/mise-backend-plugin-template) to create backend plugins:

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
- Modern development tooling (hk, stylua, luacheck, actionlint)
- Comprehensive documentation and examples
- CI/CD setup with GitHub Actions
- Multiple implementation patterns for different backend types

### 1. Plugin Structure

Create a directory with this structure:

```
my-backend-plugin/
├── metadata.lua                    # Plugin metadata
├── hooks/
│   ├── backend_list_versions.lua   # BackendListVersions hook
│   ├── backend_install.lua         # BackendInstall hook
│   └── backend_exec_env.lua        # BackendExecEnv hook

```

### 2. Basic metadata.lua

```lua
PLUGIN = {
    name = "vfox-npm",
    version = "1.0.0",
    description = "Backend plugin for npm packages",
    author = "Your Name"
}
```

## Real-World Example: vfox-npm

This small teaching implementation uses npm to install packages. It requires a POSIX shell
and Node/npm on PATH; the commands below are not a Windows implementation. For everyday
use, prefer the built-in [npm backend](/dev-tools/backends/npm.html), which handles platform
integration and additional installation options.

The snippets belong in the three hook files shown. They pass package values through quoted
environment variables instead of concatenating them into shell commands.

### metadata.lua

```lua
PLUGIN = {
    name = "vfox-npm",
    version = "1.0.0",
    description = "Backend plugin for npm packages",
    author = "Plugin Author",
    depends = { "node" },
}
```

### hooks/backend_list_versions.lua

```lua
function PLUGIN:BackendListVersions(ctx)
    if RUNTIME.osType == "windows" then
        error("This example requires a POSIX shell")
    end
    local cmd = require("cmd")
    local json = require("json")
    local result = cmd.exec('npm view "$MISE_PLUGIN_PACKAGE" versions --json', {
        env = {MISE_PLUGIN_PACKAGE = ctx.tool},
    })
    local versions = json.decode(result)
    -- npm can return a single version as a string.
    if type(versions) == "string" then
        versions = {versions}
    end
    if type(versions) ~= "table" or #versions == 0 then
        error("No versions returned for " .. ctx.tool)
    end
    return {versions = versions}
end
```

### hooks/backend_install.lua

```lua
function PLUGIN:BackendInstall(ctx)
    if RUNTIME.osType == "windows" then
        error("This example requires a POSIX shell")
    end
    local cmd = require("cmd")
    cmd.exec('npm install --no-package-lock --no-save -- "$MISE_PLUGIN_SPEC"', {
        cwd = ctx.install_path,
        env = {MISE_PLUGIN_SPEC = ctx.tool .. "@" .. ctx.version},
    })
    return {}
end
```

### hooks/backend_exec_env.lua

```lua
function PLUGIN:BackendExecEnv(ctx)
    local file = require("file")
    return {
        env_vars = {
            {key = "PATH", value = file.join_path(ctx.install_path, "node_modules", ".bin")}
        }
    }
end
```

## Usage Example

The plugin name doesn't have to match the repository name. The backend prefix is whatever name the plugin was installed under.

```bash
# Link the example you created and configure its prerequisite
mise plugin link vfox-npm /path/to/your/plugin
mise use node@24

# List available versions
mise ls-remote vfox-npm:prettier

# Install a specific version
mise install vfox-npm:prettier@3.0.0

# Use in a project
mise use vfox-npm:prettier@latest

# Execute the tool
mise exec -- prettier --help
```

Use a name that does not collide with a built-in backend. To test different registries or
behavior, define explicit tool options and read `ctx.options`; the installed name is not a
substitute for an options contract.

## Context Variables

Backend plugins receive context through the `ctx` parameter passed to each hook function:

### BackendListVersions Context

| Variable      | Description                 | Example                   |
| ------------- | --------------------------- | ------------------------- |
| `ctx.tool`    | The tool name               | `"prettier"`              |
| `ctx.options` | Tool options from mise.toml | `{channels = {"a", "b"}}` |

### BackendInstall Context

| Variable            | Description                 | Example                                                            |
| ------------------- | --------------------------- | ------------------------------------------------------------------ |
| `ctx.tool`          | The tool name               | `"prettier"`                                                       |
| `ctx.version`       | The requested version       | `"3.0.0"`                                                          |
| `ctx.install_path`  | Installation directory      | `"/home/user/.local/share/mise/installs/vfox-npm-prettier/3.0.0"`  |
| `ctx.download_path` | Download directory          | `"/home/user/.local/share/mise/downloads/vfox-npm-prettier/3.0.0"` |
| `ctx.options`       | Tool options from mise.toml | `{exe = "rg"}`                                                     |

### BackendExecEnv Context

| Variable           | Description                 | Example                                                           |
| ------------------ | --------------------------- | ----------------------------------------------------------------- |
| `ctx.tool`         | The tool name               | `"prettier"`                                                      |
| `ctx.version`      | The requested version       | `"3.0.0"`                                                         |
| `ctx.install_path` | Installation directory      | `"/home/user/.local/share/mise/installs/vfox-npm-prettier/3.0.0"` |
| `ctx.options`      | Tool options from mise.toml | `{exe = "rg"}`                                                    |

> [!TIP]
> Option values preserve their TOML types as native Lua equivalents. Strings remain strings,
> arrays become Lua sequence tables, and nested tables become Lua map tables. For example,
> `channels = ["conda-forge", "robostack"]` in `mise.toml` becomes a Lua table you can
> iterate with `ipairs(ctx.options.channels)`.

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

`cmd.exec` raises an error on a nonzero exit status and includes stderr. Do not hide stderr
or search successful stdout for an error string. Check HTTP status codes before parsing
bodies, validate required response fields, and keep credentials out of errors.

The [Lua modules reference](/plugin-lua-modules.html) explains synchronous errors and the
HTTP `try_*` methods for recoverable transport failures.

### Regex Parsing

Parse versions with Lua patterns (Lua does not have regular expressions; `string.match`/`string.gsub` use Lua's own pattern syntax):

```lua
local function parse_version(version_string)
    -- Remove prefixes like 'v' or 'release-'
    return version_string:gsub("^v", ""):gsub("^release%-", "")
end
```

### Path Handling

Use `file.join_path` for path construction and `cmd.exec`'s `cwd` option for the command's
working directory. Prefer file operations over shelling out to `mkdir`, `cp`, or `mv`.
If your installer needs shell commands, document the shell and quote every external value.

```lua
local file = require("file")
local bin_path = file.join_path(ctx.install_path, "bin")
```

### Cross-Platform Commands

The Lua runtime does not translate a shell command between operating systems. A POSIX
`mkdir -p`, `$VARIABLE`, or `chmod` example needs a different implementation on Windows.
Test on each platform you claim to support, including paths containing spaces.

## Advanced Features

### Conditional Installation

Choose installation logic using `ctx.tool`, `ctx.version`, and `RUNTIME`. Validate that the
tool and platform are supported before downloading or running an installer. Keep shared
logic in a Lua helper module instead of duplicating the same command in every branch.

### Environment Detection

vfox automatically injects runtime information into your plugin:

```lua
function PLUGIN:BackendInstall(ctx)
    -- Platform-specific installation using injected RUNTIME object
    if RUNTIME.osType == "darwin" then
        -- macOS installation logic
    elseif RUNTIME.osType == "linux" then
        -- Linux installation logic
    elseif RUNTIME.osType == "windows" then
        -- Windows installation logic
    end

    return {}
end
```

The `RUNTIME` object provides:

- `RUNTIME.osType`: Operating system type ("windows", "linux", "darwin")
- `RUNTIME.archType`: Architecture (`"amd64"`, `"arm64"`, `"x86"`, etc.)
- `RUNTIME.envType`: libc environment type (`"gnu"` on glibc Linux, `"musl"` on musl Linux, `nil` on Windows/macOS and undetected systems)
- `RUNTIME.version`: vfox runtime version
- `RUNTIME.pluginDirPath`: Plugin directory path

### Multiple Environment Variables

Set multiple environment variables:

```lua
function PLUGIN:BackendExecEnv(ctx)
    -- Add node_modules/.bin to PATH for npm-installed binaries
    local bin_path = ctx.install_path .. "/node_modules/.bin"
    return {
        env_vars = {
            {key = "PATH", value = bin_path},
            {key = "EXAMPLE_TOOL_HOME", value = ctx.install_path},
            {key = "EXAMPLE_TOOL_VERSION", value = ctx.version}
        }
    }
end
```

## Performance Optimization

### Caching

mise caches remote version lists and tool environment results. During development, use
`mise cache clear my-plugin:some-tool` when a cached result hides a hook change. A Lua table
only caches within that Lua runtime; it does not persist across separate mise invocations.
See [cache behavior](/cache-behavior.html) and the [Lua modules reference](/plugin-lua-modules.html#caching).

## Next Steps

- [Start with the backend plugin template](https://github.com/jdx/mise-backend-plugin-template)
- [Learn about Tool Plugin Development](tool-plugin-development.md)
- [Explore available Lua modules](plugin-lua-modules.md)
- [Publish your plugin](plugin-publishing.md)
- [View the vfox-npm plugin source](https://github.com/jdx/vfox-npm)
