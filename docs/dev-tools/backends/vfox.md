---
description: "Install tools with vfox plugins: Lua plugins that run on Linux, macOS and Windows."
---

# vfox backend

The `vfox` backend runs [vfox](https://github.com/version-fox/vfox) plugins:
Lua plugins that list a tool's versions and install it. The Lua runtime is
built into mise, so vfox plugins can run on Windows as well as Linux and macOS.
Prefer a built-in backend when one can install the tool, and use a vfox plugin
when the tool needs custom install logic.

## Requirements {#dependencies}

The Lua interpreter is built into mise. A plugin may still run external commands
or install software that needs system libraries, so read the plugin's
requirements.

## Usage

Install CMake from an explicit vfox plugin, then run it:

```sh
mise use vfox:version-fox/vfox-cmake
mise exec -- cmake --version
```

This writes the following to `mise.toml`. Add `-g` for a global tool.

```toml
[tools]
"vfox:version-fox/vfox-cmake" = "latest"
```

mise installs the plugin from GitHub the first time it needs it. The `vfox:`
prefix selects that plugin even when the `cmake` registry shorthand prefers
another backend. Find plugins in the
[mise-plugins organization](https://github.com/mise-plugins) and the
[vfox plugin index](https://github.com/version-fox/vfox-plugins).

## When mise uses vfox {#default-plugin-backend}

A registry shorthand such as `cmake` can list several backends, and mise uses
the first one available on your platform; run `mise registry cmake` to see them,
and see [how backend selection works](/dev-tools/backends/#how-backend-selection-works).
To use a particular plugin, write its `vfox:` identifier, as in
[Usage](#usage). On Windows, mise never selects asdf plugins. Elsewhere,
`mise settings add disable_backends asdf` skips them.

## Plugin sources {#plugins}

A `vfox:` identifier names a GitHub repository (`vfox:owner/repo`) or a full Git
URL. You can also install a plugin under a name of your own, from Git, a zip
archive or a local directory, and then use that name in `[tools]`, or
`name:tool` for a backend plugin that manages several tools. See
[Plugins](/plugins.html) to install, update and remove plugins. To write one,
see [tool plugins](/tool-plugin-development.html).

### From a signed packslip release {#install-from-a-signed-packslip}

A publisher can release a vfox plugin as a signed
[packslip](/dev-tools/backends/packslip.html) archive from a GitHub repository.
Install it with:

```sh
mise plugins install vfox:PLUGIN_NAME 'packslip:OWNER/REPO#PLUGIN_VERSION'
```

Or configure it:

```toml
[plugins]
"vfox:PLUGIN_NAME" = "packslip:OWNER/REPO#PLUGIN_VERSION"
```

Leave out `#PLUGIN_VERSION` for the latest plugin release. mise checks the
signature, digest, signer and release policy before it replaces an installed
plugin, records the version, digest and signer it installed, and checks them
again when it reinstalls the same version. `mise plugins update PLUGIN_NAME`
keeps an explicit version pin; reinstall with another version to change it.
Publishers can find the archive format in
[Publishing plugins](/plugin-publishing.html#packslip).

## URL replacements

The vfox backend applies mise's [`url_replacements`](/url-replacements.html)
setting to tool downloads and to requests made through the plugin's built-in Lua
HTTP module, including `http.get`, `http.head`, `http.download_file` and their
`try_*` variants.

After applying URL replacements, the backend uses mise's
[`netrc`](/configuration/settings.html#netrc) setting to add HTTP Basic
authentication for the destination host. An `Authorization` header that the
plugin sets itself takes precedence while the request stays on the same origin.

## Tool options

mise handles its own options, such as `version`, `install_env`, `depends`, `os`
and `postinstall`; see [tool options](/dev-tools/#tool-options). Every other
option goes to the plugin, so check the plugin's README for the options it
accepts. Plugin authors can read how options reach the hooks in
[tool plugins](/tool-plugin-development.html).

### `install_env` {#install-env}

Environment variables for the commands that the plugin runs during
installation with `cmd.exec` or `os.execute`. The plugin's built-in Lua HTTP,
archive and JSON helpers do not read them.

```toml
[tools]
"vfox:version-fox/vfox-cmake" = { version = "latest", install_env = { HTTPS_PROXY = "http://proxy.example" } }
```

### `depends` {#install-dependencies}

Tools the plugin needs while installing, such as `depends = ["go"]`, when the
plugin does not declare them itself. When a listed tool is also in `[tools]`,
mise installs it first and puts it on `PATH` for the plugin's install commands.
`depends` does not install a tool you have not configured; add it to `[tools]`,
or have it on `PATH` already. See
[tool dependencies](/dev-tools/#tool-dependencies); plugin authors declare their
own with [`PLUGIN.depends`](/tool-plugin-development.html#depends).

Implementation: [`src/backend/vfox.rs`](https://github.com/jdx/mise/blob/main/src/backend/vfox.rs).
