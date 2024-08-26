## `mise tasks deps [OPTIONS] [TASKS]...` <Badge type="warning" text="experimental" />

```text
[experimental] Display a tree visualization of a dependency graph

Usage: tasks deps [OPTIONS] [TASKS]...

Arguments:
  [TASKS]...
          Tasks to show dependencies for
          Can specify multiple tasks by separating with spaces
          e.g.: mise tasks deps lint test check

Options:
      --hidden
          Show hidden tasks

      --dot
          Display dependencies in DOT format

Examples:

    # Show dependencies for all tasks
    $ mise tasks deps

    # Show dependencies for the "lint", "test" and "check" tasks
    $ mise tasks deps lint test check

    # Show dependencies in DOT format
    $ mise tasks deps --dot
```
