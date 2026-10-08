---
description: "Write a Lua plugin that installs a family of tools under its own prefix, such as acme:deploy."
---

# Backend plugins

A backend plugin is a set of Lua hooks that installs many tools under one
prefix. Users write `my-backend:some-tool`, where `my-backend` is the name they
installed the plugin under. Write one for a package registry or an artifact
server that no built-in [backend](/dev-tools/backends/) supports.

Backend plugins are Lua 5.1 scripts in a `hooks/` directory, like
[tool plugins](/tool-plugin-development.html), but with their own hooks. A
plugin directory that has `hooks/backend_install.lua` is a backend plugin.

## Quick start {#quick-start}

Create a repository from the
[backend plugin template](https://github.com/jdx/mise-backend-plugin-template),
either with **Use this template** on GitHub or with the GitHub CLI:

```sh
gh repo create mise-acme --template jdx/mise-backend-plugin-template --public --clone
```

The template includes LuaCATS type definitions for editor completion, and runs
stylua, lua-language-server and actionlint through hk. A backend plugin has
this layout:

```text
mise-acme/
├── metadata.lua                  # name and version
├── hooks/
│   ├── backend_list_versions.lua # required
│   ├── backend_install.lua       # required
│   ├── backend_exec_env.lua      # required
│   ├── backend_uninstall.lua     # optional
│   ├── backend_list_tools.lua    # optional
│   └── backend_search_tools.lua  # optional
└── lib/                          # optional: modules for require()
```

Link your working copy and try a tool:

```sh
mise plugins link acme ./mise-acme
mise ls-remote acme:deploy
mise use acme:deploy@2.1.0
mise exec -- deploy --version
```

The prefix is the name you linked or installed the plugin under. Pick one that
does not clash with a built-in backend such as `npm` or `github`.

## Complete example {#complete-example}

This plugin installs tools from an internal artifact server that lists each
tool's versions, oldest first, in `index.json` and puts each release's archives
and a `SHA256SUMS` file in a directory named after the version:

```text
https://artifacts.example.com/deploy/index.json
https://artifacts.example.com/deploy/2.1.0/deploy-2.1.0-linux-x64.tar.gz
https://artifacts.example.com/deploy/2.1.0/SHA256SUMS
```

A `mirror` tool option replaces the server URL.

```lua [metadata.lua]
PLUGIN = {
    name = "acme",
    version = "0.1.0",
    description = "Install tools from the ACME artifact server",
}
```

```lua [lib/acme.lua]
local M = {}

function M.base_url(options)
    return options.mirror or "https://artifacts.example.com"
end

-- The server builds for Linux and macOS only.
function M.asset_name(tool, version)
    local os_names = { darwin = "macos", linux = "linux" }
    local arch_names = { amd64 = "x64", arm64 = "arm64" }
    local os_name = os_names[RUNTIME.osType] or error("unsupported OS: " .. RUNTIME.osType)
    local arch = arch_names[RUNTIME.archType] or error("unsupported architecture: " .. RUNTIME.archType)
    return tool .. "-" .. version .. "-" .. os_name .. "-" .. arch .. ".tar.gz"
end

function M.find_checksum(body, filename)
    for line in body:gmatch("[^\r\n]+") do
        local digest, name = line:match("^(%x+)%s+%*?(.+)$")
        if name == filename and #digest == 64 then
            return digest
        end
    end
    error("no SHA-256 for " .. filename)
end

-- The Lua modules have no hash function, so ask the system for one.
function M.sha256(path)
    local cmd = require("cmd")
    local command = RUNTIME.osType == "darwin" and 'shasum -a 256 "$FILE"' or 'sha256sum "$FILE"'
    return cmd.exec(command, { env = { FILE = path } }):match("^(%x+)")
end

return M
```

```lua [hooks/backend_list_versions.lua]
local http = require("http")
local json = require("json")
local acme = require("acme")

function PLUGIN:BackendListVersions(ctx)
    local url = acme.base_url(ctx.options) .. "/" .. ctx.tool .. "/index.json"
    local resp = http.get({ url = url })
    if resp.status_code == 404 then
        error("acme has no tool named " .. ctx.tool)
    elseif resp.status_code ~= 200 then
        error("fetching " .. url .. " failed: HTTP " .. resp.status_code)
    end
    -- index.json lists versions oldest first, the order mise expects
    return { versions = json.decode(resp.body).versions }
end
```

```lua [hooks/backend_install.lua]
local archiver = require("archiver")
local file = require("file")
local http = require("http")
local acme = require("acme")

function PLUGIN:BackendInstall(ctx)
    local dir = acme.base_url(ctx.options) .. "/" .. ctx.tool .. "/" .. ctx.version .. "/"
    local filename = acme.asset_name(ctx.tool, ctx.version)
    local archive = file.join_path(ctx.download_path, filename)

    local sums = http.get({ url = dir .. "SHA256SUMS" })
    if sums.status_code ~= 200 then
        error("fetching SHA256SUMS failed: HTTP " .. sums.status_code)
    end
    local expected = acme.find_checksum(sums.body, filename)

    http.download_file({ url = dir .. filename }, archive)
    if acme.sha256(archive) ~= expected then
        error("checksum mismatch for " .. filename)
    end

    archiver.decompress(archive, ctx.install_path, { strip_components = 1 })
    return {}
end
```

```lua [hooks/backend_exec_env.lua]
local file = require("file")

function PLUGIN:BackendExecEnv(ctx)
    return {
        env_vars = {
            { key = "PATH", value = file.join_path(ctx.install_path, "bin") },
        },
    }
end
```

Configure a tool and its mirror, then install it:

```toml [mise.toml]
[tools]
"acme:deploy" = { version = "2.1.0", mirror = "https://artifacts.internal.example.com" }
```

```sh
mise install
mise exec -- deploy --version
```

`mise install` calls `BackendListVersions` to resolve `2.1.0` and
`BackendInstall` to download, verify and extract the archive into
`~/.local/share/mise/installs/acme-deploy/2.1.0`. `BackendExecEnv` then puts its
`bin` directory on `PATH`. mise does not check what `BackendInstall` downloads,
so the hook verifies the checksum itself before extracting.

## Hooks {#backend-methods}

| Hook                  | File                              | Required | mise calls it                                                    |
| --------------------- | --------------------------------- | -------- | ---------------------------------------------------------------- |
| `BackendListVersions` | `hooks/backend_list_versions.lua` | yes      | to list a tool's versions (`mise ls-remote`, resolving `latest`) |
| `BackendInstall`      | `hooks/backend_install.lua`       | yes      | to install one version of a tool                                 |
| `BackendExecEnv`      | `hooks/backend_exec_env.lua`      | yes      | to build the environment for an installed version                |
| `BackendUninstall`    | `hooks/backend_uninstall.lua`     | no       | before mise removes an installed version                         |
| `BackendListTools`    | `hooks/backend_list_tools.lua`    | no       | for `mise search`, shell completion and the `mise use` picker    |
| `BackendSearchTools`  | `hooks/backend_search_tools.lua`  | no       | for `mise search` and shell completion with a query              |

A hook fails by raising an error with `error()`; mise stops and shows the
message. Hooks can use the `http`, `json`, `file`, `archiver`, `cmd` and other
modules in the [Plugin Lua reference](/plugin-lua-modules.html), and the
[`RUNTIME`](/plugin-lua-modules.html#runtime) global that describes the
platform.

### BackendListVersions

| `ctx` field | Value                                                 | Example                      |
| ----------- | ----------------------------------------------------- | ---------------------------- |
| `tool`      | The tool name after the prefix                        | `"deploy"`                   |
| `options`   | The tool's options; see [Tool options](#tool-options) | `{ mirror = "https://..." }` |

Return `{ versions = { ... } }`, a list of version strings oldest first,
according to the tool's release policy. mise keeps that order. Versions are not
always SemVer: they can be dates, prereleases or channel names. A tool plugin's
`Available` hook returns newest first instead.

### BackendInstall

| `ctx` field     | Value                                                 | Example                                                      |
| --------------- | ----------------------------------------------------- | ------------------------------------------------------------ |
| `tool`          | The tool name                                         | `"deploy"`                                                   |
| `version`       | The resolved version being installed                  | `"2.1.0"`                                                    |
| `install_path`  | The directory to install into                         | `"/home/user/.local/share/mise/installs/acme-deploy/2.1.0"`  |
| `download_path` | A directory for downloads                             | `"/home/user/.local/share/mise/downloads/acme-deploy/2.1.0"` |
| `options`       | The tool's options; see [Tool options](#tool-options) | `{ mirror = "https://..." }`                                 |

Install the version into `install_path` and return a table, even an empty one;
returning nothing fails the install. `http.download_file` and
`archiver.decompress` create the directories they write into.

mise does not verify what `BackendInstall` downloads, records no download URL
or checksum for backend plugin tools in `mise.lock`, and does not support
attestations here. Check a checksum before extracting, as the complete example
does.

### BackendExecEnv

| `ctx` field    | Value                                                 | Example                                                     |
| -------------- | ----------------------------------------------------- | ----------------------------------------------------------- |
| `tool`         | The tool name                                         | `"deploy"`                                                  |
| `version`      | The installed version                                 | `"2.1.0"`                                                   |
| `install_path` | The install directory                                 | `"/home/user/.local/share/mise/installs/acme-deploy/2.1.0"` |
| `options`      | The tool's options; see [Tool options](#tool-options) | `{ mirror = "https://..." }`                                |

Return `{ env_vars = { { key = ..., value = ... }, ... } }`. Implement this hook
even when you have nothing to add, and return `{ env_vars = {} }`. Return each
`PATH` directory as its own entry; mise joins repeated keys with the platform's
path separator. If you return no `PATH` entry, mise adds `install_path`'s `bin`.
mise caches the result for each version and set of options, so keep the hook
fast.

### BackendUninstall

| `ctx` field     | Value                                      |
| --------------- | ------------------------------------------ |
| `tool`          | The tool name                              |
| `version`       | The installed version                      |
| `install_path`  | The install directory, which still exists  |
| `download_path` | The download directory                     |
| `options`       | The tool's options from the current config |

Use this hook for cleanup that deleting the install directory cannot do, such
as running a vendor uninstaller or removing entries created outside
`install_path`:

```lua [hooks/backend_uninstall.lua]
function PLUGIN:BackendUninstall(ctx)
    -- undo changes made outside ctx.install_path; files inside it can still be read
end
```

mise calls this hook whenever it removes an installed version: `mise uninstall`,
`mise upgrade`, `mise prune`, and delayed removal of old versions. It does not
run for `--dry-run`. If the hook raises an error, mise stops and keeps the
install directory so the uninstall can be retried.

`ctx.options` comes from the current config. When the tool is no longer
configured, for example during `mise prune`, it contains only defaults. Save
anything the uninstaller needs in `install_path` during `BackendInstall` instead
of relying on `ctx.options`.

### BackendListTools

`BackendListTools` receives an empty `ctx`, because no tool is selected yet.
Return the tools the backend manages:

```lua [hooks/backend_list_tools.lua]
function PLUGIN:BackendListTools(ctx)
    return {
        tools = {
            { name = "deploy", description = "Deploy services" },
            { name = "lint", description = "Check configuration files" },
        },
    }
end
```

`name` is required and `description` is optional. mise prefixes each name with
the plugin's install name and shows the resulting `acme:deploy` identifiers in
[`mise search`](/cli/search.html), shell completion and the interactive
`mise use` picker. Return a short, finite catalog; a package-manager backend
should not list a whole ecosystem. mise caches the response for the
[`fetch_remote_versions_cache`](/configuration/settings.html#fetch_remote_versions_cache)
duration and uses the stale result when a refresh fails.

### BackendSearchTools

| `ctx` field | Value                                    | Example |
| ----------- | ---------------------------------------- | ------- |
| `query`     | The text to search for, after any prefix | `"dep"` |

Return the same shape as `BackendListTools`. mise calls this hook when
`mise search` or shell completion has a non-empty query, so a backend can search
a large or changing catalog without listing all of it. `mise search acme:dep`
sends `dep`. A plugin can implement this hook, `BackendListTools`, or both, for
example a short list of featured tools plus a registry search. mise caches each
query's response separately.

## Tool options {#tool-options}

`ctx.options` holds the options on a tool's entry in `mise.toml`, such as
`"acme:deploy" = { version = "2.1.0", verify = false }`, in every hook except
`BackendListTools` and `BackendSearchTools`. Backend plugins receive options as
tool plugins do, so `verify = false` arrives as the string `"false"`, which is
truthy in Lua; see [tool options](/tool-plugin-development.html#tool-options)
for the conversion rules, the options mise keeps to itself and the
`MISE_TOOL_OPTS__` environment variables.

## metadata.lua {#metadata-lua}

`metadata.lua` sets the global `PLUGIN` table with at least `name` and
`version`; the Lua reference lists [every field](/plugin-lua-modules.html#metadata).
A backend plugin that needs another tool while it installs, such as `node` for
an npm-based backend, lists it in `depends`:

```lua [metadata.lua]
PLUGIN = {
    name = "my-npm",
    version = "0.1.0",
    depends = { "node" },
}
```

If the user configured `node`, mise installs it before this plugin's tools and
puts it on `PATH` for the commands the hooks run; see
[depends](/tool-plugin-development.html#depends).

## Testing {#testing-your-plugin}

```sh
mise plugins link acme ./mise-acme
mise ls-remote acme:deploy
mise use acme:deploy@2.1.0
mise exec -- deploy --version
```

Run a failing command with debug output to see the hook's error and output, and
clear a tool's cache when a cached version list or environment hides an edit to
a hook:

```sh
MISE_DEBUG=1 mise install acme:deploy@2.1.0
mise cache clear acme:deploy
```

Test on every platform you support, including a path that contains spaces. The
template's `mise run test` task runs the same checks. See
[Publishing plugins](/plugin-publishing.html#testing-before-publication) for an
isolated test setup.

## Common mistakes {#common-mistakes}

- Returning two values from a helper. `gsub` returns the new string and a
  count, so `return s:gsub("^v", "")` makes
  `table.insert(versions, normalize(tag))` fail with
  `bad argument #2 to 'insert'`. Wrap the call in parentheses:

  ```lua
  local function normalize(tag)
      return (tag:gsub("^v", "")) -- the parentheses drop gsub's count
  end
  ```

- Stripping more than a prefix the publisher documents. Treat the rest of a
  version as opaque, and do not sort versions with a SemVer parser.
- Searching `cmd.exec` output for the word `error`. `cmd.exec` raises an error
  with the command's stderr when it exits with a nonzero status.
- Building shell commands from tool names, versions or options. Pass values in
  `cmd.exec`'s `env` option and quote them, as the complete example does.
- Shelling out to `mv`, `mkdir` or `cp`. Use `file.move` instead of `mv`.
  `file.move`, `http.download_file` and `archiver.decompress` create the
  directories they write into, so most plugins never need `mkdir`. The file
  module has no copy or mkdir function; if you shell out for those, quote every
  path and remember that the command differs on Windows.
- Joining paths with `..` and `"/"`. Use `file.join_path`.
- Expecting a Lua table to cache data between commands. Each mise command
  starts a new Lua runtime; mise caches version lists and environments itself.
  See [cache behavior](/cache-behavior.html) and the
  [Lua reference](/plugin-lua-modules.html#caching).

To publish the plugin, see [Publishing plugins](/plugin-publishing.html). For
another real backend plugin, see [vfox-npm](https://github.com/jdx/vfox-npm),
which installs npm packages with the `npm` on `PATH`.
