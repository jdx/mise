# `mise tasks`

- **Usage**: `mise tasks [FLAGS] [TASK] <SUBCOMMAND>`
- **Aliases**: `t`
- **Source code**: [`src/cli/tasks.rs`](https://github.com/jdx/mise/blob/main/src/cli/tasks.rs)

Manage tasks

## Arguments

### `[TASK]`

Task name to get info of

## Flags

### `--no-header`

Do not print table header

### `-x --extended`

Show all columns

### `--hidden`

Show hidden tasks

### `--sort <COLUMN>`

Sort by column. Default is name.

**Choices:**

- `name`
- `alias`
- `description`
- `source`

### `--sort-order <SORT_ORDER>`

Sort order. Default is asc.

**Choices:**

- `asc`
- `desc`

### `-J --json`

Output in JSON format

## Subcommands

- [`mise tasks deps [--hidden] [--dot] [TASKS]...`](/cli/tasks/deps.md)
- [`mise tasks edit [-p --path] <TASK>`](/cli/tasks/edit.md)
- [`mise tasks info [-J --json] <TASK>`](/cli/tasks/info.md)
- [`mise tasks ls [FLAGS]`](/cli/tasks/ls.md)
- [`mise tasks run [FLAGS] [TASK] [ARGS]...`](/cli/tasks/run.md)

Examples:

    mise tasks ls
