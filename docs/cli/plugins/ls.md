# `mise plugins ls`

**Usage**: `mise plugins ls [FLAGS]`

**Source code**: [`src/cli/plugins/ls.rs`](https://github.com/jdx/mise/blob/main/src/cli/plugins/ls.rs)

**Aliases**: `list`

List installed plugins

Can also show remotely available plugins to install.

## Flags

### `-c --core`

The built-in plugins only
Normally these are not shown

### `--user`

List installed plugins

This is the default behavior but can be used with --core
to show core and user plugins

### `-u --urls`

Show the git url for each plugin
e.g.: <https://github.com/asdf-vm/asdf-nodejs.git>

Examples:

    $ mise plugins ls
    node
    ruby

    $ mise plugins ls --urls
    node    https://github.com/asdf-vm/asdf-nodejs.git
    ruby    https://github.com/asdf-vm/asdf-ruby.git
