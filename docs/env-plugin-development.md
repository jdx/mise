# Environment Plugin Development

Environment plugins return variables and PATH entries without installing a versioned tool.
Use them for an external configuration service, secret manager, or team environment. They
run while mise constructs an environment, so keep their hooks fast and non-interactive.
Their execution frequency depends on environment caching and the command being run.

For installation lifecycles, use a [tool](/tool-plugin-development.html) or
[backend](/backend-plugin-development.html) plugin instead.

## Quick Start

Start from the [environment plugin template](https://github.com/jdx/mise-env-plugin-template),
or create the files below. Link the directory before referencing the directive:

```sh
mise plugin link my-env-plugin /path/to/my-env-plugin
```

```toml
[env]
_.my-env-plugin = {
  api_url = "https://api.example.com",
  debug = false,
}
```

Check the result with `mise env --json` or run a command through `mise exec`. Environment
output may contain secrets, so inspect it locally and avoid pasting it into logs or issues.

## Plugin Structure

```text
my-env-plugin/
├── metadata.lua
└── hooks/
    ├── mise_env.lua   # variables
    └── mise_path.lua  # optional PATH entries
```

Plugins use mise's embedded Lua 5.1 runtime. Environment hooks are mise extensions; do not
assume an upstream vfox installation will invoke them.

### metadata.lua

```lua
PLUGIN = {
    name = "my-env-plugin",
    version = "1.0.0",
    description = "Provide service configuration",
    author = "Plugin Author",
}
```

Keep metadata declarative. Document and test the required mise version in your README and
CI; a `minRuntimeVersion` field is not a mise-version compatibility check.

### hooks/mise_env.lua

A minimal working hook returns an array of key/value entries:

```lua
function PLUGIN:MiseEnv(ctx)
    return {
        {key = "API_URL", value = ctx.options.api_url or "https://api.example.com"},
        {key = "DEBUG", value = tostring(ctx.options.debug or false)},
    }
end
```

Keys and values must be strings. To provide cache and redaction metadata, return a table:

```lua
function PLUGIN:MiseEnv(ctx)
    local file = require("file")
    local json = require("json")
    local path = file.join_path(ctx.config_root, ctx.options.config_file or "service.json")
    local config = json.decode(file.read(path))
    assert(type(config.api_url) == "string", "service.json must contain a string api_url")
    return {
        cacheable = true,
        watch_files = {path},
        env = {{key = "API_URL", value = config.api_url}},
    }
end
```

This example treats `config_file` as relative to the config root. Define and document a
separate policy if your plugin also accepts absolute paths.

| Field         | Meaning                                                                                                       |
| ------------- | ------------------------------------------------------------------------------------------------------------- |
| `env`         | Array of `{key, value}` entries; omitted means no variables                                                   |
| `cacheable`   | Whether mise may cache this output; defaults to `false`                                                       |
| `watch_files` | Files whose modification times participate in cache validation; relative entries resolve from the config root |
| `redact`      | Request redaction of returned values in mise's processed output; defaults to `false`                          |

A user's explicit directive-level `redact` option overrides the plugin's preference.
Redaction does not remove values from the environment and raw task output bypasses it.
See [redactions](/environments/#redactions).

Caching requires the global `env_cache` setting. The cache is session-keyed and has a TTL;
file watching does not detect a changed value in a remote service. There are also limitations
when cached environments are inherited by nested mise invocations. Do not promise immediate
refresh of secrets merely because `cacheable = false` or `watch_files` is present. Use
`MISE_ENV_CACHE=0` when current values are required; see [cache behavior](/cache-behavior.html).

### hooks/mise_path.lua

Return an array of directory paths. For project-relative configuration, resolve paths
against `ctx.config_root`, not the process's current working directory:

```lua
function PLUGIN:MisePath(ctx)
    local file = require("file")
    if not ctx.options.bin_dir then
        return {}
    end
    return {file.join_path(ctx.config_root, ctx.options.bin_dir)}
end
```

This example accepts a relative `bin_dir`. The hook returns directories to add to PATH,
not a full PATH string. Return only existing directories your integration needs.

## Context Object

Both hooks receive `ctx.options`, containing directive configuration as typed TOML values,
and `ctx.config_root`, the root associated with the declaring config file. Resolve local
input files from that root so invoking mise from a subdirectory produces the same result.

`os.getenv` and `cmd.exec` see the mise-constructed environment, including preceding
directives and `_.path` entries. To expose configured tool binaries, use `tools = true`:

```toml
[tools]
node = "24"

[env]
_.my-env-plugin = { tools = true }
```

This runs the directive in the tool-aware phase. It does not declare which external programs
your plugin requires; document those prerequisites for users.

## Configuration in mise.toml

An empty table invokes a plugin without custom options:

```toml
[env]
_.my-env-plugin = {}
```

Use a TOML table for options. mise supports TOML 1.1 multiline inline tables, comments, and
trailing commas:

```toml
[env]
_.my-env-plugin = {
  # Relative to the file's configuration root.
  config_file = "service.json",
  bin_dir = "bin",
}
```

Reserve mise's directive controls, such as `tools` and `redact`, for their documented
meaning. Do not repurpose them as unrelated plugin options.

## Complete Example: Secret Manager Plugin

This hook reads string-valued secrets from a [HashiCorp Vault KV v2](https://developer.hashicorp.com/vault/api-docs/secret/kv/kv-v2#read-secret-version) response. It requires
a preexisting `VAULT_TOKEN` with permission to read the selected path. It does not implement
token login/renewal, namespaces, or other Vault secret engines.

**metadata.lua**:

```lua
PLUGIN = {
    name = "vault-secrets",
    version = "1.0.0",
    description = "Read Vault KV v2 secrets",
}
```

**hooks/mise_env.lua**:

```lua
local http = require("http")
local json = require("json")

function PLUGIN:MiseEnv(ctx)
    local vault_url = ctx.options.vault_url or error("vault_url is required")
    local secrets_path = ctx.options.secrets_path or error("secrets_path is required")
    local token = os.getenv("VAULT_TOKEN") or error("VAULT_TOKEN is not set")
    local response = http.get({
        url = vault_url:gsub("/+$", "") .. "/v1/" .. secrets_path,
        headers = {["X-Vault-Token"] = token},
    })
    if response.status_code ~= 200 then
        error("Vault request failed with HTTP " .. response.status_code)
    end
    local payload = json.decode(response.body)
    local data = payload.data and payload.data.data
    assert(type(data) == "table", "Expected a Vault KV v2 data response")
    local variables = {}
    for key, value in pairs(data) do
        assert(key:match("^[%a_][%w_]*$"), "Secret key is not an environment variable name")
        assert(type(value) == "string", "Secret values must be strings")
        table.insert(variables, {key = key, value = value})
    end
    return {env = variables, cacheable = false, redact = true}
end
```

Install or link this plugin as `vault-secrets`, then configure the endpoint and KV v2 API
path. Use an HTTPS endpoint you trust to receive the token:

```toml
[env]
_.vault-secrets = {
  vault_url = "https://vault.example.com",
  secrets_path = "secret/data/myapp/production",
}
```

The hook returns unmasked values to child processes. Redaction only affects supported mise
output processing. Account for the cache limitations above when defining secret freshness.

## Available Lua Modules

Use the [Lua modules reference](/plugin-lua-modules.html) for HTTP, JSON, files, commands,
strings, and logging. `cmd.exec` invokes a shell; prefer direct file/HTTP operations when
possible and never interpolate an untrusted option into a command string.

## Best Practices

Validate required options before a request and reject malformed responses with a useful
error that omits credentials and secret values. Provide defaults only when they have a clear
meaning. Avoid interactive login during shell activation; explain authentication setup in
the plugin README.

Return environment values through the hook. `env.setenv` changes the mise process itself;
it is not the mechanism for returning variables to the user's shell.

### 4. Use Built-in Caching for Expensive Operations

Opt into caching only when a stale result is acceptable for the configured TTL. List local
inputs in `watch_files`, and test refresh from an inherited shell session as well as a fresh
process. A local Lua table is not a persistent cache across mise invocations.

## Testing Your Plugin

### Local Testing

Test from an isolated configuration/data directory, using the workflow in
[Plugin Publishing](/plugin-publishing.html#testing-before-publication). Cover at least:

- A minimal directive and each supported option.
- Invocation from a subdirectory, including file and PATH resolution.
- Missing credentials, non-200 HTTP responses, and malformed payloads.
- The `tools = true` phase if the plugin invokes a configured tool.
- Fresh and cached environments when cache metadata is returned.

### Common Issues

Use `mise plugins ls` to confirm the plugin name matches the directive. Check the TOML
shape: `_.my-plugin = { key = "value" }` is a table; a string value is not the same interface.
Use `MISE_DEBUG=1 mise env` locally for hook failures, taking care with secret-bearing output.

If the hook is not running, check for safe mode or a cached environment. If a command is
missing, confirm its prerequisite and whether the directive needs `tools = true`.

## Publishing Your Plugin

Document the configuration fields, required credentials, API scope, supported platforms,
and cache/redaction behavior. Publish a Git repository and share its URL; a registry
shorthand is not required. See [Plugin Publishing](/plugin-publishing.html).

## Examples

Start with the [environment template](https://github.com/jdx/mise-env-plugin-template) and
adapt the working hooks above. Treat a third-party example as code to review, not as an
assurance that its service or authentication behavior matches your environment.

## Migration from Tool Plugins

Move environment-only behavior from `EnvKeys` into `MiseEnv`, add a directive under `[env]`,
and remove the artificial tool version/install hooks. Use `MisePath` for PATH entries.
This changes activation from a selected tool version to an explicit environment directive;
document the configuration migration for existing users.

## Related Documentation

- [Plugin Overview](/plugins.html).
- [Tool Plugin Development](/tool-plugin-development.html).
- [Backend Plugin Development](/backend-plugin-development.html).
- [Plugin Lua Modules](/plugin-lua-modules.html).
- [Environment Variables](/environments/).
