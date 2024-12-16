# `mise tasks add`

- **Usage**: `mise tasks add [FLAGS] <TASK> <RUN>...`
- **Source code**: [`src/cli/tasks/add.rs`](https://github.com/jdx/mise/blob/main/src/cli/tasks/add.rs)

Create a new task

## Arguments

### `<TASK>`

Tasks name to add

### `<RUN>...`

## Flags

### `--description <DESCRIPTION>`

Description of the task

### `-a --alias... <ALIAS>`

Other names for the task

### `--depends-post... <DEPENDS_POST>`

Dependencies to run after the task runs

### `-w --wait-for... <WAIT_FOR>`

Wait for these tasks to complete if they are to run

### `-D --dir <DIR>`

Run the task in a specific directory

### `-H --hide`

Hide the task from `mise task` and completions

### `-r --raw`

Directly connect stdin/stdout/stderr

### `-s --sources... <SOURCES>`

Glob patterns of files this task uses as input

### `--outputs... <OUTPUTS>`

Glob patterns of files this task creates, to skip if they are not modified

### `--shell <SHELL>`

Run the task in a specific shell

### `-q --quiet`

Do not print the command before running

### `--silent`

Do not print the command or its output

### `-d --depends... <DEPENDS>`

Add dependencies to the task

### `--run-windows <RUN_WINDOWS>`

Command to run on windows

### `-f --file`

Create a file task instead of a toml task

Examples:

```
mise task add pre-commit --depends "test" --depends "render" -- echo pre-commit
```
