---
description: "Run builds, tests, and deployments with your project's tools and environment."
---

# Tasks

A task is a named command or script that runs with your project's tools and
environment variables. Use tasks for builds, tests, linters, development
servers, and other commands that teammates and CI should run the same way.

## Run your first task

Add a task to `mise.toml` in a project directory:

```mise-toml [mise.toml]
[tasks.hello]
description = "Check that task execution works"
run = "echo hello from mise"
```

Run it with [`mise run`](/cli/run.html):

```sh
mise run hello
```

```text
[hello] $ echo hello from mise
hello from mise
```

mise prints the command it runs, then the command's output. Shell activation is
not required: `mise run` loads the project's configuration and, unless
[`task.run_auto_install`](/configuration/settings.html#task.run_auto_install) is
off, installs missing tools before the task starts. Run `mise tasks ls` to list
tasks and `mise tasks info hello` to see where a task is defined.

## Choose a task format

| Format                               | Use it when                                                                       | Where it lives                                                  |
| ------------------------------------ | --------------------------------------------------------------------------------- | --------------------------------------------------------------- |
| [TOML tasks](/tasks/toml-tasks.html) | The command fits on a line or two, or the task mostly runs other tasks.           | `[tasks.<name>]` in `mise.toml`                                 |
| [File tasks](/tasks/file-tasks.html) | The script is long enough to benefit from your editor's highlighting and linting. | An executable script in `mise-tasks/` or another task directory |

Both formats use the same runner and accept most of the same properties. Start
with TOML and move a script into a file when it grows. Either format can inherit shared
settings from a [task template](/tasks/templates.html).

## Write a task as a script

Save a script as `mise-tasks/hello`:

```sh [mise-tasks/hello]
#!/usr/bin/env bash
#MISE description="Check that task execution works"
echo "hello from a file task"
```

On Linux and macOS, make it executable with `chmod +x mise-tasks/hello`, then
run `mise run hello`. Define the name `hello` either in this file or in the TOML
task above, not both. [File tasks](/tasks/file-tasks.html#windows) explains how
Windows decides which files are tasks.

## Group tasks with dependencies

A task with only `depends` runs other tasks. The prerequisites can run in
parallel; their order in `depends` does not set a sequence:

```mise-toml [mise.toml]
[tasks.check]
depends = ["format", "test"]

[tasks.format]
run = "echo checking formatting"

[tasks.test]
run = "echo running tests"
```

`mise run check` runs `format` and `test`. Replace the `echo` commands with your
project's checks. When one step must finish before the next starts, use
[run steps](/tasks/architecture.html#run-steps-in-order) instead.

## Build a task workflow

- [Running tasks](/tasks/running-tasks.html): select tasks by name or pattern,
  pass arguments, and control parallelism and output. Tasks also receive
  [variables that describe the task](/tasks/running-tasks.html#task-environment).
- [Task arguments](/tasks/task-arguments.html): give a task a command-line
  interface with validation, `--help`, and completions.
- [Dependencies and execution order](/tasks/architecture.html): choose between
  `depends`, `wait_for`, `depends_post`, and run steps.
- [Task discovery and precedence](/tasks/task-discovery.html): where mise finds
  tasks and which definition wins.
- [Task templates](/tasks/templates.html): share tools, environment variables,
  and arguments across tasks.
- [Task caching](/tasks/caching.html): skip or restore work whose inputs have
  not changed.
- [Monorepo tasks](/tasks/monorepo.html): run tasks across several project
  roots.
- [Task configuration reference](/tasks/task-configuration.html): every task
  property and `task.*` setting.
