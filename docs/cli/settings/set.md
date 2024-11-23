# `mise settings set`

- **Usage**: `mise settings set [-l --local] <KEY> <VALUE>`
- **Aliases**: `create`
- **Source code**: [`src/cli/settings/set.rs`](https://github.com/jdx/mise/blob/main/src/cli/settings/set.rs)

Add/update a setting

This modifies the contents of ~/.config/mise/config.toml

## Arguments

### `<KEY>`

The setting to set

### `<VALUE>`

The value to set

## Flags

### `-l --local`

Use the local config file instead of the global one

Examples:

    mise settings legacy_version_file=true
