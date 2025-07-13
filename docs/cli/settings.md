# `mise settings`

- **Usage**: `mise settings [FLAGS] [SETTING] [VALUE] <SUBCOMMAND>`
- **Source code**: [`src/cli/settings/mod.rs`](https://github.com/jdx/mise/blob/main/src/cli/settings/mod.rs)

Show current settings

This is the contents of ~/.config/mise/config.toml

Note that aliases are also stored in this file
but managed separately with `mise aliases`

## Arguments

### `[SETTING]`

Name of setting

### `[VALUE]`

Setting value to set

## Global Flags

### `-l --local`

Use the local config file instead of the global one

## Flags

### `-a --all`

List all settings

### `-J --json`

Output in JSON format

### `--json-extended`

Output in JSON format with sources

### `-T --toml`

Output in TOML format

## Subcommands

- [`mise settings add [-l --local] <SETTING> <VALUE>`](/cli/settings/add.md)
- [`mise settings get [-l --local] <SETTING>`](/cli/settings/get.md)
- [`mise settings ls [FLAGS] [SETTING]`](/cli/settings/ls.md)
- [`mise settings set [-l --local] <SETTING> <VALUE>`](/cli/settings/set.md)
- [`mise settings unset [-l --local] <KEY>`](/cli/settings/unset.md)

Examples:
```
# list all settings
$ mise settings

# get the value of the setting "always_keep_download"
$ mise settings always_keep_download

# set the value of the setting "always_keep_download" to "true"
$ mise settings always_keep_download=true

# set the value of the setting "node.mirror_url" to "https://npm.taobao.org/mirrors/node"
$ mise settings node.mirror_url https://npm.taobao.org/mirrors/node
```
