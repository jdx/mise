# Tasks

You can define tasks in `mise.toml` files or as standalone shell scripts. These are useful for things like
running linters, tests, builders, servers, and other tasks that are specific to a project. Of course,
tasks launched with mise will include the mise environment—your tools and env vars defined in `mise.toml`.

Here's my favorite features about mise's task runner:

- building dependencies in parallel—by default with no configuration required
- last-modified checking to avoid rebuilding when there are no changes—requires minimal config
- `mise watch` to automatically rebuild on changes—no configuration required, but it helps
- ability to write tasks as actual bash script files and not inside yml/json/toml strings that lack syntax highlighting and linting/checking support

## Task Environment Variables

- `root` - the root of the project, defaults to the directory of the `mise.toml` file

## Task Configuration

You can configure how tasks are used in mise with the `[task_config]` section of `mise.toml`:

```toml
[task_config]

# add toml files containing toml tasks, or file tasks to include when looking for tasks
includes = [
    "tasks.toml", # a task toml file
    "mytasks"     # a directory containing file tasks
]
```

If using a toml file for tasks, the file should be the same format as the `[tasks]` section of `mise.toml`
but without the `[task]` prefix:

```toml
task1 = "echo task1"
task2 = "echo task2"
task3 = "echo task3"

[task4]
run = "echo task4"
```
