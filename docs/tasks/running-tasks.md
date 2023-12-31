# Running Tasks

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

```toml
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
