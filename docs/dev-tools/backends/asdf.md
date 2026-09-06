# asdf Backend

::: warning
asdf plugins are considered legacy. **New asdf and vfox plugins are not accepted into the [mise registry](https://github.com/jdx/mise/blob/main/registry/) for supply-chain security reasons** — for registry submissions use [packslip](/dev-tools/backends/packslip.html) (preferred when the project publishes packslips), [aqua](/dev-tools/backends/aqua.html), [github](/dev-tools/backends/github.html), or [gitlab](/dev-tools/backends/gitlab.html) instead.

If you are writing a private/custom plugin (not for registry submission), prefer [vfox plugins](/dev-tools/backends/vfox.html) over asdf — they're written in Lua, work cross-platform (including Windows), and have access to built-in modules for HTTP, JSON, HTML parsing, and more.
:::

The `asdf` backend runs a tool's asdf-compatible plugin scripts. Use it when an
existing plugin provides installation behavior your tool needs. These scripts
execute with your permissions and may invoke programs outside mise, so inspect
the plugin source and its prerequisites before using it.

asdf plugins generally need Bash and Unix utilities. Windows support depends
on the plugin and its execution environment; prefer a supported native backend
or a vfox plugin there.

## Usage

Use an explicit plugin source when it is not supplied by the registry. Replace
the repository and version with the plugin you intend to use:

```toml
[tools]
"asdf:owner/plugin" = "1.0.0"
```

Run `mise install`, then `mise exec -- TOOL --version`, replacing `TOOL` with its
executable name. Installing a plugin does not itself configure an active tool
version. For existing registry tools, `mise registry TOOL` shows the configured
sources.

## Feature Comparison: asdf vs vfox

| Area                 | asdf plugins                                                  | vfox plugins                                                                                   |
| -------------------- | ------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| Implementation       | Executable scripts, usually Bash                              | Lua hooks run by mise's embedded interpreter                                                   |
| External utilities   | Often need curl, jq, and platform utilities                   | Built-in HTTP, JSON, HTML, and archive modules are available                                   |
| Platform portability | Depends on scripts and available commands                     | Lua hooks can select platform-specific artifacts; publishers must supply compatible builds     |
| Installation         | Plugin scripts download and install the tool                  | Structured download metadata plus optional post-install hooks                                  |
| Lockfiles            | Version locking; no portable artifact URL/provenance contract | Tool plugins can supply download metadata and attestations; backend-plugin capabilities differ |

These are interface differences, not a sandbox boundary. Either plugin system
can run external commands. See [plugin development](/tool-plugin-development.html)
for the capabilities mise adds to the vfox hook interface.

## Hook Migration: asdf to vfox

| asdf Script                 | vfox Hook                | Notes                                                            |
| --------------------------- | ------------------------ | ---------------------------------------------------------------- |
| `bin/list-all`              | `Available`              | Return structured version objects instead of plain text          |
| `bin/download`              | `PreInstall`             | Return URL and checksum; mise handles the download               |
| `bin/install`               | `PostInstall`            | Runs after mise downloads and extracts the tool                  |
| `bin/exec-env`              | `EnvKeys`                | Return structured key/value pairs instead of `export` statements |
| `bin/list-legacy-filenames` | `PLUGIN.legacyFilenames` | Set in `metadata.lua` instead of a script                        |
| `bin/parse-legacy-file`     | `ParseLegacyFile`        | Return structured result instead of plain text                   |

## Writing asdf (legacy) plugins for mise

See the asdf documentation for more information on [writing plugins](https://asdf-vm.com/plugins/create.html).

The `bin/list-all` and `bin/latest-stable` version scripts receive environment variables and PATH
additions resolved from mise configuration before tools are loaded. This allows private plugins to
use credentials, helper executables from `_.path`, or other project-specific values from `[env]`
while listing versions. Because these values can change the available versions, mise stores
version-list caches separately for each resolved configuration environment without writing the
original values or paths to the cache.

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `asdf` backend—these
go in `[tools]` in `mise.toml`.

### `install_env`

Set environment variables for asdf plugin install scripts:

```toml
[tools]
"asdf:owner/plugin" = { version = "latest", install_env = { MAKEFLAGS = "-j8" } }
```

### Install dependencies

Matching tools selected in the same install operation and declared with the
[`depends` option](/dev-tools/#tool-dependencies) are installed before the asdf tool. Their paths
are added to the `PATH` used by its `bin/download` and `bin/install` scripts:

```toml
[tools]
python = "3.12"
"asdf:owner/plugin" = { version = "latest", depends = ["python"] }
```

This allows an asdf plugin to invoke an executable supplied by another mise-managed tool during
the same `mise install`. Other active mise tools are not added implicitly; declare every
mise-managed install requirement with `depends`. Executables already available on the ambient
system or configuration `PATH` remain available.

The `depends` option does not add or install a missing tool. A configured dependency must already
be installed or selected in the same install operation.
