---
description: "Write a Lua plugin that adds a package manager, such as VS Code extensions, to bootstrap packages."
---

# Writing package manager plugins {#package-manager-plugins}

A package manager plugin adds a manager to
[`[bootstrap.packages]`](/bootstrap/packages/) for packages that belong to
another program, such as VS Code extensions, `gh` extensions or Helm plugins.
mise asks the plugin which requested packages are installed, then hands it
batches to install, upgrade or remove.

The packages stay in the host program's own state, not in mise's data
directory. Users set up a plugin as described in the bootstrap guide to
[package manager plugins](/bootstrap/packages/plugins.html).

## Quick start {#layout}

A package manager plugin has this layout:

```text
mise-vscode-extensions/
├── metadata.lua
├── mise.plugin.toml
└── hooks/
    ├── package_installed.lua   # required: report what is installed
    ├── package_install.lua     # required: install a batch
    ├── package_upgrade.lua     # optional: upgrade a batch
    └── package_uninstall.lua   # optional: remove a batch for prune
```

`hooks/package_installed.lua` and `hooks/package_install.lua` together mark
the directory as a package manager plugin; with only one of them, it is a tool
plugin. A directory that also has `hooks/backend_install.lua` is a backend
plugin, so keep a package manager plugin in its own repository.

Users choose the manager name when they install the plugin and use it as the
prefix in `[bootstrap.packages]`. With `vscode = "..."` in `[bootstrap.plugins]`,
packages are `"vscode:ms-python.python"`. `PLUGIN.name` does not have to match,
and the name cannot be one of the built-in managers, such as `brew` or `apt`.

Link your working copy under the manager name, without a `package:` prefix, and
check status before you change anything:

```sh
mise plugins link vscode ./mise-vscode-extensions
mise bootstrap packages status
mise bootstrap packages apply --dry-run
```

## Complete example {#complete-example}

This plugin manages VS Code extensions with the `code` command on Linux and
macOS.

```lua [metadata.lua]
PLUGIN = {
    name = "vscode-extensions",
    version = "0.1.0",
    description = "Manage VS Code extensions",
}
```

```toml [mise.plugin.toml]
[package-manager]
requires = ["code"]
supports_version_pins = true
os = ["macos", "linux"]
```

```lua [hooks/package_installed.lua]
local cmd = require("cmd")

function PLUGIN:PackageInstalled(ctx)
    local installed = {}
    local output = cmd.exec("code --list-extensions --show-versions")
    for line in output:gmatch("[^\r\n]+") do
        local name, version = line:match("^(.+)@([^@]+)$")
        if name then
            installed[name:lower()] = version
        end
    end
    local results = {}
    for _, pkg in ipairs(ctx.packages) do
        local version = installed[pkg.name:lower()]
        table.insert(results, {
            name = pkg.name,
            state = version and "installed" or "missing",
            version = version,
        })
    end
    return { packages = results }
end
```

```lua [hooks/package_install.lua]
local cmd = require("cmd")

function PLUGIN:PackageInstall(ctx)
    for _, pkg in ipairs(ctx.packages) do
        local spec = pkg.version and (pkg.name .. "@" .. pkg.version) or pkg.name
        if ctx.dry_run then
            print("would install " .. spec)
        else
            cmd.exec('code --install-extension "$EXTENSION"', { env = { EXTENSION = spec } })
        end
    end
end
```

```lua [hooks/package_upgrade.lua]
local cmd = require("cmd")

function PLUGIN:PackageUpgrade(ctx)
    for _, pkg in ipairs(ctx.packages) do
        if ctx.dry_run then
            print("would upgrade " .. pkg.name)
        elseif not pkg.version then
            -- --force updates this extension; --update-extensions would update all of them
            cmd.exec('code --install-extension "$EXTENSION" --force', { env = { EXTENSION = pkg.name } })
        end
    end
end
```

```lua [hooks/package_uninstall.lua]
local cmd = require("cmd")

function PLUGIN:PackageUninstall(ctx)
    for _, pkg in ipairs(ctx.packages) do
        cmd.exec('code --uninstall-extension "$EXTENSION"', { env = { EXTENSION = pkg.name } })
    end
end
```

With the plugin linked as `vscode`, this config installs two extensions:

```toml [mise.toml]
[bootstrap.packages]
"vscode:ms-python.python" = "latest"
"vscode:golang.go" = "0.40.0"
```

`mise bootstrap packages apply` calls `PackageInstalled` with both requests,
then `PackageInstall` with the ones reported missing or at another version, then
`PackageInstalled` again to record which packages mise installed.
`mise bootstrap packages upgrade` calls `PackageUpgrade` with the installed
ones. The commands pass extension names in the `EXTENSION` environment variable
rather than splicing them into the command line.

## mise.plugin.toml {#mise-plugin-toml}

The `[package-manager]` table describes the manager:

| Key                     | Default        | Value                                                                                                 |
| ----------------------- | -------------- | ----------------------------------------------------------------------------------------------------- |
| `requires`              | none           | Host programs the hooks run. mise adds its shims and global tools to `PATH` but does not install them |
| `supports_version_pins` | `false`        | Whether the manager can install a pinned version such as `"0.40.0"`                                   |
| `os`                    | every platform | Platforms the manager runs on: `macos`, `linux` or `windows`                                          |

When a program in `requires` is missing, mise reports the manager as unavailable
and tells the user to add it to `[tools]` or install it. The hooks see the
process `PATH` plus mise's shims and the tools in the global config. When
`supports_version_pins` is `false`, mise skips pinned requests with a warning.

## Hooks {#hooks}

| Hook               | File                          | Required | mise calls it                                                         |
| ------------------ | ----------------------------- | -------- | --------------------------------------------------------------------- |
| `PackageInstalled` | `hooks/package_installed.lua` | yes      | to read the state of every request, before and after each action      |
| `PackageInstall`   | `hooks/package_install.lua`   | yes      | to install missing packages and packages at another version           |
| `PackageUpgrade`   | `hooks/package_upgrade.lua`   | no       | for `mise bootstrap packages upgrade`; falls back to `PackageInstall` |
| `PackageUninstall` | `hooks/package_uninstall.lua` | no       | for `mise bootstrap packages prune --manager <name>`                  |

Each request in `ctx.packages` is a table with `name` and `version`. A hook
fails the whole batch by raising an error with `error()`. mise does not call an
action hook when its batch is empty. Hooks can use the `cmd`, `json`, `file` and
other modules in the [Plugin Lua reference](/plugin-lua-modules.html); see the
[command module](/plugin-lua-modules.html#command-module) for running the host
program. `print()` writes a `mise [vscode] ...` line.

### PackageInstalled {#packageinstalled}

| `ctx` field | Value                                                                              |
| ----------- | ---------------------------------------------------------------------------------- |
| `packages`  | Every request in this run; `version` is the requested pin, or `nil` for `"latest"` |

Return `{ packages = { ... } }` with one entry for every request:

| Field     | Value                                           |
| --------- | ----------------------------------------------- |
| `name`    | The request's `name`                            |
| `state`   | `"installed"` or `"missing"`                    |
| `version` | The installed version, for an installed package |

The requests are the merged `[bootstrap.packages]` declarations, or the subset
named on the command line. mise compares a pinned request's `version` with the
installed version using exact string equality, and treats a mismatch as a
package to install. A missing entry or any other `state` is an error.

This hook drives status, previews and every action, so keep it fast and follow
the [rules](#hard-contracts). A wrong answer causes needless installs or hides
missing packages.

### PackageInstall {#packageinstall}

| `ctx` field | Value                                                                      |
| ----------- | -------------------------------------------------------------------------- |
| `packages`  | Requests that `PackageInstalled` reported missing or at another version    |
| `dry_run`   | `true` for `--dry-run`: print what you would do and change nothing         |
| `update`    | `true` for `--update`: refresh the manager's metadata first, if it has any |

Return nothing or a table. Install each request, at `version` when it is set.

### PackageUpgrade {#packageupgrade}

`PackageUpgrade` receives the same `ctx` as `PackageInstall`. `ctx.packages`
holds the requests that `PackageInstalled` reported installed, including ones
that are already current, so the hook can skip them. Missing packages are left
out; `mise bootstrap packages upgrade` tells the user to run `apply` for them.
Upgrade only the packages in `ctx.packages`: `code --update-extensions`, for
example, updates every extension. Without this hook, mise calls
`PackageInstall`.

### PackageUninstall {#packageuninstall}

| `ctx` field | Value                                                      |
| ----------- | ---------------------------------------------------------- |
| `packages`  | The packages to remove; `version` is the installed version |

mise sends `PackageUninstall` only packages it installed itself: one that
`PackageInstalled` reported missing before `PackageInstall` and installed after.
Packages that were already present, and packages still declared in the current
config or in another trusted config mise tracks, are never sent. Dry runs never
call this hook. After the hook returns or fails, mise calls `PackageInstalled`
again and keeps track of anything still installed.

mise keeps its record of what it installed in its state directory, by manager
name, so the record survives removing and reinstalling the plugin, and prune
works even after the last declaration for the manager is gone. After the user
confirms a prune, mise reloads the config and drops any package that has become
declared; it never adds a package the user did not confirm.

## Rules {#hard-contracts}

- Never run `sudo` in any hook. mise never elevates for package manager
  plugins.
- Treat versions as opaque strings and compare them with `==`; never parse or
  sort them.
- Keep `PackageInstalled` free of side effects and prompts.
- Never treat a package's absence from a batch as a request to remove it. A
  command can target a subset, and removing the last declaration for a manager
  sends no batch at all.
- Make `PackageUninstall` remove only the packages it is given, never every
  package the manager considers unused.
- List every host program the hooks run in `requires`.
- Use the same host profile or scope in every hook, so that `PackageInstalled`
  reports on the packages the action hooks change.

## Testing {#testing}

Test against a throwaway profile of the host program, or a fake command on
`PATH` that records its arguments, before you change real packages. Cover an
empty config, missing and installed packages, a pinned version that differs, a
failing action and a request for a subset on the command line. Check that
`mise bootstrap packages status` changes nothing, that each action touches only
`ctx.packages`, and that `--dry-run` changes nothing. Test
`mise bootstrap packages prune --manager vscode --dry-run` and a real prune
separately.

See [Publishing plugins](/plugin-publishing.html#testing-before-publication) for
an isolated test setup.
