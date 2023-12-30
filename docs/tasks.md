---
---

# Tasks <Badge type="warning" text="experimental" />
You can define tasks in `.rtx.toml` files or as standalone shell scripts. These are useful for things like
running linters, tests, builders, servers, and other tasks that are specific to a project. Of course,
tasks launched with rtx will include the rtx environment—your tools and env vars defined in `.rtx.toml`.

Here's my favorite features about rtx's task runner:

- building dependencies in parallel—by default with no configuration required
- last-modified checking to avoid rebuilding when there are no changes—requires minimal config
- `rtx watch` to automatically rebuild on changes—no configuration required, but it helps
- ability to write tasks as actual bash script files and not inside yml/json/toml strings that lack syntax highlighting and linting/checking support

> [!WARNING]
>
> This is an experimental feature. It is not yet stable and will likely change. Some of the docs
> may not be implemented, may be implemented incorrectly, or the docs may need to be updated.
> Please give feedback early since while it's experimental it's much easier to change.

## Script Tasks

Tasks can be defined in 2 ways, either as standalone script files in `.rtx/tasks/:task_name` such as the following build script
for cargo:

```bash
#!/usr/bin/env bash
# rtx:description "Build the CLI"
cargo build
```

> [!IMPORTANT]
>
> The `rtx:description` comment is optional but recommended. It will be used in the output of `rtx tasks`.
> The other configuration for "script" tasks is supported in this format so you can specify things like the
> following-note that this is parsed a TOML table:
>
> ```bash
> # rtx alias="b"
> # rtx sources=["Cargo.toml", "src/**/*.rs"]
> # rtx outputs=["target/debug/mycli"]
> # rtx env={RUST_BACKTRACE = "1"}
> # rtx depends=["lint", "test"]
> ```

Assuming that file was located in `.rtx/tasks/build`, it can then be run with `rtx run build` (or with its alias: `rtx run b`).
This script can be edited with by running `rtx task edit build` (using $EDITOR). If it doesn't exist it will be created.
These are convenient for quickly making new scripts. Having the code in a bash file and not TOML helps make it work
better in editors since they can do syntax highlighting and linting more easily. They also still work great for non-rtx users—though
of course they'll need to find a different way to install their dev tools the tasks might use.

## TOML-based Tasks

Tasks can also be defined in `.rtx.toml` files in different ways. This is a more "traditional" method of defining tasks:

```toml
tasks.clean = 'cargo clean && rm -rf .cache' # runs as a shell command

[tasks.build]
description = 'Build the CLI'
run = "cargo build"
alias = 'b' # `rtx run b`

[tasks.test]
description = 'Run automated tests'
run = [ # multiple commands are run in series
    'cargo test',
    './scripts/test-e2e.sh',
]
dir = "{{cwd}}" # run in user's cwd, default is the project's base directory

[tasks.lint]
description = 'Lint with clippy'
env = {RUST_BACKTRACE = '1'} # env vars for the script
# you can specify a multiline script instead of individual commands
run = """
#!/usr/bin/env bash
cargo clippy
"""

[task.ci] # only dependencies to be run
description = 'Run CI tasks'
depends = ['build', 'lint', 'test']

[tasks.release]
description = 'Cut a new release'
file = 'scripts/release.sh' # execute an external script
```

## Task Environment Variables

- `RTX_PROJECT_ROOT` - the root of the project, defaults to the directory of the `.rtx.toml` file

## Running Tasks

See available tasks with `rtx tasks`. Run a task with `rtx task run`, `rtx run`, or just `rtx r`.
You might even want to make a shell alias like `alias r='rtx r'` since this is likely a common command.

By default, tasks will execute with a maximum of 4 parallel jobs. Customize this with the `--jobs` option,
`jobs` setting or `RTX_JOBS` environment variable. The output normally will be by line, prefixed with the task
label. By printing line-by-line we avoid interleaving output from parallel executions. However, if
--jobs == 1, the output will be set to `interleave`.

To just print stdout/stderr directly, use `--interleave`, the `task_output` setting, or `RTX_TASK_OUTPUT=interleave`.

Stdin is not read by default. To enable this, set `raw = true` on the task that needs it. This will prevent
it running in parallel with any other task-a RWMutex will get a write lock in this case.

There is partial support for wildcards, for example, this makes a "lint" task that runs everything that begins with "lint:".

```
[tasks."lint:eslint"] # using a ":" means we need to add quotes
run = "eslint ."
[tasks."lint:prettier"]
run = "prettier --check ."
[tasks.lint]
depends = ["lint:*"]
```

> [!NOTE]
> As of this writing these wildcards only function at the right side and only work for dependencies.
> It should be possible to also run `rtx run lint:*` but that is not yet implemented.

Extra arguments will be passed to the task, for example, if we want to run in release mode:

```bash
rtx run build --release
```

If there are multiple commands, the args are only passed to the last command.

Multiple tasks/arguments can be separated with this `:::` delimiter:

```bash
rtx run build arg1 arg2 ::: test arg3 arg4
```

rtx will run the task named "default" if no task is specified—and you've created one named "default". You can also alias a different task to "default".

```bash
rtx run
```

## Running on file changes

It's often handy to only execute a task if the files it uses changes. For example, we might only want
to run `cargo build` if an ".rs" file changes. This can be done with the following config:

```toml
[tasks.build]
description = 'Build the CLI'
run = "cargo build"
sources = ['Cargo.toml', 'src/**/*.rs'] # skip running if these files haven't changed
outputs = ['target/debug/mycli']
```

Now if `target/debug/mycli` is newer than `Cargo.toml` or any ".rs" file, the task will be skipped. This uses last modified timestamps.
It wouldn't be hard to add checksum support.

## Watching files

Run a task when the source changes with `rtx watch`:

```bash
rtx watch -t build
```

Currently this just shells out to watchexec-which you can install however you want including with rtx: `rtx use -g watchexec@latest`.
This may change in the future.

Arguments to `rtx watch` will be forwarded onto watchexec. For example, to print diagnostic info:

```bash
rtx watch -t build -- --print-events --verbose
```

See watchexec's help with `watchexec --help` or `rtx watch -- --help` to see
all of the options.
