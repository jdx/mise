# `mise tool`

- **Usage**: `mise tool [FLAGS] <BACKEND>`
- **Source code**: [`src/cli/tool.rs`](https://github.com/jdx/mise/blob/main/src/cli/tool.rs)

Gets information about a tool

## Arguments

### `<BACKEND>`

Tool name to get information about

## Flags

### `-J --json`

Output in JSON format

### `--backend`

Only show backend field

### `--installed`

Only show installed versions

### `--active`

Only show active versions

### `--requested`

Only show requested versions

### `--config-source`

Only show config source

### `--tool-options`

Only show tool options

Examples:

    $ mise tool node
    Backend:            core
    Installed Versions: 20.0.0 22.0.0
    Active Version:     20.0.0
    Requested Version:  20
    Config Source:      ~/.config/mise/mise.toml
    Tool Options:       [none]
