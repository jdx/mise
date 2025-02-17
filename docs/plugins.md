# Plugins

Plugins in mise are a way to extend `mise` with new functionality like extra tools or environment variable management.

Historically it was the only way to add new tools (as the only backend was [asdf](/dev-tools/backends/asdf.html)).

The way that backend works is every tool has its own plugin which needs to be manually installed. However, now with [core tools](/core-tools.html)
and backends like [aqua](/dev-tools/backends/aqua.html)/[ubi](/dev-tools/backends/ubi.html), plugins are no longer necessary to run most tools in mise.

Tool plugins should be avoided for security reasons. New tools will not be accepted into mise built with asdf/vfox plugins unless they are very popular and
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

## asdf Plugins

mise can use asdf's plugin ecosystem under the hood. These plugins contain shell scripts like
`bin/install` (for installing) and `bin/list-all` (for listing all of the available versions).

See <https://github.com/jdx/mise/blob/main/registry.toml> for the list of built-in plugins shorthands. See asdf's
[Create a Plugin](https://asdf-vm.com/plugins/create.html) for how to create your own or just learn
more about how they work.

## vfox Plugins

Similarly, mise can also use [vfox plugins](/dev-tools/backends/vfox.html). These have the advantage of working on Windows so are preferred.

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
