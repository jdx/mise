# `mise shell`

- **Usage**: `mise shell [FLAGS] [TOOL@VERSION]...`
- **Aliases**: `sh`
- **Source code**: [`src/cli/shell.rs`](https://github.com/jdx/mise/blob/main/src/cli/shell.rs)

Sets a tool version for the current session.

Only works in a session where mise is already activated.

This works by setting environment variables for the current shell session
such as `MISE_NODE_VERSION=20` which is "eval"ed as a shell function created by `mise activate`.

## Arguments

### `[TOOL@VERSION]...`

Tool(s) to use

## Flags

### `-j --jobs <JOBS>`

Number of jobs to run in parallel
[default: 4]

### `--raw`

Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1

### `-u --unset`

Removes a previously set version

Examples:

    $ mise shell node@20
    $ node -v
    v20.0.0
