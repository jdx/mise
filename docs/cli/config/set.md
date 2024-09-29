# `mise config set [-f --file <FILE>] [-t --type <TYPE>] <KEY> <VALUE>`

Display the value of a setting in a mise.toml file

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

Examples:

    mise config set tools.python 3.12
    mise config set settings.always_keep_download true
    mise config set env.TEST_ENV_VAR ABC
