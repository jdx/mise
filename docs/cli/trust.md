# `mise trust [flags] [CONFIG_FILE]`

Marks a config file as trusted

This means mise will parse the file with potentially dangerous
features enabled.

This includes:

- environment variables
- templates
- `path:` plugin versions

## Arguments

### `[CONFIG_FILE]`

The config file to trust

## Flags

### `-a --all`

Trust all config files in the current directory and its parents

### `--untrust`

No longer trust this config

### `--show`

Show the trusted status of config files from the current directory and its parents.
Does not trust or untrust any files.

Examples:

    # trusts ~/some_dir/mise.toml
    $ mise trust ~/some_dir/mise.toml

    # trusts mise.toml in the current or parent directory
    $ mise trust
