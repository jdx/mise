# `mise alias get`

- **Usage**: `mise alias get <PLUGIN> <ALIAS>`
- **Source code**: [`src/cli/alias/get.rs`](https://github.com/jdx/mise/blob/main/src/cli/alias/get.rs)

Show an alias for a plugin

This is the contents of an alias.&lt;PLUGIN> entry in ~/.config/mise/config.toml

## Arguments

### `<PLUGIN>`

The plugin to show the alias for

### `<ALIAS>`

The alias to show

Examples:

    $ mise alias get node lts-hydrogen
    20.0.0
