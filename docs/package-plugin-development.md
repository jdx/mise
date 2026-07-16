# Package Plugin Development

A package plugin is a Lua-based vfox plugin that implements a machine-global
manager for [`[bootstrap.packages]`](/bootstrap/packages/). It wraps state owned
by a host tool rather than installing versioned tools under mise's data
directory.

## Layout

```text
mise-vscode-extensions/
├── metadata.lua
├── mise.plugin.toml
└── hooks/
    ├── package_installed.lua
    ├── package_install.lua
    └── package_upgrade.lua
```

The required `hooks/package_installed.lua` and `hooks/package_install.lua` pair
identifies the repository as a package plugin. A repository with only one of
these hooks remains a regular vfox plugin. If `hooks/backend_install.lua` is
also present, mise treats the repository as a tool backend instead; package
and tool-backend plugins must be separate repositories.

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

All hooks receive the complete package batch. Managers must be batch-oriented.

```lua
function PLUGIN:PackageInstalled(ctx)
  -- ctx.packages: {{ name = "diff", version = "1.3.4" | nil }, ...}
  return {
    packages = {
      { name = "diff", state = "installed", version = "1.3.4" },
      { name = "s3", state = "missing" },
    },
  }
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
`PackageInstall` when the upgrade hook is absent. The name reserves room for a
future `PackageUninstall` hook, but uninstall and prune are not part of v1.

## Hard contracts

- Package plugins must never invoke `sudo` in any hook. mise never elevates for
  them.
- Version strings are opaque. Compare them with exact equality only; never
  parse or sort them.
- `PackageInstalled` is side-effect free, non-interactive, never elevates, and
  should be fast.
- Hooks operate on the full request batch.
- Declare every required host binary in `requires`.

For a VS Code implementation, `PackageInstalled` can parse
`code --list-extensions --show-versions`, `PackageInstall` can run
`code --install-extension name[@version]`, and `PackageUpgrade` can run
`code --update-extensions` or reinstall the requested extensions.
