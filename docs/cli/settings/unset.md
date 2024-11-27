# `mise settings unset`

- **Usage**: `mise settings unset [-l --local] <KEY>`
- **Aliases**: `rm`, `remove`, `delete`, `del`
- **Source code**: [`src/cli/settings/unset.rs`](https://github.com/jdx/mise/blob/main/src/cli/settings/unset.rs)

Clears a setting

This modifies the contents of ~/.config/mise/config.toml

## Arguments

### `<KEY>`

The setting to remove

## Flags

### `-l --local`

Use the local config file instead of the global one

Examples:

    mise settings unset idiomatic_version_file
