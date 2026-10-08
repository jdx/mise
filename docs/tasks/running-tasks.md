---
description: "Run tasks with mise run, select them by name or pattern, pass arguments, and control parallelism and output."
socialDescription: "Run tasks with mise run, select them by pattern, and control parallelism and output."
---

# Running tasks

Run a task with [`mise run`](/cli/run.html). mise loads the task's tools and
environment, runs its dependencies first, and runs independent tasks in
parallel. [Dependencies and execution order](/tasks/architecture.html) explains
how mise orders the tasks in a run.

## Find and run tasks

`mise tasks` lists the tasks available in the current directory. Add `--hidden`
to include tasks with `hide = true`, or `--extended` to see where each task is
defined. Run a task by name:

```sh
mise run build
```

### `mise run` shorthand {#mise-run-shorthand}

`mise run build`, `mise r build`, `mise tasks run build`, and `mise build` all
run the `build` task. The last form works only while the task name does not
match a mise command, and a later mise release can add a command with that
name, so write `mise run build` in scripts and documentation. For interactive
use, a shell alias such as `alias mr='mise run'` saves typing.

### Run the default task

`mise run` with no task name runs the task named or aliased `default`:

```mise-toml [mise.toml]
[tasks.dev]
alias = "default"
run = "npm run dev"
```

With this config, `mise run` runs `dev`. When no `default` task exists, an
interactive terminal opens a task selector, and a non-interactive run fails with
`no task default found`.

### Run several tasks

Separate tasks, each with its own arguments, with `:::`:

```sh
mise run build arg1 arg2 ::: test arg3 arg4
```

The tasks run in parallel unless one depends on another.

## Select tasks by name

Name related tasks with `:`-separated groups, such as `test:unit` and
`test:e2e`, so one pattern can select them. A TOML key that contains `:` needs
quotes:

```mise-toml [mise.toml]
[tasks."test:unit"]
run = "cargo test --lib"
```

File tasks get their groups from subdirectories; see
[File tasks](/tasks/file-tasks.html#task-grouping).

### Wildcards

`mise run` and task dependencies such as `depends` accept glob patterns:

| Pattern          | Matches                                       |
| ---------------- | --------------------------------------------- |
| `?`              | One character                                 |
| `*`              | Any characters within one `:`-separated group |
| `**`             | Any number of whole groups                    |
| `{a,b}`          | Either pattern                                |
| `[abc]`, `[a-z]` | One character from the set or range           |
| `[!abc]`         | One character that is not in the set or range |

Quote a pattern so your shell does not expand it:

```sh
mise run 'test:*:local'                   # test:units:local, not test:e2e:happy:local
mise run 'test:**:local'                  # test:units:local and test:e2e:happy:local
mise run 'generate:{completions,docs:*}'  # generate:completions and every generate:docs:* task
```

Patterns also work in `depends`:

```mise-toml [mise.toml]
[tasks."lint:eslint"]
run = "eslint ."

[tasks."lint:prettier"]
run = "prettier --check ."

[tasks.lint]
depends = ["lint:*"]
```

In a monorepo, patterns can also select projects, such as `//...:test`; see
[Monorepo tasks](/tasks/monorepo.html).

## Pass arguments

Arguments after the task name go to the task:

```sh
mise run build --release
```

To give a task a validated interface, define its arguments with a
[usage spec](/tasks/task-arguments.html). The spec also gives the task
`--help`, [shell completions](/shell-setup.html), and
[generated documentation](/cli/generate/task-docs.html). Without a spec, mise
forwards extra arguments according to the task's form:

- If `run` is an array, the arguments go only to its last entry.
- For an inline command, mise appends the arguments to the command, quoted for
  the shell.
- A [shebang script](/tasks/toml-tasks.html#shell-shebang) or a file task
  receives them as script arguments, such as `$1` and `$@` in Bash.

Put mise's own flags before the task name: `mise run --silent build`.
Everything after the task name belongs to the task. A task without a usage spec
receives `--silent` as an argument, and a task with one fails with
`unexpected word: --silent` unless its spec defines that flag. A task can define
flags that share a name with a mise flag, such as `--env`.

## Control a run

These [`mise run`](/cli/run.html) flags change how a run proceeds:

| Flag                          | Effect                                                                                                                                                                                                              |
| ----------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `-j`, `--jobs <N>`            | Run up to N tasks at once. Defaults to the [`jobs`](/configuration/settings.html#jobs) setting (`MISE_JOBS`).                                                                                                       |
| `-f`, `--force`               | Run tasks even when their [outputs are up to date](#skip-tasks-that-are-up-to-date).                                                                                                                                |
| `-n`, `--dry-run`             | Print each task's command in execution order without running anything.                                                                                                                                              |
| `-c`, `--continue-on-error`   | Keep running after a task fails. See [When a task fails](#when-a-task-fails).                                                                                                                                       |
| `--skip-deps`                 | Run only the named tasks, without their dependencies.                                                                                                                                                               |
| `--timeout <DURATION>`        | Stop the whole run after a duration such as `30s` or `5m`. Overrides the [`task.timeout`](/configuration/settings.html#task.timeout) setting.                                                                       |
| `-C`, `--cd <DIR>`            | Run as if mise started in another directory.                                                                                                                                                                        |
| `-t`, `--tool <TOOL@VERSION>` | Add a tool to the run, such as `--tool node@24`.                                                                                                                                                                    |
| `--skip-tools`                | Do not install missing tools before the tasks start.                                                                                                                                                                |
| `-r`, `--raw`                 | Connect stdin, stdout, and stderr directly to each task and run one task at a time. Redaction does not apply, except to tasks that receive [secrets](/tasks/task-configuration.html#secrets), which ignore `--raw`. |

A task's own [`timeout`](/tasks/task-configuration.html#timeout) still limits
its commands when you pass `--timeout`.

### When a task fails

By default, a failing task stops the run. mise stops the tasks that are still
running (with SIGTERM on Unix) and starts no new tasks, except the
[`depends_post`](/tasks/architecture.html#depends-post) tasks of tasks that
already started. The run exits with the failed task's status.

With `--continue-on-error`, running tasks finish and the remaining tasks still
start, including tasks that depend on the failed one. At the end, mise lists the
failed tasks and exits with a non-zero status.

## Output

When tasks run in parallel, mise labels each output line with the task name
(`prefix`). When only one task can run at a time (a single task, a chain of
dependencies, or `--jobs 1`), it prints output directly (`interleave`). Choose a
style with `--output`, `MISE_TASK_OUTPUT`, the
[`task.output`](/configuration/settings.html#task.output) setting, or a task's
[`output`](/tasks/task-configuration.html#output) property:

| Style        | Output                                                                                                                                    |
| ------------ | ----------------------------------------------------------------------------------------------------------------------------------------- |
| `prefix`     | Each line, labelled with the task name.                                                                                                   |
| `interleave` | Output as it arrives, without labels.                                                                                                     |
| `keep-order` | Labelled lines; one task streams live while the others are buffered and printed in order.                                                 |
| `replacing`  | Each new stdout line replaces the previous one; stderr is printed as is.                                                                  |
| `timed`      | Labelled stdout lines that stay the task's latest line for at least a second; lines replaced sooner are dropped. Stderr is printed as is. |
| `silent`     | Nothing from tasks or mise except errors.                                                                                                 |

Verbosity is separate from style. `--quiet` hides mise's own messages, such as
`[build] $ cargo build`, and `--silent` also hides the tasks' output. For
example, `mise run --output prefix --quiet test` keeps the labels and hides
mise's messages. To make every task quiet without changing other mise commands,
set [`task.quiet`](/configuration/settings.html#task.quiet) under `[settings]`.
A task can also set its own `quiet` or `silent` property. The `quiet` output
style is deprecated; use `--output interleave --quiet` instead.

## Interactive input

Unless a task is `interactive` or `raw`, stdin is connected only with
`interleave` output: the default for a single task, a sequential chain, or
`--jobs 1`, or when you pass `--output interleave`. With other output styles,
including `prefix`, the default when tasks run in parallel, stdin is not
connected. It is also not connected when output must be redacted: when the task
receives [secrets](/tasks/task-configuration.html#secrets), or when the config
marks a value for [redaction](/environments/secrets/#redaction) with
`redact = true` or `redactions`.

Set `interactive = true` on a task that needs the terminal, such as one that
prompts. It has exclusive terminal access while it runs. `raw = true` takes
exclusive access for each command instead. Both bypass output redaction and the
artifact cache. See [`interactive`](/tasks/task-configuration.html#interactive)
and [`raw`](/tasks/task-configuration.html#raw).

## Skip tasks that are up to date

Give a task `sources` and `outputs`, and mise skips it while its outputs are up
to date:

```mise-toml [mise.toml]
[tasks.build]
description = "Build the CLI"
run = "cargo build"
sources = ["Cargo.toml", "src/**/*.rs"]
outputs = ["target/debug/mycli"]
```

While the outputs are newer than the sources and nothing has changed since the
last successful run, mise prints `[build] sources up-to-date, skipping` instead
of running the task.
[What makes a task stale](/tasks/caching.html#what-makes-a-task-stale) lists the
exact checks. Use `mise run --force build` to run it anyway.
[Task caching](/tasks/caching.html) also covers the artifact cache, which can
restore deleted outputs.

## Rerun tasks when files change

[`mise watch`](/cli/watch.html) runs a task and runs it again when its sources
change:

```sh
mise watch build
```

mise watches the `sources` of the task and of its dependencies. A task without
`sources` watches the current directory. For a process that keeps running, such
as a development server, restart it on each change:

```sh
mise watch --restart dev
```

Separate several tasks with `:::`, as with `mise run`. Without a task name,
`mise watch` runs the `default` task. `mise watch` uses
[watchexec](https://github.com/watchexec/watchexec); install it with
`mise use watchexec` or put it on `PATH` yourself.

`mise watch` passes its watchexec flags, such as `--debounce`, `--clear` and
`--shell`, on to watchexec. watchexec's `--env` and `--quiet` are spelled
`--watchexec-env` and `--watchexec-quiet` there, because `-E` and `-q` stay
mise's own `--env` and `--quiet`:

```sh
mise watch -E dev --watchexec-env LOG_LEVEL=debug build
```

## Task environment {#task-environment}

Tasks run with the project's [environment variables](/environments/) and tools,
plus these variables:

| Variable                    | Value                                                                                                                                                                                  |
| --------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `MISE_ORIGINAL_CWD`         | The directory where you ran mise.                                                                                                                                                      |
| `MISE_CONFIG_ROOT`          | The [config root](/configuration.html#config-root) of the file that defines the task, such as `~/proj` for both `~/proj/mise.toml` and `~/proj/.config/mise.toml`.                     |
| `MISE_PROJECT_ROOT`         | The root of the project that defines the task, whichever directory you run it from. In a monorepo, the subproject's directory. For a global or remote task, the project you run it in. |
| `MISE_MONOREPO_ROOT`        | The directory whose config sets `monorepo_root = true`. Set only in a [monorepo](/tasks/monorepo.html).                                                                                |
| `MISE_TASK_NAME`            | The task's name as `mise tasks ls` shows it, so a file task's name has no script extension.                                                                                            |
| `MISE_TASK_FILE`            | The task's script: the file task itself, or the file named by a TOML task's `file`. For a TOML task with an inline `run`, the config file that defines it.                             |
| `MISE_TASK_DIR`             | The directory that contains `MISE_TASK_FILE`.                                                                                                                                          |
| `MISE_TASK_COLOR`           | The ANSI sequence for the task's label color. Empty when colors are off, the task is quiet, or the output style shows no label (`interleave`, `silent`).                               |
| `MISE_ENV`                  | The active [config environments](/configuration/environments.html), comma-separated, when any are set.                                                                                 |
| `TRACEPARENT`, `TRACESTATE` | The task's trace context, when [OpenTelemetry](/tasks/opentelemetry.html#trace-propagation) export is on.                                                                              |
| `usage_*`                   | The values of the task's [arguments and flags](/tasks/task-arguments.html).                                                                                                            |

For a TOML task in `~/proj/.config/mise.toml`, `MISE_TASK_DIR` is
`~/proj/.config`, while `MISE_CONFIG_ROOT` is `~/proj`. When you print with
`MISE_TASK_COLOR`, reset the color after your text:

```sh
printf '%smessage\033[0m\n' "$MISE_TASK_COLOR"
```
