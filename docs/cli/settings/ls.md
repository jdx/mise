# `mise settings ls`

- **Usage**: `mise settings ls [--names] [KEY]`
- **Aliases**: `list`
- **Source code**: [`src/cli/settings/ls.rs`](https://github.com/jdx/mise/blob/main/src/cli/settings/ls.rs)

Show current settings

This is the contents of ~/.config/mise/config.toml

Note that aliases are also stored in this file
but managed separately with `mise aliases`

## Arguments

### `[KEY]`

List keys under this key

## Flags

### `--names`

Only display key names for each setting

Examples:

    $ mise settings ls
    legacy_version_file = false
    ...

    $ mise settings ls python
    default_packages_file = "~/.default-python-packages"
    ...
