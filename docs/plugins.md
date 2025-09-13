# Plugins

Plugins in mise are a way to extend `mise` with new functionality like extra tools or environment variable management.

Historically it was the only way to add new tools (as the only backend was [asdf](/dev-tools/backends/asdf.html)).

The way that backend works is every tool has its own plugin which needs to be manually installed. However, now with [core tools](/core-tools.html)
and backends like [aqua](/dev-tools/backends/aqua.html)/[ubi](/dev-tools/backends/ubi.html), plugins are no longer necessary to run most tools in mise.

Tool plugins should be avoided for security reasons. New tools will not be accepted into mise built with asdf/plugins unless they are very popular and
aqua/ubi is not an option for some reason.

The only exception is if the tool needs to set env vars or has a complex installation process, as plugins can provide functionality like [setting env vars globally](/environments/#plugin-provided-env-directives) without relying on a tool being installed. They can also provide [aliases for versions](/dev-tools/aliases.html#aliased-versions).

If you want to integrate a new tool into mise, you should either try to get it into the [aqua registry](https://mise.jdx.dev/dev-tools/backends/aqua.html)
or see if it can be installed with [ubi](https://mise.jdx.dev/dev-tools/backends/ubi.html). Then add it to the [registry](https://github.com/jdx/mise/blob/main/registry.toml).
Aqua is definitely preferred to ubi as it has better UX and more features like slsa verification and the ability to use different logic for older versions.

You can manage all installed plugins in `mise` with [`mise plugins`](/cli/plugins.html).

```shell
mise plugins ls --urls
# Plugin                          Url                                                     Ref  Sha
# 1password                       https://github.com/mise-plugins/mise-1password-cli.git  HEAD f5d5aab
# vfox-mise-plugins-vfox-dart     https://github.com/mise-plugins/vfox-dart               HEAD 1424253
# ...
```

## Backend Plugins

Backend plugins provide enhanced functionality with modern backend methods. These plugins use the `plugin:tool` format and offer advantages over traditional plugins:

- **Multiple Tools**: A single plugin can manage multiple tools
- **Enhanced Methods**: Backend methods for listing versions, installing, and setting environment variables
- **Cross-platform**: Work on Windows, macOS, and Linux
- **Performance**: Faster execution than shell-based plugins

Example usage:

```bash
# Install a backend plugin
mise plugin install my-plugin https://github.com/username/my-plugin

# Use the plugin:tool format
mise install my-plugin:some-tool@1.0.0
mise use my-plugin:some-tool@latest
```

See [Backend Plugin Development](backend-plugin-development.md) for creating backend plugins. You can start quickly with the [mise-backend-plugin-template](https://github.com/jdx/mise-backend-plugin-template).

## Tool Plugins

Tool plugins use the traditional hook-based approach with Lua scripts. These plugins provide:

- **Hook-based**: Use hooks like `PreInstall`, `PostInstall`, `Available`, etc.
- **Single Tool**: Each plugin manages one tool
- **Cross-platform**: Work on Windows, macOS, and Linux
- **Flexible**: Full control over installation and environment setup

Example usage:

```bash
# Install a tool plugin
mise plugin install my-tool https://github.com/username/my-tool-plugin

# Use the tool directly
mise install my-tool@1.0.0
mise use my-tool@latest
```

See [Tool Plugin Development](tool-plugin-development.md) for creating tool plugins. The [mise-tool-plugin-template](https://github.com/jdx/mise-tool-plugin-template) provides a ready-to-use starting point.

## General Plugin Usage

For end-user documentation on installing and using both backend and tool plugins, see [Using Plugins](plugin-usage.md).

## asdf (Legacy) Plugins

mise can use asdf's plugin ecosystem under the hood for backward compatibility. These plugins contain shell scripts like
`bin/install` (for installing) and `bin/list-all` (for listing all of the available versions).

asdf plugins have limitations compared to modern backends and should only be used when necessary. They only work on Linux/macOS and are slower than native backends.

See [asdf (Legacy) Plugins](asdf-legacy-plugins.md) for comprehensive documentation on using and creating these plugins.

## Plugin Authors

<https://github.com/mise-plugins> is a GitHub organization for community-developed plugins.
See [SECURITY.md](https://github.com/jdx/mise/blob/main/SECURITY.md) for more details on how plugins here are treated differently.

If you'd like your plugin to be hosted here please let me know (GH discussion or discord is fine)
and I'd be happy to host it for you.

## Tool Options

mise has support for "tool options" which is configuration specified in `mise.toml` to change behavior
of tools. One example of this is virtualenv on python runtimes:

```toml
[tools]
python = { version='3.11', virtualenv='.venv' }
```

This will be passed to all plugin scripts as `MISE_TOOL_OPTS__VIRTUALENV=.venv`. The user can specify
any option, and it will be passed to the plugin in that format.

Currently, this only supports simple strings, but we can make it compatible with more complex types
(arrays, tables) fairly easily if there is a need for it.

## Templates

Plugin custom repository values can be templates, see [Templates](/templates) for details.

```toml
[plugins]
my-plugin = "https://{{ get_env(name='GIT_USR', default='empty') }}:{{ get_env(name='GIT_PWD', default='empty') }}@github.com/foo/my-plugin.git"
```
