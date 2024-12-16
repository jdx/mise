# Task Configuration

This is an exhaustive list of all the configuration options available for tasks in `mise.toml` or
as file tasks.

## Task properties

All examples are in toml-task format instead of file, however they apply in both except where otherwise noted.

### `run`

- **Type**: `string | string[]`

The command to run. This is the only required property for a task. Note that tasks can be defined in
`mise.toml` in various ways in order to simplify the config, e.g.: these are all equal:

```toml
tasks.a = "echo hello"
tasks.b = ["echo hello"]
tasks.c.run = "echo hello"
[tasks.d]
run = "echo hello"
[tasks.e]
run = ["echo hello"]
```

### `run_windows`

An alterative script to run when `mise run` is executed on windows:

```toml
[tasks.build]
run = "cargo build"
run_windows = "cargo build --features windows"
```

### `description`

- **Type**: `string`

A description of the task. This is used in (among other places)
the help output, completions, `mise run` (without arguments), and `mise tasks`.

```toml
[tasks.build]
description = "Build the CLI"
run = "cargo build"
```

### `alias`

- **Type**: `string | string[]`

An alias for the task so you can run it with `mise run <alias>` instead of the full task name.

```toml
[tasks.build]
alias = "b" # run with `mise run b` or `mise b`
run = "cargo build"
```

### `depends`

- **Type**: `string | string[]`

Tasks that must be run before this task. This is a list of task names or aliases. Arguments can be
passed to the task, e.g.: `depends = ["build --release"]`. If multiple tasks have the same dependency,
that dependency will only be run once. mise will run whatever it can in parallel (up to [`--jobs`](/cli/run))
through the use of `depends` and related properties.

```toml
[tasks.build]
run = "cargo build"
[tasks.test]
depends = ["build"]
run = "cargo test"
```

### `depends_post`

- **Type**: `string | string[]`

Like `depends` but these tasks run _after_ this task and its dependencies complete. For example, you
may want a `postlint` task that you can run individually without also running `lint`:

```toml
[tasks.lint]
run = "eslint ."
depends_post = ["postlint"]
[tasks.postlint]
run = "echo 'linting complete'"
```

### `wait_for`

- **Type**: `string | string[]`

Similar to `depends`, it will wait for these tasks to complete before running however they won't be
added to the list of tasks to run. This is essentially optional dependencies.

```toml
[tasks.lint]
wait_for = ["render"] # creates some js files, so if it's running, wait for it to finish
run = "eslint ."
```

### `env`

- **Type**: `{ [key]: string | int | bool }`

Environment variables specific to this task. These will not be passed to `depends` tasks.

```toml
[tasks.test]
env.TEST_ENV_VAR = "ABC"
run = [
    "echo $TEST_ENV_VAR",
    "mise run some-other-task", # running tasks this will _will_ have TEST_ENV_VAR set of course
]
```

### `tools`

- **Type**: `{ [key]: string }`

Tools to install and activate before running the task. This is useful for tasks that require a specific tool to be
installed or a tool with a different version. It will only be used for that task, not dependencies.

```toml
[tasks.build]
tools.rust = "1.50.0"
run = "cargo build"
```

### `dir`

- **Type**: `string`
- **Default**: <code v-pre>"{{ config_root }}"</code> - the directory containing `mise.toml`, or in the case of something like `~/src/myproj/.config/mise.toml`, it will be `~/src/myproj`.

The directory to run the task from. The most common way this is used is when you want the task to execute
in the user's current directory:

```toml
[tasks.test]
dir = "{{cwd}}"
run = "cargo test"
```

### `hide`

- **Type**: `bool`
- **Default**: `false`

Hide the task from help, completion, and other output like `mise tasks`. Useful for deprecated or internal
tasks you don't want others to easily see.

```toml
[tasks.internal]
hide = true
run = "echo my internal task"
```

### `raw`

- **Type**: `bool`
- **Default**: `false`

Connects the task directly to the shell's stdin/stdout/stderr. This is useful for tasks that need to
accept input or output in a way that mise's normal task handling doesn't support. This is not recommended
to use because it really screws up the output whenever mise runs tasks in parallel. Ensure when using
this that no other tasks are running at the same time.

In the future we could have a property like `single = true` or something that prevents multiple tasks
from running at the same time. If that sounds useful, search/file a ticket.

### `sources`

- **Type**: `string | string[]`

Files or directories that this task uses as input, if this and `outputs` is defined, mise will skip
executing tasks where the modification time of the oldest output file is newer than the modification
time of the newest source file. This is useful for tasks that are expensive to run and only need to
be run when their inputs change.

The task itself will be automatically added as a source, so if you edit the definition that will also
cause the task to be run.

This is also used in `mise watch` to know which files/directories to watch.

This can be specified with relative paths to the config file and/or with glob patterns, e.g.: `src/**/*.rs`.
Ensure you don't go crazy with adding a ton of files in a glob though—mise has to scan each and every one to check
the timestamp.

```toml
[tasks.build]
run = "cargo build"
sources = ["Cargo.toml", "src/**/*.rs"]
outputs = ["target/debug/mycli"]
```

Running the above will only execute `cargo build` if `mise.toml`, `Cargo.toml`, or any ".rs" file in the `src` directory
has changed since the last build.

### `outputs`

- **Type**: `string | string[] | { auto = true }`

The counterpart to `sources`, these are the files or directories that the task will create/modify after
it executes.

`auto = true` is an altnernative to specifying output files manually. In that case, mise will touch
an internally tracked file based on the hash of the task definition (stored in `~/.local/state/mise/task-outputs/<hash>` if you're curious).
This is useful if you want `mise run` to execute when sources change but don't want to have to manually `touch`
a file for `sources` to work.

```toml
[tasks.build]
run = "cargo build"
sources = ["Cargo.toml", "src/**/*.rs"]
outputs = { auto = true }
```

### `shell`

- **Type**: `string`
- **Default**: [`unix_default_inline_shell_args`](/configuration/settings.html#unix_default_inline_shell_args) or [`windows_default_inline_shell_args`](/configuration/settings.html#windows_default_inline_shell_args)
- **Note**: Only applies to toml-tasks.

The shell to use to run the task. This is useful if you want to run a task with a different shell than
the default such as `fish`, `zsh`, or `pwsh`. Generally though, it's recommended to use a [shebang](/tasks/toml-tasks.html#shell-shebang) instead
because that will allow IDEs with mise support to show syntax highlighting and linting for the script.

```toml
[tasks.hello]
run = '''
#!/usr/bin/env node
console.log('hello world')
'''
```

### `quiet`

- **Type**: `bool`
- **Default**: `false`

Suppress mise's output for the task such as showing the command that is run, e.g.: `[build] $ cargo build`.
When this is set, mise won't show any output other than what the script itself outputs. If you'd also
like to hide even the output that the task emits, use [`silent`](#silent).

### `silent`

- **Type**: `bool | "stdout" | "stderr"`
- **Default**: `false`

Suppress all output from the task. If set to `"stdout"` or `"stderr"`, only that stream will be suppressed.

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

## `[task_config]` options

Options available in the top-level `mise.toml` `[task_config]` section. These apply to all tasks which
are included by that config file or use the same root directory, e.g.: `~/src/myprojec/mise.toml`'s `[task_config]`
applies to file tasks like `~/src/myproject/mise-tasks/mytask` but not to tasks in `~/src/myproject/subproj/mise.toml`.

### `task_config.dir`

Change the default directory tasks are run from.

```toml
[task_config]
dir = "{{cwd}}"
```

### `task_config.includes`

Add toml files containing toml tasks, or file tasks to include when looking for tasks.

```toml
[task_config]
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

## `[redactions]` options

Redactions are a way to hide sensitive information from the output of tasks. This is useful for things like
API keys, passwords, or other sensitive information that you don't want to accidentally leak in logs or
other output.

### `redactions.env`

- **Type**: `string[]`

A list of environment variables to redact from the output.

```toml
[redactions]
env = ["API_KEY", "PASSWORD"]
[tasks.test]
run = "echo $API_KEY"
```

Running the above task will output `echo [redacted]` instead.

You can also specify these as a glob pattern, e.g.: `redactions.env = ["SECRETS_*"]`.

### `redactions.vars`

- **Type**: `string[]`

A list of [vars](#vars) to redact from the output.

```toml
[vars]
secret = "mysecret"
[tasks.test]
run = "echo {{vars.secret}}"
```

:::tip
This is generally useful when using `mise.local.toml` to put secret vars in which can be shared
with any other `mise.toml` file in the hierarchy.
:::

## `[vars]` options

Vars are variables that can be shared between tasks like environment variables but they are not
passed as environment variables to the scripts. They are defined in the `vars` section of the
`mise.toml` file.

```toml
[vars]
e2e_args = '--headless'
[tasks.test]
run = './scripts/test-e2e.sh {{vars.e2e_args}}'
```

Like `[env]`, vars can also be read in as a file:

```toml
[vars]
_.file = ".env"
```

[Secrets](/environments/secrets) are also supported as vars.
