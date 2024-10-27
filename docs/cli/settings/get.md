# `mise settings get`

**Usage**: `mise settings get <SETTING>`

**Source code**: [`src/cli/settings/get.rs`](https://github.com/jdx/mise/blob/main/src/cli/settings/get.rs)

Show a current setting

This is the contents of a single entry in ~/.config/mise/config.toml

Note that aliases are also stored in this file
but managed separately with `mise aliases get`

## Arguments

### `<SETTING>`

The setting to show

Examples:

    $ mise settings get legacy_version_file
    true
