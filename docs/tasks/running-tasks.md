# Running Tasks

See available tasks with `mise tasks`. To show tasks hidden with property `hide=true`, use the option `--hidden`.

List dependencies of tasks with `mise task deps [tasks]...`.

Run a task with `mise task run`, `mise run`, or just `mise r`.
You might even want to make a shell alias like `alias mr='mise run --'` since this is likely a common command.

By default, tasks will execute with a maximum of 4 parallel jobs. Customize this with the `--jobs` option,
`jobs` setting or `MISE_JOBS` environment variable. The output normally will be by line, prefixed with the task
label. By printing line-by-line we avoid interleaving output from parallel executions. However, if
--jobs == 1, the output will be set to `interleave`.

To just print stdout/stderr directly, use `--interleave`, the `task_output` setting, or `MISE_TASK_OUTPUT=interleave`.

Stdin is not read by default. To enable this, set `raw = true` on the task that needs it. This will prevent
it running in parallel with any other task-a RWMutex will get a write lock in this case.

Extra arguments will be passed to the task, for example, if we want to run in release mode:

```bash
mise run build --release
```

If there are multiple commands, the args are only passed to the last command.

:::tip
You can define arguments/flags for tasks which will provide validation, parsing, autocomplete, and documentation.

* [Arguments in File Tasks](/tasks/file-tasks.html#arguments)
* [Arguments in TOML Tasks](/tasks/toml-tasks.html#arguments)
:::

Multiple tasks/arguments can be separated with this `:::` delimiter:

```bash
mise run build arg1 arg2 ::: test arg3 arg4
```

mise will run the task named "default" if no task is specified—and you've created one named "default". You can also alias a different task to "default".

```bash
mise run
```

## Task Grouping

Tasks can be grouped semantically by using name prefixes separated with `:`s.
For example all testing related tasks may begin with `test:`. Nested grouping
can also be used to further refine groups and simplify pattern matching.
For example running `mise run test:**:local` will match`test:units:local`,
`test:integration:local` and `test:e2e:happy:local`
(See [Wildcards](#wildcards) for more information).

## Wildcards

Glob style wildcards are supported when running tasks or specifying tasks
dependencies.

Available Wildcard Patterns:

- `?` matches any single character
- `*` matches 0 or more characters
- `**` matches 0 or more groups
- `{glob1,glob2,...}` matches any of the comma-separated glob patterns
- `[ab,...]` matches any of the characters or ranges `[a-z]`
- `[!ab,...]` matches any character not in the character set

### Examples

`mise run generate:{completions,docs:*}`

And with dependencies:

```toml
[tasks."lint:eslint"] # using a ":" means we need to add quotes
run = "eslint ."
[tasks."lint:prettier"]
run = "prettier --check ."
[tasks.lint]
depends = ["lint:*"]
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

Run a task when the source changes with `mise watch`:

```bash
mise watch -t build
```

Currently this just shells out to watchexec-which you can install however you want including with mise: `mise use -g watchexec@latest`.
This may change in the future.

Arguments to `mise watch` will be forwarded onto watchexec. For example, to print diagnostic info:

```bash
mise watch -t build -- --print-events --verbose
```

See watchexec's help with `watchexec --help` or `mise watch -- --help` to see
all of the options.
