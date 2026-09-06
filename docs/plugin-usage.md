# Using Plugins

Use a plugin when an integration needs custom installation logic, environment directives,
or a host package manager. A tool does not need a registry shorthand or a plugin to be
installable: first check whether a built-in [backend](/dev-tools/backends/) accepts it directly.
For example, `mise use npm:prettier` uses the built-in npm backend.

## What Are Plugins?

Lua plugins support several interfaces. The [plugin overview](/plugins.html#choose-a-plugin-type)
compares backend, tool, environment, and package plugins. This page covers installing and
maintaining them; [asdf plugins](/asdf-legacy-plugins.html) have a separate compatibility guide.

### Backend Plugins

A backend plugin manages one or more tools using `plugin-name:tool`. For example,
`vfox-npm:prettier` selects `prettier` through a plugin installed as `vfox-npm`.

### Tool Plugins

A tool plugin manages one tool. Use the name under which it was installed, such as
`my-tool`. Lua runs on Windows, macOS, and Linux, but the plugin's own dependencies and
installation logic determine which platforms it supports.

## Installing Plugins

### From a Git Repository

```sh
# Substitute your plugin's name and repository URL.
mise plugin install my-plugin https://github.com/your-org/my-plugin
```

For a specific Git revision, append `#<ref>` to the URL:

```sh
mise plugin install my-plugin 'https://github.com/your-org/my-plugin#v1.0.0'
```

Use a commit ID when you need an immutable source revision. A tag or branch can move.
Review updates before changing the revision; a tool version pin is a separate choice.

### From Zip File

For an HTTPS archive, provide its URL instead of a Git URL:

```sh
mise plugin install my-plugin https://github.com/your-org/my-plugin/archive/refs/tags/v1.0.0.zip
```

The archive must contain a valid plugin. An unpacked archive has no Git checkout for
`mise plugin update`; install a new archive explicitly when updating it.

### From Local Directory

```sh
mise plugin link my-plugin /path/to/plugin/directory
```

Local plugins can also be declared in `mise.toml`:

```toml
[plugins]
my-plugin = "./plugins/my-plugin"
```

Absolute paths and `~/...` are supported. Explicit relative paths beginning with `./` or
`../` resolve from the config root of the declaring file. mise symlinks the directory,
so local edits take effect immediately. Existing installations are not replaced
automatically: use `mise plugins install --force my-plugin` to apply a changed local source,
or `mise plugins link --force my-plugin /path/to/plugin` when linking explicitly.

## Using Plugins (Advanced)

After installing a backend plugin, select a tool and then invoke its executable:

```sh
mise ls-remote my-backend:some-tool
mise use my-backend:some-tool@1.0.0
mise exec -- some-tool --version
```

To use a version for just one command without writing configuration:

```sh
mise exec my-backend:some-tool@1.0.0 -- some-tool --version
```

Replace the names with the plugin's documented identifiers. The argument after `--` is
an executable, which may differ from the tool name: a TypeScript package provides `tsc`,
for example. `mise install` alone installs a version; `mise use` also selects it in configuration.

## Plugin:Tool Format

The prefix is the installed backend plugin name, and the suffix identifies a tool within
that backend. It is not the syntax for an ordinary single-tool plugin. Use:

```toml
[plugins]
my-backend = "https://github.com/your-org/my-backend"
my-tool = "https://github.com/your-org/my-tool-plugin"

[tools]
"my-backend:some-tool" = "1.0.0"
my-tool = "2.0.0"
```

The repository and version values are illustrative; choose versions returned by the plugin.

## Managing Plugins

### List installed plugins

```sh
mise plugins ls
mise plugins ls --urls
```

### Update plugins

```sh
mise plugin update my-plugin
mise plugin update my-plugin#v1.1.0
mise plugin update  # all installed Git plugins
```

This updates plugin code, not the versions of tools it has installed. Local links are
skipped. Check the resulting revision with `mise plugins ls --urls`.

### Remove plugins

```sh
mise plugin remove my-plugin
```

By default this removes plugin code and retains installed tools. Those tools may still
need the plugin to resolve their executable paths or environment. Remove unneeded tool
versions with `mise uninstall` before removing the plugin. For a single-tool plugin,
`mise plugin remove --purge my-plugin` also removes its installs, downloads, and cache.
Remove corresponding `[plugins]` and `[tools]` entries if you no longer want the integration.

## Configuration

Declare plugin sources in `[plugins]` and tool versions in `[tools]`, as shown above.
Environment plugins use `[env]`; package plugins use `[bootstrap.packages]`. Configuration
options belong to the plugin's own interface, so consult its README before copying options
from another plugin.

## Finding Plugins

Check the [mise-plugins organization](https://github.com/mise-plugins), the
[mise discussions](https://github.com/jdx/mise/discussions), or your organization's repositories.
The [registry](/registry.html) contains some existing plugin-backed tool shorthands, but it
is not a general catalog of plugins and does not accept new asdf or vfox tool entries.

## Plugin Examples

### vfox-npm (Example Plugin)

[vfox-npm](https://github.com/jdx/vfox-npm) demonstrates a multi-tool backend. It is an example
for plugin development; use mise's built-in [npm backend](/dev-tools/backends/npm.html) for
normal npm tool installation:

```sh
mise use npm:prettier
mise exec -- prettier --check .
```

## Backend Plugins (Advanced)

Backend plugins implement `BackendListVersions`, `BackendInstall`, and optionally
`BackendExecEnv`. See [Backend Plugin Development](/backend-plugin-development.html) for
context fields and typed tool options.

## Tool Plugins (Advanced)

Tool plugins use hooks such as `Available`, `PreInstall`, `PostInstall`, and `EnvKeys`.
See [Tool Plugin Development](/tool-plugin-development.html) for lifecycle ordering and
idiomatic version-file support.

## Security Considerations

Plugin code runs with your permissions during installation and use. Inspect its source
and revisions before installing or updating it. Plugins can invoke external commands;
Lua does not provide an OS sandbox for those operations.

A tool pin in `mise.toml` or `mise.lock` does not pin plugin code. Pin the plugin repository
revision separately, and check which [lockfile guarantees](/dev-tools/mise-lock.html) its
backend supports. See [Security](/security.html) for safe mode and trust boundaries.

## Troubleshooting

### Plugin installation fails

Check the repository URL, revision, Git credentials, and `mise plugins ls --urls` output.
A local directory must have the plugin's expected files and hooks. To replace an existing
plugin, use `--force` only after checking the intended source.

### Tool installation fails

```sh
mise ls-remote my-backend:some-tool
mise install --verbose my-backend:some-tool@1.0.0
```

Confirm the plugin supports the host platform and that its required external programs are
available. Verbose output shows the underlying error; avoid publishing credentials from logs.

### Environment issues

```sh
mise ls --current
mise where my-backend:some-tool
mise exec my-backend:some-tool@1.0.0 -- some-tool --version
```

Use `mise where` instead of guessing an installation path. For environment plugins, check
`mise env --json` locally; its output may contain secrets. A successful installation alone
does not activate a tool in the current shell.

## Next Steps

- [Create a backend plugin](/backend-plugin-development.html).
- [Create a tool plugin](/tool-plugin-development.html).
- [Create an environment plugin](/env-plugin-development.html).
- [Create a package plugin](/package-plugin-development.html).
- [Publish a plugin](/plugin-publishing.html).
