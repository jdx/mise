# `mise latest`

- **Usage**: `mise latest [-i --installed] <TOOL@VERSION>`
- **Source code**: [`src/cli/latest.rs`](https://github.com/jdx/mise/blob/main/src/cli/latest.rs)

Gets the latest available version for a plugin

Supports prefixes such as `node@20` to get the latest version of node 20.

## Arguments

### `<TOOL@VERSION>`

Tool to get the latest version of

## Flags

### `-i --installed`

Show latest installed instead of available version

Examples:

    $ mise latest node@20  # get the latest version of node 20
    20.0.0

    $ mise latest node     # get the latest stable version of node
    20.0.0
