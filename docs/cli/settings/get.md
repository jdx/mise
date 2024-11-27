# `mise settings get`

- **Usage**: `mise settings get [-l --local] <KEY>`
- **Source code**: [`src/cli/settings/get.rs`](https://github.com/jdx/mise/blob/main/src/cli/settings/get.rs)

Show a current setting

This is the contents of a single entry in ~/.config/mise/config.toml

Note that aliases are also stored in this file
but managed separately with `mise aliases get`

## Arguments

### `<KEY>`

The setting to show

## Flags

### `-l --local`

Use the local config file instead of the global one

Examples:

    $ mise settings get idiomatic_version_file
    true
