# `mise tasks [flags] [subcommand]`

[experimental] Manage tasks

## Flags

### `--no-header`

Do not print table header

### `-x --extended`

Show all columns

### `--hidden`

Show hidden tasks

### `--sort <COLUMN>`

Sort by column. Default is name.

### `--sort-order <SORT_ORDER>`

Sort order. Default is asc.

### `-J --json`

Output in JSON format

## Subcommands

* [`mise tasks deps [args] [flags]`](/cli/tasks/deps.md)
* [`mise tasks edit <TASK> [-p --path]`](/cli/tasks/edit.md)
* [`mise tasks info <TASK> [-J --json]`](/cli/tasks/info.md)
* [`mise tasks ls [flags]`](/cli/tasks/ls.md)
* [`mise tasks run [args] [flags]`](/cli/tasks/run.md)

Examples:

    mise tasks ls
