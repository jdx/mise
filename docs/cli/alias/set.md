# `mise alias set`

**Usage**: `mise alias set <ARGS>…`

**Source code**: [`src/cli/alias/set.rs`](https://github.com/jdx/mise/blob/main/src/cli/alias/set.rs)

**Aliases**: `add`, `create`

Add/update an alias for a plugin

This modifies the contents of ~/.config/mise/config.toml

## Arguments

### `<PLUGIN>`

The plugin to set the alias for

### `<ALIAS>`

The alias to set

### `<VALUE>`

The value to set the alias to

Examples:

    mise alias set node lts-jod 22.0.0
