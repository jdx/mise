# `mise tasks ls`

- **Usage**: `mise tasks ls [FLAGS]`
- **Source code**: [`src/cli/tasks/ls.rs`](https://github.com/jdx/mise/blob/main/src/cli/tasks/ls.rs)

List available tasks to execute
These may be included from the config file or from the project's .mise/tasks directory
mise will merge all tasks from all parent directories into this list.

So if you have global tasks in `~/.config/mise/tasks/*` and project-specific tasks in
~/myproject/.mise/tasks/*, then they'll both be available but the project-specific
tasks will override the global ones if they have the same name.

## Flags

### `-x --extended`

Show all columns

### `--no-header`

Do not print table header

### `--hidden`

Show hidden tasks

### `-g --global`

Only show global tasks

### `-J --json`

Output in JSON format

### `-l --local`

Only show non-global tasks

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

Examples:

```
mise tasks ls
```
