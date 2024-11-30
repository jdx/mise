# `mise settings`

- **Usage**: `mise settings [FLAGS] [KEY] [VALUE] <SUBCOMMAND>`
- **Source code**: [`src/cli/settings.rs`](https://github.com/jdx/mise/blob/main/src/cli/settings.rs)

Manage settings

## Arguments

### `[KEY]`

Setting name to get/set

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

- [`mise settings add [-l --local] <KEY> <VALUE>`](/cli/settings/add.md)
- [`mise settings get [-l --local] <KEY>`](/cli/settings/get.md)
- [`mise settings ls [FLAGS] [KEY]`](/cli/settings/ls.md)
- [`mise settings set [-l --local] <KEY> <VALUE>`](/cli/settings/set.md)
- [`mise settings unset [-l --local] <KEY>`](/cli/settings/unset.md)

Examples:
    # list all settings
    $ mise settings

    # get the value of the setting "always_keep_download"
    $ mise settings always_keep_download

    # set the value of the setting "always_keep_download" to "true"
    $ mise settings always_keep_download=true

    # set the value of the setting "node.mirror_url" to "https://npm.taobao.org/mirrors/node"
    $ mise settings node.mirror_url https://npm.taobao.org/mirrors/node
