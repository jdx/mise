# Environment Plugin Development

Environment plugins are a special type of mise plugin that provide environment variables and PATH modifications without managing tool versions. They're ideal for integrating external services, managing secrets, and standardizing environment configuration across teams.

Unlike [tool plugins](tool-plugin-development.md) and [backend plugins](backend-plugin-development.md), environment plugins:

- ✅ Don't implement version management (`Available`, `PreInstall`, `PostInstall` hooks)
- ✅ Only implement environment hooks (`MiseEnv`, `MisePath`)
- ✅ Are configured via `env._.<plugin-name>` syntax
- ✅ Can accept configuration options as TOML values
- ✅ Execute on every environment activation

## Quick Start

The fastest way to create an environment plugin is to use the [mise-env-plugin-template](https://github.com/jdx/mise-env-plugin-template).

::: tip
The [mise-env-plugin-template](https://github.com/jdx/mise-env-plugin-template) provides a ready-to-use starting point with LuaCATS type definitions, stylua formatting, and hk linting pre-configured.
:::

To get started:

```bash
# Clone the template
git clone https://github.com/jdx/mise-env-plugin-template my-env-plugin
cd my-env-plugin

# Customize for your use case
# Edit metadata.lua, hooks/mise_env.lua, hooks/mise_path.lua
```

## Plugin Structure

Environment plugins are implemented in Lua (version 5.1 at the moment). A minimal environment plugin has this structure:

```
my-env-plugin/
├── metadata.lua           # Plugin metadata
└── hooks/
    ├── mise_env.lua      # Returns environment variables (required)
    └── mise_path.lua     # Returns PATH entries (optional)
```

### metadata.lua

The `metadata.lua` file defines your plugin's basic information:

```lua
PLUGIN = {}

--- Plugin name (required)
PLUGIN.name = "my-env-plugin"

--- Plugin version (required)
PLUGIN.version = "1.0.0"

--- Plugin description (required)
PLUGIN.description = "Provides environment variables for my service"

--- Plugin homepage (optional)
PLUGIN.homepage = "https://github.com/username/my-env-plugin"

--- Plugin license (optional)
PLUGIN.license = "MIT"

--- Minimum mise/vfox version required (optional)
PLUGIN.minRuntimeVersion = "0.3.0"
```

### hooks/mise_env.lua

The `MiseEnv` hook returns environment variables to set:

```lua
function PLUGIN:MiseEnv(ctx)
    -- Access configuration from mise.toml via ctx.options
    local api_url = ctx.options.api_url or "https://api.example.com"
    local debug = ctx.options.debug or false

    -- Return array of environment variables
    return {
        {
            key = "API_URL",
            value = api_url
        },
        {
            key = "DEBUG",
            value = tostring(debug)
        },
        {
            key = "SERVICE_TOKEN",
            value = get_token_from_somewhere()  -- Your custom logic
        }
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
function PLUGIN:MiseEnv(ctx)
    local config_path = ctx.options.config_file or "config.json"
    local config = load_config(config_path)

    return {
        cacheable = true,
        watch_files = {config_path},
        env = {
            {key = "API_URL", value = config.api_url},
            {key = "API_KEY", value = config.api_key}
        }
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

### hooks/mise_path.lua

The `MisePath` hook returns directories to add to PATH (optional):

```lua
function PLUGIN:MisePath(ctx)
    -- Return array of paths to prepend to PATH
    local paths = {
        "/opt/my-service/bin"
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
_.my-env-plugin = {
  api_url = "https://prod.api.example.com",
  debug = false,
  custom_bin_path = "/custom/path/bin",
}
```

All fields in the TOML table are passed to your hooks as `ctx.options`.

## Complete Example: Secret Manager Plugin

Here's a complete example of a plugin that fetches secrets from an external service:

**metadata.lua**:

```lua
PLUGIN = {}
PLUGIN.name = "vault-secrets"
PLUGIN.version = "1.0.0"
PLUGIN.description = "Fetch secrets from HashiCorp Vault"
PLUGIN.minRuntimeVersion = "0.3.0"
```

**hooks/mise_env.lua**:

```lua
local http = require("http")
local json = require("json")

function PLUGIN:MiseEnv(ctx)
    local vault_url = ctx.options.vault_url or error("vault_url required")
    local secrets_path = ctx.options.secrets_path or error("secrets_path required")
    local vault_token = os.getenv("VAULT_TOKEN") or error("VAULT_TOKEN not set")

    -- Fetch secrets from Vault
    local url = vault_url .. "/v1/" .. secrets_path
    local response = http.get({
        url = url,
        headers = {
            ["X-Vault-Token"] = vault_token
        }
    })

    if response.status_code ~= 200 then
        error("Failed to fetch secrets: " .. response.status_code)
    end

    local data = json.decode(response.body)
    local env_vars = {}

    -- Convert Vault secrets to environment variables
    for key, value in pairs(data.data.data) do
        table.insert(env_vars, {
            key = key,
            value = value
        })
    end

    return env_vars
end
```

**Usage in mise.toml**:

```toml
[env]
_.vault-secrets = {
  vault_url = "https://vault.example.com",
  secrets_path = "secret/data/myapp/production",
}
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
function PLUGIN:MiseEnv(ctx)
    local api_url = ctx.options.api_url or "https://api.example.com"
    local timeout = ctx.options.timeout or 30

    -- ...
end
```

### 2. Validate Required Options

```lua
function PLUGIN:MiseEnv(ctx)
    if not ctx.options.api_key then
        error("api_key is required in mise.toml configuration")
    end

    -- ...
end
```

### 3. Handle Errors Gracefully

```lua
function PLUGIN:MiseEnv(ctx)
    local response = http.get({url = ctx.options.api_url})

    if response.status_code ~= 200 then
        error("API request failed: " .. response.status_code .. " - " .. response.body)
    end

    -- ...
end
```

### 4. Use Built-in Caching for Expensive Operations

For plugins that fetch data from external services, use mise's built-in caching by returning the extended format with `cacheable = true`:

```lua
function PLUGIN:MiseEnv(ctx)
    local config_file = ctx.options.config_file or "secrets.json"

    -- Fetch secrets (mise will cache the result)
    local secrets = fetch_secrets(ctx.options)

    return {
        cacheable = true,
        watch_files = {config_file},  -- Re-fetch if config changes
        env = secrets
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
function PLUGIN:MiseEnv(ctx)
    local env_name = ctx.options.environment or "development"

    -- Load different config based on environment
    local config = load_config(env_name)

    return {
        {key = "ENV", value = env_name},
        {key = "API_URL", value = config.api_url},
        -- ...
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

- [mise-env-sample](https://github.com/jdx/mise-env-plugin-template) - Simple example showing basic usage
- The [mise-plugins](https://github.com/mise-plugins) organization currently hosts tool plugins only—add your environment plugin there (or share it with the community) so others can learn from more examples

## Migration from Tool Plugins

If you have an existing tool plugin that only sets environment variables, you can simplify it to an environment-only plugin:

**Before** (tool plugin with unused hooks):

```
my-plugin/
├── metadata.lua
└── hooks/
    ├── available.lua        # Returns empty list
    ├── pre_install.lua      # Not used
    ├── post_install.lua     # Not used
    └── env_keys.lua         # Actually sets env vars
```

**After** (environment plugin):

```
my-plugin/
├── metadata.lua
└── hooks/
    └── mise_env.lua         # Clean and focused
```

## Related Documentation

- [Plugin Overview](/plugins.html) - Overview of all plugin types
- [Tool Plugin Development](/tool-plugin-development.html) - For plugins that manage tool versions
- [Backend Plugin Development](/backend-plugin-development.html) - For multi-tool backends
- [Plugin Lua Modules](/plugin-lua-modules.html) - Available Lua APIs
- [Plugin Publishing](/plugin-publishing.html) - Publishing your plugin
- [Environment Variables](/environments/) - How mise manages environments
