## `mise generate task-docs [OPTIONS]` <Badge type="warning" text="experimental" />

```text
[experimental] Generate documentation for tasks in a project

Usage: generate task-docs [OPTIONS]

Options:
  -m, --multi
          render each task as a separate document, requires `--output` to be a directory

  -i, --inject
          inserts the documentation into an existing file
          
          This will look for a special comment, <!-- mise-tasks -->, and replace it with the generated documentation.
          It will replace everything between the comment and the next comment, <!-- /mise-tasks --> so it can be
          run multiple times on the same file to update the documentation.

  -I, --index
          write only an index of tasks, intended for use with `--multi`

  -o, --output <OUTPUT>
          writes the generated docs to a file/directory

Examples:

    $ mise generate task-docs
```
