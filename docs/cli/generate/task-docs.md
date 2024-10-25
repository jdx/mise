# `mise generate task-docs`

**Usage**: `mise generate task-docs [FLAGS]`

[experimental] Generate documentation for tasks in a project

## Flags

### `-I --index`

write only an index of tasks, intended for use with `--multi`

### `-i --inject`

inserts the documentation into an existing file

This will look for a special comment, &lt;!-- mise-tasks -->, and replace it with the generated documentation.
It will replace everything between the comment and the next comment, &lt;!-- /mise-tasks --> so it can be
run multiple times on the same file to update the documentation.

### `-m --multi`

render each task as a separate document, requires `--output` to be a directory

### `-o --output <OUTPUT>`

writes the generated docs to a file/directory

### `-r --root <ROOT>`

root directory to search for tasks

### `-s --style <STYLE>`

**Choices:**

- `simple`
- `detailed`

Examples:

    mise generate task-docs
