# Vfox Backend

::: tip
vfox is the recommended plugin system for mise. It provides cross-platform support, built-in modules, and a modern hook-based architecture.
:::

Plugins for [vfox](https://github.com/version-fox/vfox) can be used in mise to install tools.

## Why vfox?

- **Cross-platform** — Lua hooks run on Windows, macOS, and Linux; plugins still need compatible artifacts and platform handling
- **Built-in modules** — HTTP, JSON, HTML parsing, archive extraction, semver comparison, and logging are all available out of the box, with no external dependencies needed
- **Security** — [tool plugins](../../tool-plugin-development.md) support attestation verification (GitHub artifact attestations, cosign signatures, SLSA provenance) for downloaded artifacts. When a tool plugin's `PreInstall` hook returns an `attestation` table, mise verifies it during install and records the result in `mise.lock`, protecting against downgrade attacks on subsequent installs. Backend plugins do not currently support attestation
- **Structured hooks** — contexts for version discovery, installation, and environment setup; backend plugins can manage multiple tools

The code for this is inside the mise repository at [`./src/backend/vfox.rs`](https://github.com/jdx/mise/blob/main/src/backend/vfox.rs).

## Dependencies

The Lua interpreter is built into mise. A plugin may still invoke external
commands or install software that needs system libraries. Read that plugin's
requirements; the built-in interpreter does not make every plugin portable.

## Usage

Install cmake from an explicit vfox plugin and verify the selected executable:

```sh
mise use vfox:version-fox/vfox-cmake
mise exec -- cmake --version
```

This writes the following project configuration. Add `-g` for a global tool.

```toml
[tools]
"vfox:version-fox/vfox-cmake" = "latest"
```

The explicit prefix selects that plugin even if the `cmake` registry shorthand
prefers another backend.

## Default plugin backend

Windows excludes asdf from default backend selection. On Linux and macOS, you
can exclude it with `mise settings add disable_backends asdf`, but this does not
make vfox take precedence over built-in, Packslip, Aqua, or release backends.
Check `mise registry cmake` for the shorthand's available sources, or use an
explicit `vfox:` identifier when you intend to use a particular plugin.

## Plugins

In addition to the standard vfox tool plugins, mise supports backend plugins that can manage multiple tools using the `plugin:tool` format. These plugins are well suited to:

- Installing tools from private repositories
- Package managers (npm, pip, etc.)
- Custom tool families

### Example: Plugin Usage

```bash
# Install a plugin
mise plugins install my-plugin https://github.com/username/my-plugin

# Use the plugin:tool format
mise install my-plugin:some-tool@1.0.0
mise use my-plugin:some-tool@latest
```

### Install from Zip File

Replace `PLUGIN_NAME` and `HTTPS_ZIP_URL` with the plugin name and archive URL.

```bash
# Install a plugin from a zip file over HTTPS
mise plugins install PLUGIN_NAME HTTPS_ZIP_URL
# Example: Installing a plugin from a zip file
mise plugins install vfox-cmake https://github.com/mise-plugins/vfox-cmake/archive/refs/heads/main.zip
```

For more information, see:

- [Using Plugins](../../plugin-usage.md) - End-user guide
- [Plugin Development](../../tool-plugin-development.md) - Developer guide
- [Plugin Template](https://github.com/jdx/mise-tool-plugin-template) - Quick start template for creating plugins

## URL replacements

The vfox backend honors mise's [`url_replacements`](/url-replacements.html) setting for both
tool artifact downloads and requests made through the plugin's built-in Lua HTTP module. This
includes `http.get`, `http.head`, `http.download_file`, and their `try_*` variants.

After applying URL replacements, vfox also uses mise's [`netrc`](/configuration/settings.html#netrc)
setting to add HTTP Basic authentication for the destination host. An explicit `Authorization`
header supplied by a plugin takes precedence when the request stays on the same origin.

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `vfox` backend—these
go in `[tools]` in `mise.toml`.

Traditional vfox `PreInstall` and `PostInstall` hooks receive custom options in the structured
`ctx.options` table. Scalar values use mise's existing string representation, while arrays and
tables remain structured:

```toml
[tools]
"vfox:example/plugin" = { version = "1.0.0", bundled = false, channels = ["stable", "beta"] }
```

```lua
function PLUGIN:PreInstall(ctx)
    local bundled = ctx.options.bundled == "false"
    local channels = ctx.options.channels
    -- ...
end
```

Existing plugins can continue reading custom options from their hook environment with the
`MISE_TOOL_OPTS__` prefix. Those variables are available only while mise runs the plugin hook and
are not exported to the user's shell. New plugins should use `ctx.options`.

### `install_env`

Set environment variables for commands that a vfox plugin starts with `cmd.exec`
during install hooks. vfox's built-in Lua HTTP, archive, and JSON helpers do not
use these variables directly.

```toml
[tools]
"vfox:version-fox/vfox-cmake" = { version = "latest", install_env = { HTTPS_PROXY = "http://proxy.example" } }
```

### Install dependencies

Plugin authors should declare intrinsic install requirements with `PLUGIN.depends` in
`metadata.lua`. Users can supplement those declarations with the
[`depends` tool option](/dev-tools/#tool-dependencies). Matching configured tools from both
sources share one install dependency context: they are ordered before the dependent tool, and their
paths and `tools = true` values are available to install hooks launched through `os.execute` or
`cmd.exec`.

Declarations do not configure or automatically install a tool. A matching configured dependency
must resolve and be installed; an unconfigured dependency can still be supplied by the existing
system or configuration `PATH`. `io.popen` does not receive this install environment.
