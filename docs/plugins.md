# Plugins

Plugins in mise extend functionality. Historically they were the only way to add new tools as the only backend was asdf the way
that backend works is every tool has its own plugin which needs to be manually installed. However now with core languages and
backends like aqua/ubi, plugins are no longer necessary to run most tools in mise.

Meanwhile, plugins have expanded beyond tools and can provide functionality like [setting env vars globally](/environments.html#plugin-provided-env-directives) without relying on a tool being installed.

Tool plugins should be avoided for security reasons. New tools will not be accepted into mise built with asdf/vfox plugins unless they are very popular and
aqua/ubi is not an option for some reason.
If you want to integrate a new tool into mise, you should either try to get it into the [aqua registry](https://mise.jdx.dev/dev-tools/backends/ubi.html)
or see if it can be installed with [ubi](https://mise.jdx.dev/dev-tools/backends/ubi.html). Then add it to the [registry](https://github.com/jdx/mise/blob/main/registry.toml).
Aqua is definitely preferred to ubi as it has better UX and more features like slsa verification and the ability to use different logic for older versions.

## asdf Plugins

mise uses asdf's plugin ecosystem under the hood. These plugins contain shell scripts like
`bin/install` (for installing) and `bin/list-all` (for listing all of the available versions).

See <https://github.com/mise-plugins/registry> for the list of built-in plugins shorthands. See asdf's
[Create a Plugin](https://asdf-vm.com/plugins/create.html) for how to create your own or just learn
more about how they work.

## vfox Plugins

Similarly, mise can also use [vfox plugins](https://mise.jdx.dev/dev-tools/backends/vfox.html). These have the advantage of working on Windows so are preferred.

## Plugin Authors

<https://github.com/mise-plugins> is a GitHub organization for community-developed plugins.
See [SECURITY.md](https://github.com/jdx/mise/blob/main/SECURITY.md) for more details on how plugins here are treated differently.

If you'd like your plugin to be hosted here please let me know (GH discussion or discord is fine)
and I'd be happy to host it for you.

## Plugin Options

mise has support for "plugin options" which is configuration specified in `mise.toml` to change behavior
of plugins. One example of this is virtualenv on python runtimes:

```toml
[tools]
python = {version='3.11', virtualenv='.venv'}
```

This will be passed to all plugin scripts as `MISE_TOOL_OPTS__VIRTUALENV=.venv`. The user can specify
any option and it will be passed to the plugin in that format.

Currently this only supports simple strings, but we can make it compatible with more complex types
(arrays, tables) fairly easily if there is a need for it.

## Templates

Plugin custom repository values can be templates, see [Templates](/templates) for details.

```toml
[plugins]
my-plugin = "https://{{ get_env(name='GIT_USR', default='empty') }}:{{ get_env(name='GIT_PWD', default='empty') }}@github.com/foo/my-plugin.git"
```
