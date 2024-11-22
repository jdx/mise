# `mise settings set`

- **Usage**: `mise settings set <SETTING> <VALUE>`
- **Aliases**: `create`
- **Source code**: [`src/cli/settings/set.rs`](https://github.com/jdx/mise/blob/main/src/cli/settings/set.rs)

Add/update a setting

This modifies the contents of ~/.config/mise/config.toml

## Arguments

### `<SETTING>`

The setting to set

### `<VALUE>`

The value to set

Examples:

    mise settings legacy_version_file=true
