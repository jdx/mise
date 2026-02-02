# Environment Plugin Development

Environment plugins are a special type of mise plugin that provide environment variables and PATH modifications without managing tool versions. They're ideal for integrating external services, managing secrets, and standardizing environment configuration across teams.

> [!TIP]
> The fastest way to get started is with the [mise-env-plugin-template](https://github.com/jdx/mise-env-plugin-template) repository.

Unlike [tool plugins](tool-plugin-development.md) and [backend plugins](backend-plugin-development.md), environment plugins:

- Don't implement version management (`Available`, `PreInstall`, `PostInstall` hooks)
- Only implement environment hooks (`MiseEnv`, `MisePath`)
- Are configured via `env._.<plugin-name>` syntax
- Can accept configuration options as TOML values
- Execute on every environment activation

## Quick Start

The fastest way to create an environment plugin is to use the [mise-env-plugin-template](https://github.com/jdx/mise-env-plugin-template):

```bash
# Clone the template
git clone https://github.com/jdx/mise-env-plugin-template my-env-plugin
cd my-env-plugin

# Customize for your use case
# Edit metadata.luau, hooks/mise_env.luau, hooks/mise_path.luau
```

## Plugin Structure

Environment plugins are implemented in [Luau](https://luau.org/), a fast, small, safe, gradually typed embeddable scripting language derived from Lua. A minimal environment plugin has this structure:

```
my-env-plugin/
├── metadata.luau          # Plugin metadata
├── .luaurc                # Luau type checking configuration
├── lib/
│   └── types.luau         # Type definitions
└── hooks/
    ├── mise_env.luau      # Returns environment variables (required)
    └── mise_path.luau     # Returns PATH entries (optional)
```

## Type Definitions

Create a `lib/types.luau` file for shared type definitions:

```lua
--!strict
export type EnvResult = {
    key: string,
    value: string,
}

export type MiseEnvContext = {
    options: { [string]: any },
}

export type MiseEnvReturn = { EnvResult } | {
    cacheable: boolean?,
    watch_files: { string }?,
    env: { EnvResult },
}

export type PluginType = {
    MiseEnv: (self: PluginType, ctx: MiseEnvContext) -> MiseEnvReturn,
    MisePath: (self: PluginType, ctx: MiseEnvContext) -> { string },
}

export type HttpModule = {
    get: (opts: { url: string, headers: { [string]: string }? }) -> ({ status_code: number, headers: { [string]: string }, body: string }, string?),
}

export type JsonModule = {
    encode: (value: any) -> string,
    decode: (str: string) -> any,
}

export type FileModule = {
    read: (path: string) -> string?,
    exists: (path: string) -> boolean,
}

export type CmdModule = {
    exec: (command: string, opts: { cwd: string?, env: { [string]: string }? }?) -> string,
}

export type EnvModule = {
    getenv: (key: string) -> string?,
    setenv: (key: string, value: string) -> (),
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

### metadata.luau

The `metadata.luau` file defines your plugin's basic information:

```lua
-- metadata.luau
PLUGIN = {
    name = "my-env-plugin",
    version = "1.0.0",
    description = "Provides environment variables for my service",
    homepage = "https://github.com/username/my-env-plugin",
    license = "MIT",
    minRuntimeVersion = "0.3.0",
}
```

### hooks/mise_env.luau

The `MiseEnv` hook returns environment variables to set:

```lua
--!strict
-- hooks/mise_env.luau
local Types = require("@lib/types")

local plugin = PLUGIN :: Types.PluginType

function plugin:MiseEnv(ctx: Types.MiseEnvContext): Types.MiseEnvReturn
    -- Access configuration from mise.toml via ctx.options
    local api_url = ctx.options.api_url or "https://api.example.com"
    local debug = ctx.options.debug or false

    -- Return array of environment variables
    return {
        {
            key = "API_URL",
            value = api_url,
        },
        {
            key = "DEBUG",
            value = tostring(debug),
        },
    }
end
```

::: tip
When `cmd.exec()` is called from `MiseEnv` or `MisePath` hooks, it inherits the mise-constructed environment — including `_.path` entries and environment variables from preceding directives. If the module directive is configured with `tools = true` (e.g., `_.my-plugin = { tools = true }`), tool installation bin paths are also included, so mise-managed tools are directly callable (e.g., `cmd.exec("node --version")`).
:::

**Return value**: Either a simple array of env keys, or a table with caching metadata.

Simple format - array of tables, each with:

- `key` (string, required): Environment variable name
- `value` (string, required): Environment variable value

Extended format - table with:

- `env` (array, required): Array of `{key, value}` tables (same as simple format)
- `cacheable` (boolean, optional): If `true`, mise can cache this plugin's output. Default: `false`
- `watch_files` (array of strings, optional): File paths to watch for changes. If any file's mtime changes, the cache is invalidated.

Example using extended format with caching:

```lua
--!strict
-- hooks/mise_env.luau
local Types = require("@lib/types")
local file = require("file") :: Types.FileModule
local json = require("json") :: Types.JsonModule

local plugin = PLUGIN :: Types.PluginType

local function load_config(config_path: string): { api_url: string, api_key: string }
    local content = file.read(config_path)
    if not content then
        error(`Failed to read config file: {config_path}`)
    end
    return json.decode(content)
end

function plugin:MiseEnv(ctx: Types.MiseEnvContext): Types.MiseEnvReturn
    local config_path = ctx.options.config_file or "config.json"
    local config = load_config(config_path)

    return {
        cacheable = true,
        watch_files = { config_path },
        env = {
            { key = "API_URL", value = config.api_url },
            { key = "API_KEY", value = config.api_key },
        },
    }
end
```

When `cacheable = true`, mise will cache the environment variables and only re-execute the plugin when:

- Any file in `watch_files` changes
- The mise configuration changes
- The cache TTL expires (configured via `env_cache_ttl` setting)

::: tip
For caching to work, users must enable the `env_cache` setting:

```toml
# ~/.config/mise/config.toml
[settings]
env_cache = true
```

:::

### hooks/mise_path.luau

The `MisePath` hook returns directories to add to PATH (optional):

```lua
--!strict
-- hooks/mise_path.luau
local Types = require("@lib/types")

local plugin = PLUGIN :: Types.PluginType

function plugin:MisePath(ctx: Types.MiseEnvContext): { string }
    -- Return array of paths to prepend to PATH
    local paths: { string } = {
        "/opt/my-service/bin",
    }

    -- Optionally add user-configured path
    if ctx.options.custom_bin_path then
        table.insert(paths, ctx.options.custom_bin_path)
    end

    return paths
end
```

**Return value**: Array of strings (directory paths)

## Context Object

Both hooks receive a `ctx` parameter with:

- **`ctx.options`**: TOML table of user configuration from `mise.toml`

For environment plugins, `ctx.options` is the primary way to accept user configuration.

## Configuration in mise.toml

Users configure environment plugins using the `env._` directive:

Simple activation with no options:

```toml
[env]
_.my-env-plugin = {}
```

With configuration options:

```toml
[env]
_.my-env-plugin = { api_url = "https://prod.api.example.com", debug = false, custom_bin_path = "/custom/path/bin" }
```

All fields in the TOML table are passed to your hooks as `ctx.options`.

## Complete Example: Secret Manager Plugin

Here's a complete example of a plugin that fetches secrets from an external service:

**metadata.luau**:

```lua
-- metadata.luau
PLUGIN = {
    name = "vault-secrets",
    version = "1.0.0",
    description = "Fetch secrets from HashiCorp Vault",
    minRuntimeVersion = "0.3.0",
}
```

**hooks/mise_env.luau**:

```lua
--!strict
-- hooks/mise_env.luau
local Types = require("@lib/types")
local http = require("http") :: Types.HttpModule
local json = require("json") :: Types.JsonModule
local env = require("env") :: Types.EnvModule

local plugin = PLUGIN :: Types.PluginType

function plugin:MiseEnv(ctx: Types.MiseEnvContext): Types.MiseEnvReturn
    local vault_url = ctx.options.vault_url
    if not vault_url then
        error("vault_url required")
    end

    local secrets_path = ctx.options.secrets_path
    if not secrets_path then
        error("secrets_path required")
    end

    local vault_token = env.getenv("VAULT_TOKEN")
    if not vault_token then
        error("VAULT_TOKEN not set")
    end

    -- Fetch secrets from Vault
    local url = `{vault_url}/v1/{secrets_path}`
    local response, err = http.get({
        url = url,
        headers = {
            ["X-Vault-Token"] = vault_token,
        },
    })

    if err then
        error(`Failed to fetch secrets: {err}`)
    end

    if response.status_code ~= 200 then
        error(`Failed to fetch secrets: {response.status_code}`)
    end

    local data = json.decode(response.body)
    local env_vars: { Types.EnvResult } = {}

    -- Convert Vault secrets to environment variables
    for key, value in pairs(data.data.data) do
        table.insert(env_vars, {
            key = key,
            value = value,
        })
    end

    return env_vars
end
```

**Usage in mise.toml**:

```toml
[env]
_.vault-secrets = { vault_url = "https://vault.example.com", secrets_path = "secret/data/myapp/production" }
```

## Available Lua Modules

Environment plugins have access to mise's built-in Lua modules:

- **`http`**: Make HTTP requests
- **`json`**: Encode/decode JSON
- **`file`**: Read/write files
- **`cmd`**: Execute shell commands
- **`strings`**: String manipulation utilities
- **`env`**: Access environment variables

See [Plugin Lua Modules](/plugin-lua-modules.html) for complete documentation.

## Best Practices

### 1. Provide Sensible Defaults

```lua
--!strict
local Types = require("@lib/types")

local plugin = PLUGIN :: Types.PluginType

function plugin:MiseEnv(ctx: Types.MiseEnvContext): Types.MiseEnvReturn
    local api_url = ctx.options.api_url or "https://api.example.com"
    local timeout = ctx.options.timeout or 30

    -- ...
    return {}
end
```

### 2. Validate Required Options

```lua
--!strict
local Types = require("@lib/types")

local plugin = PLUGIN :: Types.PluginType

function plugin:MiseEnv(ctx: Types.MiseEnvContext): Types.MiseEnvReturn
    if not ctx.options.api_key then
        error("api_key is required in mise.toml configuration")
    end

    -- ...
    return {}
end
```

### 3. Handle Errors Gracefully

```lua
--!strict
local Types = require("@lib/types")
local http = require("http") :: Types.HttpModule

local plugin = PLUGIN :: Types.PluginType

function plugin:MiseEnv(ctx: Types.MiseEnvContext): Types.MiseEnvReturn
    local response, err = http.get({ url = ctx.options.api_url })

    if err then
        error(`HTTP request failed: {err}`)
    end

    if response.status_code ~= 200 then
        error(`API request failed: {response.status_code} - {response.body}`)
    end

    -- ...
    return {}
end
```

### 4. Use Built-in Caching for Expensive Operations

For plugins that fetch data from external services, use mise's built-in caching by returning the extended format with `cacheable = true`:

```lua
--!strict
local Types = require("@lib/types")

local plugin = PLUGIN :: Types.PluginType

local function fetch_secrets(options: { [string]: any }): { Types.EnvResult }
    -- Fetch secrets from external service
    return {}
end

function plugin:MiseEnv(ctx: Types.MiseEnvContext): Types.MiseEnvReturn
    local config_file = ctx.options.config_file or "secrets.json"

    -- Fetch secrets (mise will cache the result)
    local secrets = fetch_secrets(ctx.options)

    return {
        cacheable = true,
        watch_files = { config_file }, -- Re-fetch if config changes
        env = secrets,
    }
end
```

This is preferred over manual caching because:

- mise handles cache invalidation automatically
- Cache is encrypted with session-scoped keys
- Integrates with `mise cache clear` and `mise cache prune`
- Respects the `env_cache_ttl` setting

Note: Users must enable `env_cache = true` in their settings for caching to work.

### 5. Support Multiple Environments

```lua
--!strict
local Types = require("@lib/types")

local plugin = PLUGIN :: Types.PluginType

local function load_config(env_name: string): { api_url: string }
    -- Load different config based on environment
    return { api_url = `https://{env_name}.api.example.com` }
end

function plugin:MiseEnv(ctx: Types.MiseEnvContext): Types.MiseEnvReturn
    local env_name = ctx.options.environment or "development"

    -- Load different config based on environment
    local config = load_config(env_name)

    return {
        { key = "ENV", value = env_name },
        { key = "API_URL", value = config.api_url },
    }
end
```

## Testing Your Plugin

### Local Testing

1. Link your plugin for development:

```bash
mise plugin link my-env-plugin /path/to/my-env-plugin
```

2. Configure it in `mise.toml`:

```toml
[env]
_.my-env-plugin = { test_option = "value" }
```

3. Test the environment:

```bash
# See environment variables
mise env | grep MY_

# Run a command with the environment
mise exec -- env | grep MY_

# Debug with MISE_DEBUG
MISE_DEBUG=1 mise env
```

### Common Issues

**Plugin not found**: Make sure you've installed/linked the plugin:

```bash
mise plugin ls
```

**Hook not executing**: Enable debug logging:

```bash
MISE_DEBUG=1 mise env
```

**Options not passed**: Verify TOML syntax in `mise.toml`:

```toml
[env]
# Correct: TOML table
_.my-plugin = { key = "value" }

# Wrong: String value
_.my-plugin = "value"  # This won't work
```

## Publishing Your Plugin

Once your environment plugin is ready:

1. **Create a GitHub repository** for your plugin
2. **Add a README** with usage instructions
3. **Tag releases** following semantic versioning
4. (Optional) share the repository URL so others can install it directly with `mise plugin install`.

See [Plugin Publishing](/plugin-publishing.html) for detailed instructions.

## Examples

- [mise-env-plugin-template](https://github.com/jdx/mise-env-plugin-template) - Template for creating environment plugins
- The [mise-plugins](https://github.com/mise-plugins) organization currently hosts tool plugins only—add your environment plugin there (or share it with the community) so others can learn from more examples

## Migration from Tool Plugins

If you have an existing tool plugin that only sets environment variables, you can simplify it to an environment-only plugin:

**Before** (tool plugin with unused hooks):

```
my-plugin/
├── metadata.luau
└── hooks/
    ├── available.luau       # Returns empty list
    ├── pre_install.luau     # Not used
    ├── post_install.luau    # Not used
    └── env_keys.luau        # Actually sets env vars
```

**After** (environment plugin):

```
my-plugin/
├── metadata.luau
├── .luaurc
├── lib/
│   └── types.luau
└── hooks/
    └── mise_env.luau        # Clean and focused
```

## Related Documentation

- [Plugin Overview](/plugins.html) - Overview of all plugin types
- [Tool Plugin Development](/tool-plugin-development.html) - For plugins that manage tool versions
- [Backend Plugin Development](/backend-plugin-development.html) - For multi-tool backends
- [Plugin Lua Modules](/plugin-lua-modules.html) - Available Lua APIs
- [Plugin Publishing](/plugin-publishing.html) - Publishing your plugin
- [Environment Variables](/environments/) - How mise manages environments
