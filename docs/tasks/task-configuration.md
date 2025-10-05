# Task Configuration

This is an exhaustive list of all the configuration options available for tasks in `mise.toml` or
as file tasks.

## Task properties

All examples are in toml-task format instead of file, however they apply in both except where otherwise noted.

### `run`

- **Type**: `string | (string | { task: string } | { tasks: string[] })[]`

The command(s) to run. This is the only required property for a task.

You can now mix scripts with task references:

```toml
[tasks.grouped]
run = [
  { task = "t1" },          # run t1 (with its dependencies)
  { tasks = ["t2", "t3"] }, # run t2 and t3 in parallel (with their dependencies)
  "echo end",               # then run a script
]
```

Simple forms still work and are equivalent:

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

- **Type**: `string | (string | { task: string } | { tasks: string[] })[]`

Windows-specific variant of `run` supporting the same structured syntax:

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
    "mise run some-other-task", # running tasks like this _will_ have TEST_ENV_VAR set of course
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

### `confirm`

- **Type**: `string`

A message to show before running the task. This is useful for tasks that are destructive or take a long
time to run. The user will be prompted to confirm before the task is run.

```toml
[tasks.release]
confirm = "Are you sure you want to cut a release?"
description = 'Cut a new release'
file = 'scripts/release.sh'
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
- **Default**: `{ auto = true }`

The counterpart to `sources`, these are the files or directories that the task will create/modify after
it executes.

`auto = true` is an alternative to specifying output files manually. In that case, mise will touch
an internally tracked file based on the hash of the task definition (stored in `~/.local/state/mise/task-outputs/<hash>` if you're curious).
This is useful if you want `mise run` to execute when sources change but don't want to have to manually `touch`
a file for `sources` to work.

```toml
[tasks.build]
run = "cargo build"
sources = ["Cargo.toml", "src/**/*.rs"]
outputs = { auto = true } # this is the default when sources is defined
```

### `shell`

- **Type**: `string`
- **Default**: [`unix_default_inline_shell_args`](/configuration/settings.html#unix_default_inline_shell_args) or [`windows_default_inline_shell_args`](/configuration/settings.html#windows_default_inline_shell_args)
- **Note**: Only applies to toml-tasks.

The shell to use to run the task. This is useful if you want to run a task with a different shell than
the default such as `fish`, `zsh`, or `pwsh`. Generally though, it's recommended to use a [shebang](./toml-tasks#shell-shebang) instead
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

### `usage`

- **Type**: `string`

More advanced usage specs can be added to the task's `usage` field. This only applies to toml-tasks.

```toml
[tasks.test]
usage = '''
arg "file" description="The file to test" default="src/main.rs"
'''
run = 'cargo test {{arg(name="file")}}'
```

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

## Monorepo Support <Badge type="warning" text="experimental" />

### `experimental_monorepo_root`

- **Type**: `bool`
- **Default**: `false`
- **Requires**: `MISE_EXPERIMENTAL=1`
- **Location**: Top-level in `mise.toml`

Enable monorepo task support with bazel/buck2-style target paths. When enabled, mise will automatically
discover tasks in subdirectories and prefix them with their relative path from the monorepo root.

```toml
experimental_monorepo_root = true
```

With this enabled and the following structure:

```
myproject/
├── mise.toml (with experimental_monorepo_root = true)
├── projects/
│   ├── frontend/
│   │   └── mise.toml (with tasks: build, test)
│   └── backend/
│       └── mise.toml (with tasks: build, test)
```

Tasks will be automatically namespaced:

- `//projects/frontend:build`
- `//projects/frontend:test`
- `//projects/backend:build`
- `//projects/backend:test`

You can run these tasks using either absolute or relative syntax:

```bash
# Absolute path (with // prefix)
mise run '//projects/frontend:build'

# Relative path (without // prefix)
mise run 'projects/frontend:build'

# With wildcard patterns
mise run '//...:test'              # Run 'test' task in all projects (ellipsis for paths)
mise run '//projects/...:build'    # Run 'build' in all subdirs under projects/
mise run '//projects/frontend:*'   # Run all tasks in projects/frontend (asterisk for task names)
mise run '//projects/frontend:test:*' # Run all tasks starting with 'test:' in projects/frontend
```

This is useful for large monorepos where multiple projects need their own tasks but you want to run
them all from the root directory.

**Notes:**

- Task names use `:` as the separator between path and task name
- Quote task names in shell commands to prevent interpretation of `/` and `:`
- Subdirectories with `.mise.toml`, `mise.toml`, `.mise/config.toml`, or `.config/mise/config.toml` will be scanned
- Hidden directories and common build artifacts (`node_modules`, `target`, `dist`, `build`) are automatically excluded
- **Wildcard syntax:**
  - Use `...` (ellipsis) for path matching: `//...:task` or `//path/.../subpath:task`
  - Use `*` (asterisk) for task name matching: `//path:task*` or `//path:*`
  - Path wildcards (`...`) match any directory depth
  - Task wildcards (`*`) match any characters in task names
- When a monorepo root config is trusted, all descendant configs are automatically trusted

**Performance tuning:**

For large monorepos, you can control task discovery depth with the `task.monorepo_depth` setting (default: 5):

```toml
[settings]
task.monorepo_depth = 3  # Only search 3 levels deep
```

This limits how deep mise will search for task files. For example:

- `1` = immediate children only (`monorepo_root/projects/`)
- `2` = grandchildren (`monorepo_root/projects/frontend/`)
- `5` = default (5 levels deep)

Reduce this value if you notice slow task discovery in very large monorepos, especially if your projects are concentrated at a specific depth level.

## `redactions` <Badge type="warning" text="experimental" />

- **Type**: `string[]`

Redactions are a way to hide sensitive information from the output of tasks. This is useful for things like
API keys, passwords, or other sensitive information that you don't want to accidentally leak in logs or
other output.

A list of environment variables to redact from the output.

```toml
redactions = ["API_KEY", "PASSWORD"]
```

Running the above task will output `echo [redacted]` instead.

You can also specify these as a glob pattern, e.g.: `redactions.env = ["SECRETS_*"]`.

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

[Secrets](/environments/secrets/) are also supported as vars.

## Task Configuration Settings

<script setup>
import Settings from '/components/settings.vue';
</script>

The following settings control task behavior. These can be set globally in `~/.config/mise/config.toml` or per-project in `mise.toml`:

<Settings :level="3" prefix="task" />
