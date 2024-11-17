# `mise plugins ls`

- **Usage**: `mise plugins ls [-u --urls]`
- **Aliases**: `list`
- **Source code**: [`src/cli/plugins/ls.rs`](https://github.com/jdx/mise/blob/main/src/cli/plugins/ls.rs)

List installed plugins

Can also show remotely available plugins to install.

## Flags

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
