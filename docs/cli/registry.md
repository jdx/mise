# `mise registry`

**Usage**: `mise registry [NAME]`

**Source code**: [`src/cli/registry.rs`](https://github.com/jdx/mise/blob/main/src/cli/registry.rs)

List available tools to install

This command lists the tools available in the registry as shorthand names.

For example, `poetry` is shorthand for `asdf:mise-plugins/mise-poetry`.

## Arguments

### `[NAME]`

Show only the specified tool's full name

Examples:

    $ mise registry
    node    core:node
    poetry  asdf:mise-plugins/mise-poetry
    ubi     cargo:ubi-cli

    $ mise registry poetry
    asdf:mise-plugins/mise-poetry
