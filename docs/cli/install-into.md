# `mise install-into`

- **Usage**: `mise install-into <TOOL@VERSION> <PATH>`
- **Source code**: [`src/cli/install_into.rs`](https://github.com/jdx/mise/blob/main/src/cli/install_into.rs)

Install a tool version to a specific path

Used for building a tool to a directory for use outside of mise

## Arguments

### `<TOOL@VERSION>`

Tool to install e.g.: node@20

### `<PATH>`

Path to install the tool into

Examples:

```
# install node@20.0.0 into ./mynode
$ mise install-into node@20.0.0 ./mynode && ./mynode/bin/node -v
20.0.0
```
