# Plugins

mise uses asdf's plugin ecosystem under the hood. These plugins contain shell scripts like
`bin/install` (for installing) and `bin/list-all` (for listing all of the available versions).

See <https://github.com/mise-plugins/registry> for the list of built-in plugins shorthands. See asdf's
[Create a Plugin](https://asdf-vm.com/plugins/create.html) for how to create your own or just learn
more about how they work.

## Core Plugins

mise comes with some plugins built into the CLI written in Rust. These are new and will improve over
time. They can be easily overridden by installing a plugin with the same name, e.g.: `mise plugin install python https://github.com/asdf-community/asdf-python`.

You can see the core plugins with `mise plugin ls --core`.

- [Bun](/lang/bun)
- [Deno](/lang/deno)
- [Erlang](/lang/erlang) <Badge type="warning" text="experimental" />
- [Go](/lang/go)
- [Java](/lang/java)
- [NodeJS](/lang/node)
- [Python](/lang/python)
- [Ruby](/lang/ruby)

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
