# `mise settings unset`

- **Usage**: `mise settings unset <SETTING>`
- **Aliases**: `rm`, `remove`, `delete`, `del`
- **Source code**: [`src/cli/settings/unset.rs`](https://github.com/jdx/mise/blob/main/src/cli/settings/unset.rs)

Clears a setting

This modifies the contents of ~/.config/mise/config.toml

## Arguments

### `<SETTING>`

The setting to remove

Examples:

    mise settings unset legacy_version_file
