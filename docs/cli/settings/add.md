# `mise settings add`

**Usage**: `mise settings add <SETTING> <VALUE>`

Adds a setting to the configuration file

Used with an array setting, this will append the value to the array.
This modifies the contents of ~/.config/mise/config.toml

## Arguments

### `<SETTING>`

The setting to set

### `<VALUE>`

The value to set

Examples:

    mise settings add disable_hints python_multi
