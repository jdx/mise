# `mise tasks deps`

**Usage**: `mise tasks deps [--hidden] [--dot] [TASKS]...`

**Source code**: [`src/cli/tasks/deps.rs`](https://github.com/jdx/mise/blob/main/src/cli/tasks/deps.rs)

[experimental] Display a tree visualization of a dependency graph

## Arguments

### `[TASKS]...`

Tasks to show dependencies for
Can specify multiple tasks by separating with spaces
e.g.: mise tasks deps lint test check

## Flags

### `--hidden`

Show hidden tasks

### `--dot`

Display dependencies in DOT format

Examples:

    # Show dependencies for all tasks
    $ mise tasks deps

    # Show dependencies for the "lint", "test" and "check" tasks
    $ mise tasks deps lint test check

    # Show dependencies in DOT format
    $ mise tasks deps --dot
