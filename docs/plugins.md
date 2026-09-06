# Plugins

Plugins add installation logic, environment directives, or bootstrap package managers to
mise. Most tools can use a built-in [backend](/dev-tools/backends/) directly, even when they
have no [registry](/registry.html) shorthand. Start there before installing a plugin.

For release binaries, prefer [packslip](/dev-tools/backends/packslip.html) when the publisher
provides signed manifests, then [aqua](/dev-tools/backends/aqua.html),
[github](/dev-tools/backends/github.html), or [gitlab](/dev-tools/backends/gitlab.html).
Use a plugin when your integration needs custom behavior those backends cannot provide.

Plugin code can read files, make requests, and run processes with your permissions. Lua's
cross-platform runtime does not make a plugin an OS sandbox. Review the source and its
updates; new asdf and vfox tool plugins are not accepted into the mise registry.

## Choose a plugin type

| Type        | Use it for                                            | Configuration                        | Author guide                                            |
| ----------- | ----------------------------------------------------- | ------------------------------------ | ------------------------------------------------------- |
| Backend     | Several versioned tools managed by one integration    | `[tools]`, `my-backend:tool`         | [Backend development](/backend-plugin-development.html) |
| Tool        | One versioned tool with download/install hooks        | `[tools]`, the installed plugin name | [Tool development](/tool-plugin-development.html)       |
| Environment | Variables or PATH entries without a tool installation | `[env]`, `_.my-plugin`               | [Environment development](/env-plugin-development.html) |
| Package     | Host-managed packages for machine bootstrap           | `[bootstrap.packages]`               | [Package development](/package-plugin-development.html) |
| asdf        | An existing shell-based tool integration              | `[tools]`, an asdf backend           | [Legacy plugins](/asdf-legacy-plugins.html)             |

Register a package manager in `[bootstrap.plugins]`, or install it as `package:<name>`,
before declaring its requests in `[bootstrap.packages]`. See the
[package plugin setup](/bootstrap/packages/plugins.html) for a complete configuration.

The Lua runtime is available on Windows, macOS, and Linux. Each plugin must still support
the selected platform and any external programs it invokes. asdf plugins use shell scripts
and are disabled by default on Windows.

## Backend Plugins

A backend plugin implements `BackendListVersions`, `BackendInstall`, and
`BackendExecEnv`. The prefix is the name under which you install the plugin:

```sh
# Replace this example repository and tool with your plugin's values.
mise plugin install my-backend https://github.com/your-org/my-backend
mise use my-backend:some-tool@1.0.0
mise exec -- some-tool --version
```

See [Using Plugins](/plugin-usage.html) for installation, local development, and updates.
The [backend template](https://github.com/jdx/mise-backend-plugin-template) provides a starting point.

## Tool Plugins

A tool plugin manages one tool through hooks such as `Available`, `PreInstall`, and
`EnvKeys`. Use its installed name as the tool name:

```sh
mise plugin install my-tool https://github.com/your-org/my-tool-plugin
mise use my-tool@1.0.0
mise exec -- my-tool --version
```

These repository and executable names are placeholders. Start with the
[tool template](https://github.com/jdx/mise-tool-plugin-template) when writing your own.

## Environment Plugins

An environment plugin implements `MiseEnv` and optionally `MisePath`. Install it before
using its directive:

```sh
mise plugin install my-env-plugin https://github.com/your-org/my-env-plugin
```

```toml
[env]
_.my-env-plugin = {
  api_url = "https://api.example.com",
  debug = true,
}
```

The fields are defined by the plugin. See [environment plugin development](/env-plugin-development.html)
for return values, cache behavior, and the [environment template](https://github.com/jdx/mise-env-plugin-template).

## Package plugins

Package plugins implement a host package manager for [bootstrap packages](/bootstrap/packages/plugins.html).
They operate on batches of package requests and report installed state. Their installations
belong to the host manager, unlike versioned tools stored under mise's data directory.
See [Package Plugin Development](/package-plugin-development.html) for the hook contract.

## General Plugin Usage

[Using Plugins](/plugin-usage.html) explains repository URLs, archive installation, local
links, pinning plugin revisions, and diagnostics. List what is already installed with:

```sh
mise plugins ls --urls
```

## asdf (Legacy) Plugins

mise can run existing asdf plugins with scripts such as `bin/list-all`, `bin/install`, and
`bin/exec-env`. Use the [legacy guide](/asdf-legacy-plugins.html) to maintain one, or the
[hook migration table](/dev-tools/backends/asdf.html#hook-migration-asdf-to-vfox) to port it to Lua.

## Plugin Authors

The [mise-plugins organization](https://github.com/mise-plugins) hosts community plugins.
Contact the maintainers through a [GitHub discussion](https://github.com/jdx/mise/discussions)
to discuss hosting. Hosting a plugin and adding a registry shorthand are separate decisions;
see the [publishing guide](/plugin-publishing.html).

## Tool Options

Plugins define custom options in their tool configuration. For example, a plugin that
supports a `mirror` option could accept:

```toml
[tools]
my-tool = {
  version = "1.0.0",
  mirror = "https://mirror.example.com",
}
```

For asdf and version-specific vfox lifecycle hooks, that option is exposed as
`MISE_TOOL_OPTS__MIRROR`. These variables are scoped to hook execution, not exported into
your shell. mise-managed fields such as `depends`, `install_env`, and `os` are handled by
mise instead. Backend plugin hooks receive typed options through `ctx.options`, including
arrays and nested tables; see [backend context](/backend-plugin-development.html#context-variables).

## Templates

Repository values in `[plugins]` support [templates](/templates.html). Prefer a normal SSH
or HTTPS repository URL and your Git credential setup for private repositories; embedding
credentials in a URL can expose them in configuration or logs.

```toml
[plugins]
my-backend = "git@github.com:your-org/my-backend.git"
```
