# `mise run`

**Usage**: `mise run [FLAGS]`

**Aliases**: `r`

[experimental] Run task(s)

This command will run a tasks, or multiple tasks in parallel.
Tasks may have dependencies on other tasks or on source files.
If source is configured on a tasks, it will only run if the source
files have changed.

Tasks can be defined in mise.toml or as standalone scripts.
In mise.toml, tasks take this form:

    [tasks.build]
    run = "npm run build"
    sources = ["src/**/*.ts"]
    outputs = ["dist/**/*.js"]

Alternatively, tasks can be defined as standalone scripts.
These must be located in `mise-tasks`, `.mise-tasks`, `.mise/tasks`, `mise/tasks` or
`.config/mise/tasks`.
The name of the script will be the name of the tasks.

    $ cat .mise/tasks/build<<EOF
    #!/usr/bin/env bash
    npm run build
    EOF
    $ mise run build

## Flags

### `-C --cd <CD>`

Change to this directory before executing the command

### `-n --dry-run`

Don't actually run the tasks(s), just print them in order of execution

### `-f --force`

Force the tasks to run even if outputs are up to date

### `-p --prefix`

Print stdout/stderr by line, prefixed with the tasks's label
Defaults to true if --jobs > 1
Configure with `task_output` config or `MISE_TASK_OUTPUT` env var

### `-i --interleave`

Print directly to stdout/stderr instead of by line
Defaults to true if --jobs == 1
Configure with `task_output` config or `MISE_TASK_OUTPUT` env var

### `-t --tool... <TOOL@VERSION>`

Tool(s) to also add e.g.: node@20 python@3.10

### `-j --jobs <JOBS>`

Number of tasks to run in parallel
[default: 4]
Configure with `jobs` config or `MISE_JOBS` env var

### `-r --raw`

Read/write directly to stdin/stdout/stderr instead of by line
Configure with `raw` config or `MISE_RAW` env var

### `--timings`

Shows elapsed time after each tasks

Examples:

    # Runs the "lint" tasks. This needs to either be defined in mise.toml
    # or as a standalone script. See the project README for more information.
    $ mise run lint

    # Forces the "build" tasks to run even if its sources are up-to-date.
    $ mise run build --force

    # Run "test" with stdin/stdout/stderr all connected to the current terminal.
    # This forces `--jobs=1` to prevent interleaving of output.
    $ mise run test --raw

    # Runs the "lint", "test", and "check" tasks in parallel.
    $ mise run lint ::: test ::: check

    # Execute multiple tasks each with their own arguments.
    $ mise tasks cmd1 arg1 arg2 ::: cmd2 arg1 arg2
