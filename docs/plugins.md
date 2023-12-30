# Plugins

rtx uses asdf's plugin ecosystem under the hood. These plugins contain shell scripts like
`bin/install` (for installing) and `bin/list-all` (for listing all of the available versions).

See <https://github.com/rtx-plugins/registry> for the list of built-in plugins shorthands. See asdf's
[Create a Plugin](https://asdf-vm.com/plugins/create.html) for how to create your own or just learn
more about how they work.

## Core Plugins

rtx comes with some plugins built into the CLI written in Rust. These are new and will improve over
time. They can be easily overridden by installing a plugin with the same name, e.g.: `rtx plugin install python https://github.com/asdf-community/asdf-python`.

You can see the core plugins with `rtx plugin ls --core`.

- [Python](./docs/python.md)
- [NodeJS](./docs/node.md)
- [Ruby](./docs/ruby.md)
- [Go](./docs/go.md)
- [Java](./docs/java.md)
- [Deno](./docs/deno.md)
- [Bun](./docs/bun.md)

## Plugin Authors

<https://github.com/rtx-plugins> is a GitHub organization for community-developed plugins.
See [SECURITY.md](./SECURITY.md) for more details on how plugins here are treated differently.

If you'd like your plugin to be hosted here please let me know (GH discussion or discord is fine)
and I'd be happy to host it for you.

## Plugin Options

rtx has support for "plugin options" which is configuration specified in `.rtx.toml` to change behavior
of plugins. One example of this is virtualenv on python runtimes:

```toml
[tools]
python = {version='3.11', virtualenv='.venv'}
```

This will be passed to all plugin scripts as `RTX_TOOL_OPTS__VIRTUALENV=.venv`. The user can specify
any option and it will be passed to the plugin in that format.

Currently this only supports simple strings, but we can make it compatible with more complex types
(arrays, tables) fairly easily if there is a need for it.
