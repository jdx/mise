# `mise settings ls`

**Usage**: `mise settings ls [--keys]`

**Source code**: [`src/cli/settings/ls.rs`](https://github.com/jdx/mise/blob/main/src/cli/settings/ls.rs)

**Aliases**: `list`

Show current settings

This is the contents of ~/.config/mise/config.toml

Note that aliases are also stored in this file
but managed separately with `mise aliases`

## Flags

### `--keys`

Only display key names for each setting

Examples:

    $ mise settings
    legacy_version_file = false
