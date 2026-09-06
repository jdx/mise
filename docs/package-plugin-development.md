# Package Plugin Development

A package plugin is a Lua-based vfox plugin that implements a machine-global
manager for [`[bootstrap.packages]`](/bootstrap/packages/). It wraps state owned
by a host tool rather than installing versioned tools under mise's data
directory.

Start with a manager that can report installed state without prompting or making changes.
The status hook drives previews and action selection, so an inaccurate answer can cause
unnecessary installations or hide missing packages. See [package plugin usage](/bootstrap/packages/plugins.html)
for user configuration.

## Layout

```text
mise-vscode-extensions/
├── metadata.lua
├── mise.plugin.toml
└── hooks/
    ├── package_installed.lua
    ├── package_install.lua
    ├── package_upgrade.lua
    └── package_uninstall.lua
```

The required `hooks/package_installed.lua` and `hooks/package_install.lua` pair
identifies the repository as a package plugin. A repository with only one of
these hooks remains a regular vfox plugin. If `hooks/backend_install.lua` is
also present, mise treats the repository as a tool backend instead; package
and tool-backend plugins must be separate repositories.

Provide normal Lua metadata as well as the package-manager declaration:

```lua
PLUGIN = {
  name = "vscode-extensions",
  version = "1.0.0",
  description = "Manage VS Code extensions",
}
```

In `mise.plugin.toml`:

```toml
[package-manager]
requires = ["code"]
supports_version_pins = true
os = ["macos", "linux"]
```

- `requires` lists host binaries the hooks invoke. mise adds its shims and
  global toolset bin paths to `PATH`, but does not install these tools
  automatically; users declare them in `[tools]` or install them manually.
- `supports_version_pins` defaults to `false`.
- `os` is optional and defaults to every platform. Values use mise platform
  names such as `macos`, `linux`, and `windows`.

## Hooks

Hooks are batch-oriented, but each hook receives the batch for its own phase:

- `PackageInstalled` receives every request in the current invocation. This may
  be the merged `[bootstrap.packages]` declarations or an explicit subset named
  on the command line.
- `PackageInstall` receives only requests mise selected for installation, such
  as packages reported missing or at a mismatched requested version.
- `PackageUpgrade` receives the actionable requests reported as present,
  including packages that are already current, so the manager can no-op them.
  Requests reported missing or unavailable and unsupported version pins are
  omitted.

mise does not call an action hook when its action batch is empty.

For example, a VS Code manager can inspect extensions with one host command and return
only the requested identities:

```lua
function PLUGIN:PackageInstalled(ctx)
  local output = require("cmd").exec("code --list-extensions --show-versions")
  local installed = {}
  for line in output:gmatch("[^\r\n]+") do
    local name, version = line:match("^(.+)@([^@]+)$")
    if name then
      installed[name:lower()] = version
    end
  end
  local results = {}
  for _, package in ipairs(ctx.packages) do
    local version = installed[package.name:lower()]
    table.insert(results, {
      name = package.name,
      state = version and "installed" or "missing",
      version = version,
    })
  end
  return {packages = results}
end
```

`PackageInstalled` must be side-effect free, fast, non-interactive, and never
elevate. It must return one `installed` or `missing` entry for every request.
mise computes a version mismatch when a requested pin is not exactly equal to
the returned version.

```lua
function PLUGIN:PackageInstall(ctx)
  -- ctx.dry_run: print intended actions and do nothing
  -- ctx.update: refresh manager metadata first when applicable
  for _, package in ipairs(ctx.packages) do
    -- install package.name, optionally at package.version
  end
  return {}
end
```

`PackageUpgrade` has the same context and response. It is optional; mise calls
`PackageInstall` when the upgrade hook is absent.

An action batch is not a complete desired-state snapshot. An explicit command
may target only a subset, and removing the final declaration for a manager
produces no batch for that manager. A plugin must not infer that an identity
should be removed merely because it is absent from `ctx.packages`.

`PackageUninstall` is optional and is used only by the explicit destructive
command `mise bootstrap packages prune --manager <plugin>`. mise passes the
concrete, approved removal batch after protecting packages declared by the
current config and trusted, loadable tracked configs:

```lua
function PLUGIN:PackageUninstall(ctx)
  for _, package in ipairs(ctx.packages) do
    -- uninstall package.name; package.version is the observed installed version
  end
  return {}
end
```

Dry runs do not invoke this hook. mise records ownership only when a package
reported missing before `PackageInstall` is present afterwards. Packages that
were already installed, including installations made before ownership tracking
was introduced, are never claimed or sent to `PackageUninstall`. The ownership
ledger persists across plugin removal and reinstallation. Explicit prune still
works when the desired set is empty, including after the final declaration is
removed. After the hook returns or fails, mise calls `PackageInstalled` to
verify each removal when possible and retains ownership for anything still
present. After confirmation, mise reloads the complete desired set before
invoking the hook; newly declared packages are removed from the approved batch,
and new removal candidates are never added without another confirmation.

## Hard contracts

- Package plugins must never invoke `sudo` in any hook. mise never elevates for
  them.
- Version strings are opaque. Compare them with exact equality only; never
  parse or sort them.
- `PackageInstalled` is side-effect free, non-interactive, never elevates, and
  should be fast.
- Hooks operate on phase-specific batches and must not treat absence from a
  batch as an uninstall request.
- `PackageUninstall` removes only the identities provided by mise and must not
  perform manager-wide orphan cleanup.
- Declare every required host binary in `requires`.

For a VS Code implementation, `PackageInstalled` can parse
`code --list-extensions --show-versions`, `PackageInstall` can run
`code --install-extension name[@version]`, and `PackageUpgrade` can reinstall only the
requested extensions. Avoid `code --update-extensions` for a selected batch: it updates
extensions outside that batch too. Keep any profile selection consistent between status
and action hooks, and quote package arguments for the shell in use.

## Testing

Test against a disposable host profile or a fake host CLI before changing real packages.
Cover an empty batch, missing and installed packages, exact pin mismatch, failed action,
and a subset request. Verify that status calls never mutate state and that an action touches
only `ctx.packages`. Test dry runs and the explicit prune ownership checks separately.

See [Plugin Publishing](/plugin-publishing.html) for isolated mise directories and release
validation, and [Lua modules](/plugin-lua-modules.html#command-module) for command execution.
