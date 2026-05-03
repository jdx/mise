# Vfox Backend

::: tip
Vfox is the recommended plugin system for mise. It provides cross-platform support, built-in modules, and a modern hook-based architecture.
:::

[Vfox](https://github.com/version-fox/vfox) plugins may be used in mise to install tools.

## Why vfox?

- **Cross-platform** — plugins work on Windows, macOS, and Linux without platform-specific code
- **Built-in modules** — HTTP, JSON, HTML parsing, archive extraction, semver comparison, and logging are all available out of the box, no external dependencies needed
- **Security** — [tool plugins](../../tool-plugin-development.md) support attestation verification (GitHub artifact attestations, cosign signatures, SLSA provenance) for downloaded artifacts. When a tool plugin's `PreInstall` hook returns an `attestation` table, mise verifies it during install and records the result in `mise.lock`, protecting against downgrade attacks on subsequent installs. Backend plugins do not currently support attestation
- **Modern architecture** — structured hooks with typed contexts, backend plugins for multi-tool management, rolling version checksums, and lock file support

The code for this is inside the mise repository at [`./src/backend/vfox.rs`](https://github.com/jdx/mise/blob/main/src/backend/vfox.rs).

## Dependencies

No extra system packages are required to _run_ the vfox backend. Vfox Lua code is executed by the interpreter built into mise.

### `depends` in `metadata.lua`

If a plugin’s install hooks need another mise tool’s executables (for example `node` for `npm` in `BackendInstall`), declare them on the `PLUGIN` table in `metadata.lua`:

```lua
PLUGIN = {
    name = "vfox-npm",
    version = "1.0.0",
    depends = { "node" },
}
```

Use an array of **tool name strings** (the same identifiers you would use in `mise.toml`). mise treats them as install dependencies so those tools are ordered before this plugin and appear on `PATH` in the dependency environment used during installation. This is separate from `depends` on a `[tools]` entry in `mise.toml`, which only orders installs among tools you already listed in config.

## Usage

The following installs the latest version of cmake and sets it as the active version on PATH:

```sh
$ mise use -g vfox:version-fox/vfox-cmake
$ cmake --version
cmake version 3.21.3
```

The version will be set in `~/.config/mise/config.toml` with the following format:

```toml
[tools]
"vfox:version-fox/vfox-cmake" = "latest"
```

## Default plugin backend

On Windows, mise uses vfox plugins by default.
If you'd like to use plugins by default even on Linux/macOS, set the following settings:

```sh
mise settings add disable_backends asdf
```

Now you can list available plugins with `mise registry`:

```sh
$ mise registry | grep vfox:
clang                         vfox:mise-plugins/vfox-clang
cmake                         vfox:mise-plugins/vfox-cmake
crystal                       vfox:mise-plugins/vfox-crystal
dart                          vfox:mise-plugins/vfox-dart
dotnet                        vfox:mise-plugins/vfox-dotnet
etcd                          aqua:etcd-io/etcd vfox:mise-plugins/vfox-etcd
flutter                       vfox:mise-plugins/vfox-flutter
gradle                        aqua:gradle/gradle vfox:mise-plugins/vfox-gradle
groovy                        vfox:mise-plugins/vfox-groovy
kotlin                        vfox:mise-plugins/vfox-kotlin
maven                         aqua:apache/maven vfox:mise-plugins/vfox-maven
php                           vfox:mise-plugins/vfox-php
scala                         vfox:mise-plugins/vfox-scala
terraform                     aqua:hashicorp/terraform vfox:mise-plugins/vfox-terraform
vlang                         vfox:mise-plugins/vfox-vlang
```

And they will be installed when running commands such as `mise use -g cmake` without needing to
specify `vfox:cmake`.

## Plugins

In addition to the standard vfox plugins, mise supports modern plugins that can manage multiple tools using the `plugin:tool` format. These plugins are perfect for:

- Installing tools from private repositories
- Package managers (npm, pip, etc.)
- Custom tool families

### Example: Plugin Usage

```bash
# Install a plugin
mise plugin install my-plugin https://github.com/username/my-plugin

# Use the plugin:tool format
mise install my-plugin:some-tool@1.0.0
mise use my-plugin:some-tool@latest
```

### Install from Zip File

```bash
# Install a plugin from a zip file over HTTPS
mise plugin install <plugin-name> <zip-url>
# Example: Installing a plugin from a zip file
mise plugin install vfox-cmake https://github.com/mise-plugins/vfox-cmake/archive/refs/heads/main.zip
```

For more information, see:

- [Using Plugins](../../plugin-usage.md) - End-user guide
- [Plugin Development](../../tool-plugin-development.md) - Developer guide
- [Plugin Template](https://github.com/jdx/mise-tool-plugin-template) - Quick start template for creating plugins
