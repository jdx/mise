# Tasks

A task is a named command or script that runs with your project's tools and
environment variables. Use tasks for builds, tests, linters, development servers,
and other commands you want teammates and CI to run consistently.

## Run your first task

Create this file in a project directory:

```toml [mise.toml]
[tasks.hello]
description = "Check that task execution works"
run = "echo hello from mise"
```

```sh
mise run hello
# hello from mise
```

Shell activation is not required. mise loads the configuration for the task and,
by default, installs any missing configured tools before starting it.
Use `mise tasks ls` to list tasks and `mise tasks info hello` to inspect one.

## Choose a task format

| Format                             | Use it when                                                          | Configuration                                       |
| ---------------------------------- | -------------------------------------------------------------------- | --------------------------------------------------- |
| [TOML tasks](./toml-tasks.html)    | Commands are short or mostly configure dependencies and options.     | `[tasks.<name>]` in `mise.toml`                     |
| [File tasks](./file-tasks.html)    | A script benefits from its language's editor support and lint tools. | A script in `mise-tasks/` or another task directory |
| [Task templates](./templates.html) | Several tasks share configuration.                                   | `[task_templates.<name>]`, selected with `extends`  |

The formats use the same task runner. Start with TOML and move longer scripts into
files as they grow.

## Tasks in `mise.toml` files

A dependency-only task can group other tasks. Prerequisites may run in parallel;
their order in `depends` does not establish a sequence:

```toml [mise.toml]
[tasks.check]
depends = ["format", "test"]

[tasks.format]
run = "echo checking formatting"

[tasks.test]
run = "echo running tests"
```

`mise run check` runs both prerequisites. Replace the `echo` commands with your
project's checks. Use a [run array](./running-tasks.html#execution-order) when one
step must finish before the next starts.

## File Tasks

Save a script as `mise-tasks/hello`:

```sh [mise-tasks/hello]
#!/usr/bin/env bash
#MISE description="Check that task execution works"
echo "hello from a file task"
```

On macOS and Linux, make it executable with `chmod +x mise-tasks/hello`, then run
`mise run hello`. This is an alternative to the TOML task above. The
[file task guide](./file-tasks.html#windows) explains Windows detection and interpreters.

## Build a task workflow

- [Running tasks](./running-tasks.html): arguments, wildcards, parallelism, and execution order.
- [Task arguments](./task-arguments.html): define a CLI with validation, help, and completions.
- [Task configuration](./task-configuration.html): find a specific property and its scope.
- [Task caching](./caching.html): choose freshness checks or cached outputs and declare their inputs.
- [Monorepo tasks](./monorepo.html): run tasks across configured project roots.
- [Task architecture](./architecture.html): understand discovery, scheduling, and failures.

## Environment variables passed to tasks

The following environment variables are passed to the task:

- `MISE_ORIGINAL_CWD`: The original working directory from where the task was run.
- `MISE_CONFIG_ROOT`: The directory containing the `mise.toml` file where the task was defined. If the config path is something like `~/src/myproj/.config/mise.toml`, this is `~/src/myproj`.
- `MISE_PROJECT_ROOT`: The root of the project that defines the task. For monorepo subproject tasks this is the subproject's directory and is stable regardless of the directory the task is invoked from.
- `MISE_MONOREPO_ROOT`: The root of the monorepo (the directory containing the config with `monorepo_root = true`). Only set inside a monorepo.
- `MISE_TASK_NAME`: The name of the task being run.
- `MISE_TASK_COLOR`: The ANSI sequence that starts the task's prefix color and emphasis. This is
  set to an empty string when colors are disabled or the selected output mode does not display a
  task prefix. Add an ANSI reset after the text, for example
  `printf '%smessage\033[0m\n' "$MISE_TASK_COLOR"`. The replacing output style also provides the value when
  it uses its text fallback. The variable describes the task label style and does not mean that every line
  is automatically prefixed.
- `MISE_TASK_DIR`: The directory containing the task script.
- `MISE_TASK_FILE`: The full path to the task script.
