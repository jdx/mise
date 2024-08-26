## `mise tasks ls [OPTIONS]`

```text
[experimental] List available tasks to execute
These may be included from the config file or from the project's .mise/tasks directory
mise will merge all tasks from all parent directories into this list.

So if you have global tasks in ~/.config/mise/tasks/* and project-specific tasks in
~/myproject/.mise/tasks/*, then they'll both be available but the project-specific
tasks will override the global ones if they have the same name.

Usage: tasks ls [OPTIONS]

Options:
      --no-header
          Do not print table header

  -x, --extended
          Show all columns

      --hidden
          Show hidden tasks

      --sort <COLUMN>
          Sort by column. Default is name.
          
          [possible values: name, alias, description, source]

      --sort-order <SORT_ORDER>
          Sort order. Default is asc.
          
          [possible values: asc, desc]

  -J, --json
          Output in JSON format

Examples:
    
    $ mise tasks ls
```
