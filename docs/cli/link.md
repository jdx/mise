# `mise link`

**Usage**: `mise link [-f --force] <TOOL@VERSION> <PATH>`

**Source code**: [`src/cli/link.rs`](https://github.com/jdx/mise/blob/main/src/cli/link.rs)

**Aliases**: `ln`

Symlinks a tool version into mise

Use this for adding installs either custom compiled outside mise or built with a different tool.

## Arguments

### `<TOOL@VERSION>`

Tool name and version to create a symlink for

### `<PATH>`

The local path to the tool version
e.g.: ~/.nvm/versions/node/v20.0.0

## Flags

### `-f --force`

Overwrite an existing tool version if it exists

Examples:

    # build node-20.0.0 with node-build and link it into mise
    $ node-build 20.0.0 ~/.nodes/20.0.0
    $ mise link node@20.0.0 ~/.nodes/20.0.0

    # have mise use the python version provided by Homebrew
    $ brew install node
    $ mise link node@brew $(brew --prefix node)
    $ mise use node@brew
