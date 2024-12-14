# `mise where`

- **Usage**: `mise where <TOOL@VERSION>`
- **Source code**: [`src/cli/where.rs`](https://github.com/jdx/mise/blob/main/src/cli/where.rs)

Display the installation path for a tool

The tool must be installed for this to work.

## Arguments

### `<TOOL@VERSION>`

Tool(s) to look up
e.g.: ruby@3
if "@&lt;PREFIX>" is specified, it will show the latest installed version
that matches the prefix
otherwise, it will show the current, active installed version

Examples:

```
# Show the latest installed version of node
# If it is is not installed, errors
$ mise where node@20
/home/jdx/.local/share/mise/installs/node/20.0.0
```

```
# Show the current, active install directory of node
# Errors if node is not referenced in any .tool-version file
$ mise where node
/home/jdx/.local/share/mise/installs/node/20.0.0
```
