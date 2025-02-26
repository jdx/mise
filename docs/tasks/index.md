# Tasks

> Like [make](https://www.gnu.org/software/make/manual/make.html) it manages _tasks_ used
> to build and test projects.

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

```toml [mise.toml]
[tasks.build]
description = "Build the CLI"
run = "cargo build"
```

You can then run the task with `mise run build` (or `mise build` if it doesn't conflict with an existing command).

- See [TOML tasks](./toml-tasks.html) for more information.
- See [Running Tasks](./running-tasks.html) to learn how to run tasks.

## File Tasks

You can also define tasks as standalone shell scripts. All you have to do is to create an `executable` file in a specific directory like `mise-tasks`.

```sh [mise-tasks/build]
#!/usr/bin/env bash
#MISE description="Build the CLI"
cargo build
```

You can then run the task with `mise run build` like for TOML tasks.
See the [file tasks reference](./file-tasks.html) for more information.

## Environment variables passed to tasks

The following environment variables are passed to the task:

- `MISE_ORIGINAL_CWD`: The original working directory from where the task was run.
- `MISE_CONFIG_ROOT`: The directory containing the `mise.toml` file where the task was defined or if the config path is something like `~/src/myproj/.config/mise.toml`, it will be `~/src/myproj`.
- `MISE_PROJECT_ROOT`: The root of the project.
- `MISE_TASK_NAME`: The name of the task being run.
- `MISE_TASK_DIR`: The directory containing the task script.
- `MISE_TASK_FILE`: The full path to the task script.
