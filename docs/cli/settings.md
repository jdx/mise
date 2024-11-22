# `mise settings`

- **Usage**: `mise settings [--names] [SETTING] <SUBCOMMAND>`
- **Source code**: [`src/cli/settings.rs`](https://github.com/jdx/mise/blob/main/src/cli/settings.rs)

Manage settings

## Arguments

### `[SETTING]`

## Flags

### `--names`

Only display key names for each setting

## Subcommands

- [`mise settings add <SETTING> <VALUE>`](/cli/settings/add.md)
- [`mise settings get <SETTING>`](/cli/settings/get.md)
- [`mise settings ls [--names] [KEY]`](/cli/settings/ls.md)
- [`mise settings set <SETTING> <VALUE>`](/cli/settings/set.md)
- [`mise settings unset <SETTING>`](/cli/settings/unset.md)

Examples:
    # list all settings
    $ mise settings

    # get the value of the setting "always_keep_download"
    $ mise settings always_keep_download

    # set the value of the setting "always_keep_download" to "true"
    $ mise settings always_keep_download=true
