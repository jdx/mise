# Tasks

You can define tasks in `mise.toml` files or as standalone shell scripts. These are useful for
things like running linters, tests, builders, servers, and other tasks that are specific to a
project. Of
course, tasks launched with mise will include the mise environment—your tools and env vars defined
in `mise.toml`.

Here's my favorite features about mise's task runner:

- building dependencies in parallel—by default with no configuration required
- last-modified checking to avoid rebuilding when there are no changes—requires minimal config
- [mise watch](./running-tasks.html#watching-files) to automatically rebuild on changes—no configuration required, but it helps
- ability to write tasks as actual bash script files and not inside yml/json/toml strings that lack
  syntax highlighting and linting/checking support

There are 2 ways to define tasks: [inside of `mise.toml` files](./toml-tasks.html) or as [standalone shell scripts](./file-tasks.html).

## Tasks in `mise.toml` files

Tasks are defined in the `[tasks]` section of the `mise.toml` file.

::: code-group

```toml [mise.toml]
[tasks.build]
description = "Build the CLI"
run = "cargo build"
```

:::

You can then run the task with `mise run build` (or `mise build` if it doesn't conflict with an existing command).

- See the [TOML tasks](./toml-tasks.html) for more information.
- See [Running Tasks](./running-tasks.html) to learn how to run tasks.

## File Tasks

You can also define tasks as standalone shell scripts. All you have to do is to create an `executable` file in a specific directory like `mise-tasks`.

::: code-group

```sh [mise-tasks/build]
#!/usr/bin/env bash
#MISE description="Build the CLI"
cargo build
```

:::

You can then run the task with `mise run build` like for TOML tasks.
See the [file tasks reference](./file-tasks.html) for more information.

## Task Configuration

The `[task_config]` section of `mise.toml` allows you to customize how `mise` executes and organizes task.

### Changing the default directory tasks are run from

```toml
[task_config]
# change the default directory tasks are run from
dir = "{{cwd}}"
```

### Including toml tasks files or other directories of file tasks

```toml
[task_config]
# add toml files containing toml tasks, or file tasks to include when looking for tasks
includes = [
    "tasks.toml", # a task toml file
    "mytasks"     # a directory containing file tasks (in addition to the default file tasks directories)
]
```

If using included task toml files, note that they have a different format than the `mise.toml` file. They are just a list of tasks.
The file should be the same format as the `[tasks]` section of `mise.toml` but without the `[task]` prefix:

::: code-group

```toml [tasks.toml]
task1 = "echo task1"
task2 = "echo task2"
task3 = "echo task3"

[task4]
run = "echo task4"
```

:::

If you want auto-completion/validation in included toml tasks files, you can use the following JSON schema: <https://mise.jdx.dev/schema/mise-task.json>

## Vars

Vars are variables that can be shared between tasks like environment variables but they are not
passed as environment variables to the scripts. They are defined in the `vars` section of the
`mise.toml` file.

```toml
[vars]
e2e_args = '--headless'

[tasks.test]
run = './scripts/test-e2e.sh {{vars.e2e_args}}'
```

Like most configuration in mise, vars can be defined across several files. So for example, you could
put some vars in your global mise config `~/.config/mise/config.toml`, use them in a task at
`~/src/work/myproject/mise.toml`. You can also override those vars in "later" config files such
as `~/src/work/myproject/mise.local.toml` and they will be used inside tasks of any config file.

As of this writing vars are only supported in TOML tasks. I want to add support for file tasks, but
I don't want to turn all file tasks into tera templates just for this feature.

## Environment variables passed to tasks

The following environment variables are passed to the task:

- `MISE_ORIGINAL_CWD`: The original working directory from where the task was run.
- `MISE_CONFIG_ROOT` or `root`: The directory containing the `mise.toml` file where the task was defined.
- `MISE_PROJECT_ROOT`: The root of the project.
- `MISE_TASK_NAME`: The name of the task being run.
- `MISE_TASK_DIR`: The directory containing the task script.
- `MISE_TASK_FILE`: The full path to the task script.
