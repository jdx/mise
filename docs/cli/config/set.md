# `mise config set`

- **Usage**: `mise config set [-f --file <FILE>] [-t --type <TYPE>] <KEY> <VALUE>`
- **Source code**: [`src/cli/config/set.rs`](https://github.com/jdx/mise/blob/main/src/cli/config/set.rs)

Set the value of a setting in a mise.toml file

## Arguments

### `<KEY>`

The path of the config to display

### `<VALUE>`

The value to set the key to

## Flags

### `-f --file <FILE>`

The path to the mise.toml file to edit

If not provided, the nearest mise.toml file will be used

### `-t --type <TYPE>`

**Choices:**

- `infer`
- `string`
- `integer`
- `float`
- `bool`
- `list`

Examples:

    $ mise config set tools.python 3.12
    $ mise config set settings.always_keep_download true
    $ mise config set env.TEST_ENV_VAR ABC
    $ mise config set settings.disable_tools --type list node,rust

    # Type for `settings` is inferred
    $ mise config set settings.jobs 4
