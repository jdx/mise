# `mise settings ls`

- **Usage**: `mise settings ls [FLAGS] [SETTING]`
- **Aliases**: `list`
- **Source code**: [`src/cli/settings/ls.rs`](https://github.com/jdx/mise/blob/main/src/cli/settings/ls.rs)

Show current settings

This is the contents of ~/.config/mise/config.toml

Note that aliases are also stored in this file
but managed separately with `mise aliases`

## Arguments

### `[SETTING]`

Name of setting

## Flags

### `-a --all`

List all settings

### `-l --local`

Use the local config file instead of the global one

### `-J --json`

Output in JSON format

### `--json-extended`

Output in JSON format with sources

### `-T --toml`

Output in TOML format

Examples:

```
$ mise settings ls
idiomatic_version_file = false
...

$ mise settings ls python
default_packages_file = "~/.default-python-packages"
...
```
