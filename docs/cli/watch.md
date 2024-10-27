# `mise watch`

**Usage**: `mise watch [-t --task... <TASK>] [-g --glob... <GLOB>] [ARGS]...`

**Source code**: [`src/cli/watch.rs`](https://github.com/jdx/mise/blob/main/src/cli/watch.rs)

**Aliases**: `w`

[experimental] Run task(s) and watch for changes to rerun it

This command uses the `watchexec` tool to watch for changes to files and rerun the specified task(s).
It must be installed for this command to work, but you can install it with `mise use -g watchexec@latest`.

## Arguments

### `[ARGS]...`

Extra arguments

## Flags

### `-t --task... <TASK>`

Tasks to run

### `-g --glob... <GLOB>`

Files to watch
Defaults to sources from the tasks(s)

Examples:

    $ mise watch -t build
    Runs the "build" tasks. Will re-run the tasks when any of its sources change.
    Uses "sources" from the tasks definition to determine which files to watch.

    $ mise watch -t build --glob src/**/*.rs
    Runs the "build" tasks but specify the files to watch with a glob pattern.
    This overrides the "sources" from the tasks definition.

    $ mise run -t build --clear
    Extra arguments are passed to watchexec. See `watchexec --help` for details.
