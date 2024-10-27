# `mise uninstall`

**Usage**: `mise uninstall [-a --all] [-n --dry-run] [INSTALLED_TOOL@VERSION]...`

**Source code**: [`src/cli/uninstall.rs`](https://github.com/jdx/mise/blob/main/src/cli/uninstall.rs)

**Aliases**: `remove`, `rm`

Removes installed tool versions

This only removes the installed version, it does not modify mise.toml.

## Arguments

### `[INSTALLED_TOOL@VERSION]...`

Tool(s) to remove

## Flags

### `-a --all`

Delete all installed versions

### `-n --dry-run`

Do not actually delete anything

Examples:

    # will uninstall specific version
    $ mise uninstall node@18.0.0

    # will uninstall the current node version (if only one version is installed)
    $ mise uninstall node

    # will uninstall all installed versions of node
    $ mise uninstall --all node@18.0.0 # will uninstall all node versions
