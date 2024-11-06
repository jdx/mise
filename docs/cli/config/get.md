# `mise config get`

- **Usage**: `mise config get [-f --file <FILE>] [KEY]`
- **Source code**: [`src/cli/config/get.rs`](https://github.com/jdx/mise/blob/main/src/cli/config/get.rs)

Display the value of a setting in a mise.toml file

## Arguments

### `[KEY]`

The path of the config to display

## Flags

### `-f --file <FILE>`

The path to the mise.toml file to edit

If not provided, the nearest mise.toml file will be used

Examples:

    $ mise toml get tools.python
    3.12
