# `mise settings add`

- **Usage**: `mise settings add [-l --local] <KEY> <VALUE>`
- **Source code**: [`src/cli/settings/add.rs`](https://github.com/jdx/mise/blob/main/src/cli/settings/add.rs)

Adds a setting to the configuration file

Used with an array setting, this will append the value to the array.
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

```
mise settings add disable_hints python_multi
```
