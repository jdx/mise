---
description: "Install, configure, update and remove plugins that add tools, environment variables or package managers to mise."
socialDescription: "Install, update and remove plugins that add tools, environment variables or package managers."
---

# Plugins

A plugin is a directory of scripts, usually Lua, that teaches mise to install a
tool, set environment variables or manage another program's packages. Most
tools need no plugin, so check the [backends](/dev-tools/backends/) first.

## When you need a plugin {#when-you-need-a-plugin}

A tool does not need a registry short name or a plugin to be installable. The
built-in backends install most tools directly, for example
`mise use github:owner/repo` for release assets or `mise use npm:prettier` for
an npm package; see [which backend to use](/dev-tools/backends/#which-backend-to-use).

Use a plugin when installing or configuring something needs logic that no
backend has: a tool with an unusual download or build, a family of tools from
an internal artifact server, environment variables loaded from a service, or
packages that belong to another program, such as VS Code extensions.

Community plugins are in the [mise-plugins](https://github.com/mise-plugins)
organization and the [vfox plugin index](https://github.com/version-fox/vfox-plugins).
The [registry](/registry.html) is not a catalog of plugins.

## Plugin types {#choose-a-plugin-type}

| Type            | Adds                                                       | You use it as                                                 | Write one                                                   |
| --------------- | ---------------------------------------------------------- | ------------------------------------------------------------- | ----------------------------------------------------------- |
| Tool            | One tool and its versions                                  | `[tools]` `my-tool = "1.2.0"`                                 | [Tool plugins](/tool-plugin-development.html)               |
| Backend         | A family of tools under one prefix                         | `[tools]` `"my-backend:some-tool" = "1.2.0"`                  | [Backend plugins](/backend-plugin-development.html)         |
| Environment     | Environment variables and `PATH` entries; installs nothing | `[env]` `_.my-env-plugin = { api_url = "..." }`               | [Environment plugins](/env-plugin-development.html)         |
| Package manager | Packages owned by another program, such as VS Code         | `[bootstrap.packages]` `"vscode:ms-python.python" = "latest"` | [Package manager plugins](/package-plugin-development.html) |
| asdf (legacy)   | One tool, through shell scripts; Linux and macOS only      | `[tools]` `my-tool = "1.2.0"`                                 | [asdf plugins (legacy)](/asdf-legacy-plugins.html)          |

Lua plugins are [vfox](https://vfox.dev) plugins: tool plugins use the vfox
hooks, so most vfox plugins work in mise. Backend, environment and package
manager hooks are mise additions that the vfox CLI does not run.

The Lua runtime is built into mise on Linux, macOS and Windows. A plugin still
decides which platforms it supports, through the downloads it picks and the
programs it runs. asdf plugins are shell scripts, and mise does not run
asdf-backed tools on Windows.

## Install a plugin {#installing-plugins}

### In mise.toml {#in-mise-toml}

Declare the plugin under `[plugins]` so that everyone on the project gets the
same plugin:

```toml [mise.toml]
[plugins]
my-tool = "https://github.com/your-org/mise-my-tool#v1.2.0"

[tools]
my-tool = "2.0.0"
```

`mise install` installs the plugin, then the tool. The part after `#` selects a
tag, branch or full commit SHA; mise rejects abbreviated SHAs. A tag or branch
can move, so pin a full commit SHA to get the same plugin code every time.

An entry applies only when mise installs the plugin. After you change it,
`mise install` and `mise doctor` warn that the installed plugin no longer
matches; run `mise plugins install --force my-tool` to reinstall it from
`[plugins]`. See [`[plugins]`](/configuration.html#plugins) for every value the
section accepts.

### From the command line {#from-a-git-repository}

[`mise plugins install`](/cli/plugins/install.html) installs a plugin for you
alone; nothing in the project records it:

```sh
mise plugins install my-tool https://github.com/your-org/mise-my-tool
mise plugins install my-tool 'https://github.com/your-org/mise-my-tool#v1.2.0'
```

If you leave out the name, mise takes it from the URL and drops a leading
`asdf-`, `mise-` or `vfox-`, so `https://github.com/your-org/mise-my-tool`
installs as `my-tool`. If the plugin is already installed, mise prints a
warning and changes nothing; add `--force` to replace it.

### From a private repository {#from-a-private-repository}

Use an SSH URL, or an HTTPS URL with your Git credential helper. Do not put a
token in the URL, where it ends up in `mise.toml`, shell history and logs.
`[plugins]` values are [Tera templates](/templates.html), so a shared config can
take part of the source from the environment:

```toml [mise.toml]
[plugins]
my-backend = "git@github.com:{{ env.PLUGIN_ORG }}/my-backend.git"
```

### From a zip archive {#from-zip-file}

A URL that ends in `.zip` installs the archive's contents:

```sh
mise plugins install my-tool https://github.com/your-org/mise-my-tool/archive/refs/tags/v1.2.0.zip
```

When the archive holds a single top-level directory, as GitHub's tag archives
do, mise unwraps it. A plugin installed from an archive is not a Git checkout,
so `mise plugins update` cannot update it; replace it with
`mise plugins install --force my-tool <new-archive-url>`.

### From a local directory {#from-local-directory}

[`mise plugins link`](/cli/plugins/link.html) symlinks a directory, so edits to
the plugin take effect without reinstalling it:

```sh
mise plugins link my-tool ./mise-my-tool
```

Add `--force` to replace a plugin that is already installed. A `[plugins]` entry
can also name a local directory; a path that starts with `./` or `../` resolves
from the [config root](/configuration.html#config-root) of the file that
declares it.

### From a signed packslip release {#from-a-signed-packslip-release}

A publisher can release a Lua plugin as a signed
[packslip](/dev-tools/backends/packslip.html) archive on GitHub:

```sh
mise plugins install vfox:my-plugin 'packslip:your-org/my-plugin#1.0.0'
```

A packslip source always installs a vfox plugin, so the `vfox:` prefix is
optional, on the command line and in `[plugins]`, where
`my-plugin = "packslip:your-org/my-plugin#1.0.0"` works as well as
`"vfox:my-plugin" = ...`. On the command line, another type prefix such as
`asdf:` is rejected. A plugin that is already installed keeps the type it was
installed with. Omit `#1.0.0` to use
the latest eligible release. mise verifies the archive's signature, digest and
signer, and `mise plugins update` keeps an explicit version pin. Only GitHub
repositories that publish `packslip.sigstore.json` are supported; see
[the vfox backend](/dev-tools/backends/vfox.html#install-from-a-signed-packslip).

### From a tool identifier {#vfox-identifiers}

A `vfox:` identifier in `[tools]` names the plugin's GitHub repository, and mise
installs the plugin the first time it needs it:

```sh
mise use vfox:version-fox/vfox-cmake
```

See the [vfox backend](/dev-tools/backends/vfox.html) and the
[asdf backend](/dev-tools/backends/asdf.html), which accepts `asdf:owner/repo`
the same way.

## Use a plugin {#using-plugins}

### Tool plugins {#tool-plugins}

A tool plugin's install name is the tool name:

```sh
mise ls-remote my-tool
mise use my-tool@2.0.0
mise exec -- my-tool --version
```

The command after `--` is an executable, which can differ from the tool name.

### Backend plugins {#backend-plugins}

Prefix each tool with the name you installed the plugin under:

```sh
mise ls-remote my-backend:some-tool
mise use my-backend:some-tool@1.0.0
```

```toml [mise.toml]
[tools]
"my-backend:some-tool" = "1.0.0"
```

The plugin's README lists the tools it provides. When the plugin can list them,
they also appear in [`mise search`](/cli/search.html) and in shell completion.

### Environment plugins {#environment-plugins}

An environment plugin adds a directive under `_` in `[env]`. The table holds the
options the plugin documents:

```toml [mise.toml]
[env]
_.my-env-plugin = { api_url = "https://api.example.com", debug = true }
```

`mise env` shows the variables it sets. mise reads two keys itself and does not
pass them to the plugin: `tools = true` runs the directive after the configured
tools are on `PATH`, and `redact = true` marks the values for
[redaction](/environments/secrets/#redaction). Install the plugin, or list it
under `[plugins]`, before you add the directive; see
[Troubleshooting](#troubleshooting).

### Package manager plugins {#package-plugins}

A package manager plugin adds a manager to
[`[bootstrap.packages]`](/bootstrap/packages/). The name you give the plugin is
the prefix for its packages:

```toml [mise.toml]
[bootstrap.plugins]
vscode = "https://github.com/your-org/mise-vscode-extensions"

[bootstrap.packages]
"vscode:ms-python.python" = "latest"
```

`mise bootstrap` installs the plugin, then the packages. To install it without
declaring it, run `mise plugins install package:vscode <url>`. The bootstrap
guide to [package manager plugins](/bootstrap/packages/plugins.html) covers the
commands and how mise tracks what it installed.

## Tool options {#tool-options}

The options on a tool's entry also go to its plugin. The plugin's README lists
the ones it reads:

```toml [mise.toml]
[tools]
my-tool = { version = "2.0.0", mirror = "https://mirror.example.com" }
```

mise keeps `os`, `depends`, `install_env`, `lazy`, `lazy_bins` and
`auto_update` to itself; see [tool options](/dev-tools/#tool-options). Every
other option reaches the plugin, including `postinstall` and
`minimum_release_age`, which mise also acts on. Lua hooks read options from
`ctx.options`. Install, uninstall and environment hooks, and the asdf scripts
that act on one installed version, also see them as `MISE_TOOL_OPTS__`
environment variables, here `MISE_TOOL_OPTS__MIRROR`; see
[tool plugin options](/tool-plugin-development.html#tool-options).

## Update and pin plugins {#update-plugins}

```sh
mise plugins update my-tool          # update one plugin
mise plugins update my-tool#v1.3.0   # check out another ref
mise plugins update                  # update every plugin
```

[`mise plugins update`](/cli/plugins/update.html) updates a plugin's code, not
the tool versions it installed. mise skips linked plugins and cannot update a
plugin installed from a zip archive. A packslip plugin keeps an explicit version
pin; `mise plugins update my-plugin#1.1.0` moves it to another release.

A version pin in `mise.toml` or `mise.lock` pins the tool, not the plugin's
code. Pin the plugin as well, with a ref in `[plugins]` or on the command line,
and review the changes before you move the ref.

To see installed plugins and their revisions, run:

```sh
mise plugins ls --urls
mise plugins ls --outdated   # plugins whose Git remote has newer commits
```

## Remove plugins {#remove-plugins}

```sh
mise plugins uninstall my-tool
```

[`mise plugins uninstall`](/cli/plugins/uninstall.html) removes the plugin's
code. The tool versions it installed stay, but mise needs the plugin to set up
their environment, so remove them first with
[`mise uninstall`](/cli/uninstall.html), or pass `--purge`. For a tool plugin,
`--purge` also deletes the tool's installs, downloads and cache; for a backend
plugin, it does the same for each `my-backend:<tool>` that is installed or in
config. It skips a tool whose directory another tool also uses, with a warning,
and leaves installs in shared and system install directories in place. Remove
the `[plugins]` and `[tools]` entries too, or mise installs the plugin again.

## Migrate an asdf plugin {#hook-migration}

An asdf plugin keeps working on Linux and macOS. To make it run on Windows and
give mise a checksum for each download, port it to a Lua tool plugin;
[asdf plugins (legacy)](/asdf-legacy-plugins.html) maps each asdf script to the
Lua hook that replaces it, and [Tool plugins](/tool-plugin-development.html)
describes the hooks.

## Security {#security-considerations}

A plugin runs with your permissions. It can read and write files, make network
requests and start programs, and Lua is not a sandbox. Read a plugin's source
before you install it and before each update, and pin it to a reviewed
revision. In [safe mode](/security.html#safe-mode), mise refuses to install
plugins. See [Security](/security.html) for trust and verification.

## Troubleshooting {#troubleshooting}

| Symptom                                                                | What to do                                                                                                                                                                  |
| ---------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `mise plugins install` fails to clone                                  | Check the URL and ref with `git ls-remote <url>`, and your Git credentials for a private repository.                                                                        |
| `Plugin my-tool already installed` and nothing changes                 | Run `mise plugins install --force my-tool`, with the URL or from `[plugins]`.                                                                                               |
| A warning that the plugin does not match `[plugins]`                   | Run `mise plugins install --force my-tool` to reinstall it from `[plugins]`.                                                                                                |
| A tool fails to install                                                | Run `MISE_DEBUG=1 mise install my-tool@2.0.0` to see the plugin's output. Check that the plugin supports your platform and that the programs it runs are installed.         |
| Every command in a project fails with `Invalid version: my-env-plugin` | The environment plugin named in `_.my-env-plugin` is not installed. List it under `[plugins]`, or run `mise plugins install` or `mise plugins link` from another directory. |
| An environment plugin's variables are missing                          | Run `mise env` and check that `mise plugins ls` shows the plugin under the directive's name. `mise env` output can contain secrets; do not paste it into issues.            |
