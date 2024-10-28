# `mise tasks edit`

**Usage**: `mise tasks edit [-p --path] <TASK>`

**Source code**: [`src/cli/tasks/edit.rs`](https://github.com/jdx/mise/blob/main/src/cli/tasks/edit.rs)

Edit a tasks with $EDITOR

The tasks will be created as a standalone script if it does not already exist.

## Arguments

### `<TASK>`

Tasks to edit

## Flags

### `-p --path`

Display the path to the tasks instead of editing it

Examples:

    mise tasks edit build
    mise tasks edit test
