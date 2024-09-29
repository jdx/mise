# `mise unset [-f --file <FILE>] [-g --global] [KEYS]...`

Remove environment variable(s) from the config file.

By default, this command modifies `mise.toml` in the current directory.

## Arguments

### `[KEYS]...`

Environment variable(s) to remove
e.g.: NODE_ENV

## Flags

### `-f --file <FILE>`

Specify a file to use instead of `mise.toml`

### `-g --global`

Use the global config file

Examples:

    # Remove NODE_ENV from the current directory's config
    $ mise unset NODE_ENV
    
    # Remove NODE_ENV from the global config
    $ mise unset NODE_ENV -g
