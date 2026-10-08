---
description: "Write a Lua plugin that sets environment variables and PATH entries from an [env] directive."
---

# Environment plugins

An environment plugin adds a directive to `[env]` whose Lua hooks return
environment variables and `PATH` entries. Use one to load configuration from a
service or a file format that mise does not read; it installs nothing.

mise calls these hooks whenever it builds the environment, which can be many
times in one shell session, so keep them fast and never prompt. For a tool with
versions, write a [tool plugin](/tool-plugin-development.html) or a
[backend plugin](/backend-plugin-development.html) instead.

## Quick start {#quick-start}

Create a repository from the
[environment plugin template](https://github.com/jdx/mise-env-plugin-template),
either with **Use this template** on GitHub or with the GitHub CLI:

```sh
gh repo create mise-my-env-plugin --template jdx/mise-env-plugin-template --public --clone
```

An environment plugin has this layout:

```text
mise-my-env-plugin/
├── metadata.lua
└── hooks/
    ├── mise_env.lua    # environment variables
    └── mise_path.lua   # PATH entries
```

A minimal plugin needs `metadata.lua` and one hook:

```lua [metadata.lua]
PLUGIN = {
    name = "my-env-plugin",
    version = "0.1.0",
    description = "Set the API URL for this project",
}
```

```lua [hooks/mise_env.lua]
function PLUGIN:MiseEnv(ctx)
    return {
        { key = "API_URL", value = ctx.options.api_url or "https://api.example.com" },
    }
end
```

Link the plugin before you add its directive. When a directive names a plugin
that is not installed and mise has no source to install it from (a registry
entry, a `[plugins]` URL, or a name in `owner/repo` or URL form), mise warns,
skips that directive and loads the rest of `[env]`, and `mise doctor` reports
it. Once the plugin is linked or installed, the next command uses it:

```sh
mise plugins link my-env-plugin ./mise-my-env-plugin
```

Then add the directive and check the result:

```toml [mise.toml]
[env]
_.my-env-plugin = { api_url = "https://api.staging.example.com" }
```

```sh
mise exec -- printenv API_URL
# https://api.staging.example.com
```

## Complete example: a Vault secrets plugin {#complete-example}

Before you write a secrets plugin, check whether
[mise secrets](/environments/secrets/) already supports your store.

This plugin reads string secrets from a
[HashiCorp Vault KV v2](https://developer.hashicorp.com/vault/api-docs/secret/kv/kv-v2#read-secret-version)
path. It needs a `VAULT_TOKEN` that can read the path. It does not log in, renew
tokens, or support namespaces or other secret engines.

```lua [metadata.lua]
PLUGIN = {
    name = "vault-secrets",
    version = "1.0.0",
    description = "Read Vault KV v2 secrets",
}
```

```lua [hooks/mise_env.lua]
local http = require("http")
local json = require("json")

function PLUGIN:MiseEnv(ctx)
    local vault_url = ctx.options.vault_url or error("vault_url is required")
    assert(vault_url:match("^https://"), "vault_url must use HTTPS")
    local secrets_path = ctx.options.secrets_path or error("secrets_path is required")
    local token = os.getenv("VAULT_TOKEN") or error("VAULT_TOKEN is not set")
    local response = http.get({
        url = vault_url:gsub("/+$", "") .. "/v1/" .. secrets_path,
        headers = { ["X-Vault-Token"] = token },
    })
    if response.status_code ~= 200 then
        error("Vault request failed with HTTP " .. response.status_code)
    end
    local payload = json.decode(response.body)
    local data = payload.data and payload.data.data
    assert(type(data) == "table", "expected a Vault KV v2 data response")
    local variables = {}
    for key, value in pairs(data) do
        assert(key:match("^[%a_][%w_]*$"), "secret key is not an environment variable name")
        assert(type(value) == "string", "secret values must be strings")
        table.insert(variables, { key = key, value = value })
    end
    return { env = variables, cacheable = false, redact = true }
end
```

Link the plugin as `vault-secrets`, then point it at your Vault. Use an HTTPS
endpoint you trust with the token:

```toml [mise.toml]
[env]
_.vault-secrets = { vault_url = "https://vault.example.com", secrets_path = "secret/data/myapp/production" }
```

The hook returns `redact = true`, so mise redacts the values in task output and
its logs, and `cacheable = false`, so mise asks Vault again each time it builds
the environment. Programs still receive the real values.

## Hooks {#hooks}

| Hook       | File                  | Returns                      |
| ---------- | --------------------- | ---------------------------- |
| `MiseEnv`  | `hooks/mise_env.lua`  | Environment variables        |
| `MisePath` | `hooks/mise_path.lua` | Directories to add to `PATH` |

Implement either hook or both. A hook fails by raising an error with `error()`;
mise stops and shows the message. Keep credentials and secret values out of
error messages. Hooks can use the `http`, `json`, `file`, `cmd` and other
modules described in the [Plugin Lua reference](/plugin-lua-modules.html).

### Hook context {#context-object}

Both hooks receive the same `ctx`:

| `ctx` field   | Value                                                                                       |
| ------------- | ------------------------------------------------------------------------------------------- |
| `options`     | The directive's table, with TOML types kept: `debug = false` arrives as the boolean `false` |
| `config_root` | The [config root](/configuration.html#config-root) of the file that declares the directive  |

Resolve relative paths against `ctx.config_root`, not the current directory, so
that running mise from a subdirectory gives the same result. Unlike tool and
backend hooks, environment hooks receive booleans and numbers with their TOML
types.

`os.getenv` and `cmd.exec` see the environment mise has built so far, including
earlier directives and `_.path` entries. `cmd.exec` runs a shell, so pass option
values in its `env` option instead of building a command string from them. To see the configured tools as well,
the user adds `tools = true`, which runs the directive after the tools are on
`PATH`:

```toml [mise.toml]
[tools]
node = "24"

[env]
_.my-env-plugin = { tools = true }
```

`tools = true` does not install anything the plugin runs; list those programs in
your README.

### MiseEnv {#miseenv-hook}

Return a list of `{ key = ..., value = ... }` tables, or a table with these
fields to control caching and redaction:

| Field         | Value                                                                                              |
| ------------- | -------------------------------------------------------------------------------------------------- |
| `env`         | The list of `{ key, value }` tables; omit it to set nothing                                        |
| `cacheable`   | `true` lets mise cache the result; defaults to `false`. See [Caching](#caching)                    |
| `watch_files` | Files whose changes invalidate a cached result; relative paths resolve from `ctx.config_root`      |
| `redact`      | `true` marks the values as secrets for redaction; defaults to `false`. See [Redaction](#redaction) |

Keys and values must be strings. Returning nothing sets nothing. This hook reads
a JSON file next to the config file and lets mise cache the result until the
file changes:

```lua [hooks/mise_env.lua]
function PLUGIN:MiseEnv(ctx)
    local file = require("file")
    local json = require("json")
    local path = file.join_path(ctx.config_root, ctx.options.config_file or "service.json")
    local config = json.decode(file.read(path))
    assert(type(config.api_url) == "string", "service.json must contain a string api_url")
    return {
        cacheable = true,
        watch_files = { path },
        env = { { key = "API_URL", value = config.api_url } },
    }
end
```

Return variables from the hook. `env.setenv` changes the environment of the
mise process running the hook, not the environment mise returns.

### MisePath {#misepath-hook}

Return a list of directories to add to `PATH`, not a full `PATH` string:

```lua [hooks/mise_path.lua]
function PLUGIN:MisePath(ctx)
    local file = require("file")
    if not ctx.options.bin_dir then
        return {}
    end
    local path = file.join_path(ctx.config_root, ctx.options.bin_dir)
    local info = file.stat(path)
    if not info or not info.is_dir then
        return {}
    end
    return { path }
end
```

Return only directories that exist and that the integration needs.

## Options {#options}

Users pass options as the directive's table. An empty table runs the plugin
with no options:

```toml [mise.toml]
[env]
_.my-env-plugin = {}
```

mise removes `tools` and `redact` from the table before it calls your hooks, so
do not use those names for your own options. Do not name the plugin `path`,
`file`, `source` or `python`: those are built-in `_.` directives and never reach
a plugin. A string such as `_.my-env-plugin = "value"` reaches the plugin as a
string, not a table, so document the table form.

## Caching <Badge type="warning" text="experimental" /> {#caching}

Caching applies only when the user turns on the experimental
[`env_cache`](/configuration/settings.html#env_cache) setting
(`MISE_ENV_CACHE=1`); it is off by default. mise then reuses a cached result
until [`env_cache_ttl`](/configuration/settings.html#env_cache_ttl) expires, a
config file changes, the plugin changes, or a file in `watch_files` changes.

mise cannot see changes in a remote service, so return `cacheable = true` only
when a result up to one TTL old is acceptable. A directive whose values are
redacted is never cached, whatever `cacheable` says. Users who need fresh values
can set `MISE_ENV_CACHE=0`. See
[environment cache](/cache-behavior.html#environment-caching).

## Redaction {#redaction}

Return `redact = true` when the values are secrets. mise then replaces them with
`[redacted]` in the output it captures, such as task output and its own log
messages. A `redact` key in the user's directive overrides the plugin's choice.
Programs still receive the real values, and `mise env` prints them on purpose;
see [redaction](/environments/secrets/#redaction).

## Testing {#testing-your-plugin}

Test from an isolated configuration and data directory, as described in
[Publishing plugins](/plugin-publishing.html#testing-before-publication). Cover:

- a minimal directive and each option you support
- running mise from a subdirectory, for file and `PATH` resolution
- missing credentials, HTTP errors and malformed responses
- the `tools = true` case if the plugin runs a configured tool
- a fresh and a cached environment if the hook returns `cacheable = true`

Run `MISE_DEBUG=1 mise env` to see a hook's errors. The output can contain
secrets, so do not paste it into issues.

## Common mistakes {#common-mistakes}

| Symptom                                                                    | Cause and fix                                                                                                                                                  |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `skipping env plugin my-env-plugin: it is not installed and has no source` | The plugin is not installed under the directive's name. Link it, install it with `mise plugins install my-env-plugin <git-url>`, or list it under `[plugins]`. |
| The hook does not run                                                      | [Safe mode](/security.html#safe-mode) skips project `[env]` directives, and a cached environment can skip the hook.                                            |
| A program the hook runs is not found                                       | Install it, or add `tools = true` if it is a configured tool.                                                                                                  |
| A relative path resolves differently in a subdirectory                     | Join it with `ctx.config_root`.                                                                                                                                |
| Old values after a change in the service                                   | The result was cached. Return `cacheable = false`, or tell users to set `MISE_ENV_CACHE=0`.                                                                    |

Say in your README which mise version the plugin needs; `MiseEnv` and
`MisePath` are mise hooks that the vfox CLI does not run, and mise reads no
minimum-version field from `metadata.lua`.

## Migrate from a tool plugin {#migration-from-tool-plugins}

If a tool plugin exists only to set environment variables, move that logic from
`EnvKeys` into `MiseEnv`, return `PATH` entries from `MisePath`, and remove the
version and install hooks. Users then replace the `[tools]` entry with a
directive under `[env]`, so document that change for them.

To publish the plugin, see [Publishing plugins](/plugin-publishing.html), and
document its options, the credentials it needs, and its caching and redaction
behavior.
