# `mise set [--file <FILE>] [-g --global] [ENV_VARS]...`

Manage environment variables

By default this command modifies ".mise.toml" in the current directory.

## Arguments

### `[ENV_VARS]...`

Environment variable(s) to set
e.g.: NODE_ENV=production

## Flags

### `--file <FILE>`

The TOML file to update

Defaults to MISE_DEFAULT_CONFIG_FILENAME environment variable, or ".mise.toml".

### `-g --global`

Set the environment variable in the global config file

Examples:

    $ mise set NODE_ENV=production

    $ mise set NODE_ENV
    production

    $ mise set
    key       value       source
    NODE_ENV  production  ~/.config/mise/config.toml
