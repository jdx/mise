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

## Select tasks by tag

Give tasks [`tags`](/tasks/task-configuration.html#tags) that say what kind of
task each one is:

```mise-toml [mise.toml]
[tasks.clippy]
tags = "lint"
run = "cargo clippy"

[tasks.shellcheck]
tags = "lint"
run = "shellcheck scripts/*.sh"

[tasks."test:e2e"]
tags = ["test", "slow"]
run = "./e2e/run"
```

`--tag` runs the tasks that have the tag, whatever their names:

```sh
mise run --tag lint                # clippy and shellcheck
mise run --tag lint --tag test     # every task tagged lint or test
mise run --tag slow 'test:*'      # test:e2e, the only test:* task tagged slow
mise tasks ls --tag lint           # list them instead
```

With no task name, `--tag` picks from every task, as if the name were `**`.
With a name or a pattern, it keeps the matching tasks that have the tag, and
fails when none does. A task's dependencies run whatever their tags. Put
`--tag` before the task name; after it, the flag goes to the task. In a
monorepo, add a pattern to reach other projects, such as
`mise run --tag lint '//...:**'`.

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
| `--report-file <PATH>`        | Write a JSON report of every task's result when the run ends. See [Run report](#run-report).                                                                                                                        |
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

### Run report

`mise run --report-file <PATH>` writes a JSON report when the run ends, whether
it passes, fails, is interrupted with Ctrl-C, or stops at `--timeout`. CI
dashboards and agent tools can read what ran and how it ended instead of
parsing the output. Setting `MISE_TASK_REPORT_FILE` does the same without
changing the command line. The tasks the run starts do not inherit
`MISE_TASK_REPORT_FILE`, so a `mise run` inside a task does not overwrite the
report. A Ctrl-C before any task starts, while tools install for example,
still writes a `cancelled` report. Neither the flag nor the variable can be
combined with `--dry-run`.

```json
{
  "version": 1,
  "status": "failed",
  "started_at": "2026-11-04T12:00:00.000Z",
  "ended_at": "2026-11-04T12:00:01.500Z",
  "duration_ms": 1500,
  "tasks": [
    {
      "name": "lint",
      "args": [],
      "status": "skipped",
      "exit_code": null,
      "duration_ms": 2,
      "started_at": "2026-11-04T12:00:00.100Z",
      "ended_at": "2026-11-04T12:00:00.102Z"
    },
    {
      "name": "test",
      "args": ["--quick"],
      "status": "failed",
      "exit_code": 3,
      "duration_ms": 1300,
      "started_at": "2026-11-04T12:00:00.110Z",
      "ended_at": "2026-11-04T12:00:01.410Z"
    },
    {
      "name": "deploy",
      "args": [],
      "status": "not_run",
      "exit_code": null,
      "duration_ms": null,
      "started_at": null,
      "ended_at": null
    }
  ]
}
```

`tasks` lists every task the run scheduled, dependencies included, in the order
they started, followed by the tasks that never started. A task's `status` is one
of:

| Status      | Meaning                                                                                                       |
| ----------- | ------------------------------------------------------------------------------------------------------------- |
| `passed`    | It ran and exited 0.                                                                                          |
| `failed`    | It ran, or failed to start, and did not succeed. `exit_code` is its exit status, or `null` when it had none.  |
| `cancelled` | mise stopped it: Ctrl-C, `--timeout`, or another task failing.                                                |
| `skipped`   | It did not need to run: its [outputs were up to date](#skip-tasks-that-are-up-to-date), or `skip_if` was met. |
| `cached`    | Its result was restored from the [artifact cache](/tasks/caching.html).                                       |
| `not_run`   | The run ended before it started.                                                                              |

The top-level `status` is `passed`, `failed`, `cancelled`, or `timed_out`.
Arguments are redacted like the terminal output, including the values of
`redact = true` environment variables and credentials in URLs. A `version`
change means a
field changed meaning; new fields can appear without one.

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
| `summary`    | Nothing while a task runs; then `[test] passed in 2.1s` if it passed, or everything it printed, labelled, if it failed.                   |
| `timed`      | Labelled stdout lines that stay the task's latest line for at least a second; lines replaced sooner are dropped. Stderr is printed as is. |
| `silent`     | Nothing from tasks or mise except errors.                                                                                                 |

Verbosity is separate from style. `--quiet` hides mise's own messages, such as
`[build] $ cargo build`, and `--silent` also hides the tasks' output. For
example, `mise run --output prefix --quiet test` keeps the labels and hides
mise's messages. To make every task quiet without changing other mise commands,
set [`task.quiet`](/configuration/settings.html#task.quiet) under `[settings]`.
A task can also set its own `quiet` or `silent` property. The `quiet` output
style is deprecated; use `--output interleave --quiet` instead.

### Only what failed {#summary-output}

`summary` holds each task's output until it ends. A test suite that passes
prints one line per task instead of every test's output, and a failing task
prints all of it. It suits CI logs and coding agents, which read every line a
command prints; for an agent, set `MISE_TASK_OUTPUT=summary` in its
environment.

```console
$ mise run --output summary test
[test:lint] $ cargo clippy -- -D warnings
[test:lint] error: unused variable: `x`
[test:lint]  --> src/main.rs:12:9
[test:unit] passed in 4.2s
[test:lint] ERROR task failed
```

A task that mise stopped because another one failed prints
`[name] cancelled`, and a dry run prints the commands it would run. mise holds
the last 64 KiB of each task's output, and prints what it holds for tasks still
running when the run is interrupted or reaches its `--timeout`. `raw` and
`interactive` tasks still get the terminal.

A task that keeps running, such as a development server or a watcher, prints
nothing in `summary` until it exits, or until the run is interrupted. Give it
`output = "interleave"` to see what it prints as it runs.

### Read the output of the last run

With the [`task.logs`](/configuration/settings.html#task.logs) setting on,
mise keeps the combined stdout and stderr of each task's last run, so output
that `replacing`, `timed`, `silent` or `summary` hid can be read without
running the task again. Task logs are off by default; turn them on in
`mise.toml`:

```toml
[settings]
task.logs = true
```

or for one shell with `export MISE_TASK_LOGS=1`, then:

```sh
mise run --output replacing test
mise tasks log test          # print the output of the last run
mise tasks log test --path   # print where the log is kept
```

Each run replaces the task's previous log, and a log keeps only its last 4 MiB.
Logs live under mise's state directory (`~/.local/state/mise/task-logs`), one
per task per project, and only you can read them. A global task gets one log
per project it runs in, and one shared by its runs outside any project. Values
that mise redacts in the terminal are redacted in the log too. A run whose
command does not start (cancelled first, or a program that is missing or not
executable) keeps the previous log.

When a task fails under `replacing` or `timed`, or under `summary` when it
printed more than the 64 KiB `summary` holds, mise prints the log's path after
the error.

mise logs only output it reads. A `raw` or `interactive` task gets the terminal
itself, and so does an `interleave` task when nothing needs redacting and
neither the artifact cache nor OpenTelemetry log export reads its output, so
their runs leave no log (and remove the one an earlier run left).

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

## Run the tasks a change affects {#affected-tasks}

`mise run --affected` runs only the matching tasks whose `sources` include a
file changed in Git:

```mise-toml [mise.toml]
[tasks."test:unit"]
sources = ["src/**"]
run = "cargo test"

[tasks."test:docs"]
sources = ["docs/**"]
run = "vale docs"
```

```sh
# After a commit that touches src/lib.rs, runs test:unit but not test:docs
mise run --affected 'test:*'

# Show why each task was or was not selected, without running anything
mise run --affected --affected-explain --dry-run 'test:*'
```

A task is also affected when a changed file is the config file that defines it
or the script its `file` names. Symlinks in the task directory and in the
leading directories of its `sources` are followed, as Git reports each file by
its own path. A literal source that is a symlink matches its target too, but a
glob that matches a link matches only the link's name; list the target, or the
link literally, to catch edits to the target. A changed submodule counts as the
files that changed in it, under its path, so `sources` match inside submodules
like any other directory. mise can only look inside an initialized submodule:
for one that is not checked out, Git reports just its path, so add that path
itself (such as `sub`) to `sources` to catch the change. mise compares the
commit the parent records for the submodule at the merge base with the one at
the head, and adds the submodule's working tree only when it is checked out at
that head; untracked files in such a submodule count even when its recorded
commit did not change. Run from inside a submodule, `--affected` compares that
submodule's own history. A separate checkout nested in the project, such as an
ignored clone, counts its own changes.
A task without `sources` might read any file, so mise runs it whenever anything
changed. When nothing changed, `--affected` runs nothing. Dependencies of a
selected task still run, and the usual
[up-to-date check](#skip-tasks-that-are-up-to-date) still applies to every task;
add `--force` to run them regardless.

mise counts the same changes as in a monorepo: the commits between
`--affected-base` (default `HEAD~1`) and `--affected-head` (default `HEAD`),
plus uncommitted and untracked files when the head is checked out. The
`--affected-committed`, `--affected-uncommitted`, and `--affected-untracked`
flags narrow that, and `--affected-json` prints the selected tasks with their
reasons (`changed-source`, `task-config`, `task-file`, `no-sources`, or `argument-sources`
for sources that use <span v-pre>`{{ usage.* }}`</span>). See
[Run affected tasks](/tasks/workspace-graph.html#affected-tasks) for how mise
picks the revisions in CI.

Under a monorepo root (`monorepo_root = true`), `--affected` selects
[projects](/tasks/workspace-graph.html#affected-tasks) instead, and runs the
matching tasks in every affected project.

## See how long tasks take

mise records each task run in this project: its arguments, start time,
duration, and whether it succeeded, failed, was restored from the artifact
cache, was skipped, or was cancelled.
[`mise tasks history`](/cli/tasks/history.html) lists the runs, oldest first,
and `--summary` shows one line per task:

```sh
$ mise tasks history --summary
Task   Runs  Failed  Median  Last    Last status
build  12    1       41.20s  38.91s  ok
lint   8     0       6.05s   5.87s   ok
test   10    2       62.04s  58.30s  failed
```

The median counts only the runs that executed and succeeded.
The median is a task's typical cost, so a person or an agent can tell which
checks are slow before choosing what to run. `--json` prints the same data for
scripts:

```sh
mise tasks history --summary --json | jq -r '.[] | "\(.task) \(.median_ms)"'
```

A duration is the wall time from when the task was ready to start until it
ended, including resolving its environment and waiting for a
[`lock`](/tasks/task-configuration.html#lock). It does not include its
dependencies, which are recorded as runs of their own. A task with nothing to
run, such as one that only lists `depends`, is not recorded.

Runs are kept per project (the monorepo root in a monorepo) under
`$MISE_STATE_DIR/task-history/`, the last
[`task.history_limit`](/configuration/settings.html#task.history_limit) runs of
each task (20 by default). Set
[`task.history`](/configuration/settings.html#task.history) to `false` to stop
recording. Recording never fails a run, and `mise run --dry-run` records
nothing.

Arguments are stored redacted, like terminal output, with URL credentials
removed. A run that ends before its task's environment is resolved, such as one
skipped because its sources are fresh, is stored without arguments, since the
secrets that environment would register are not known yet.

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

A script or coding agent that needs to block until a file changes can use
`--once` instead of a sleep and poll loop. mise runs nothing at start. It waits
until one of the task's sources changes, runs the task once, and exits with the
task's exit status:

```sh
mise watch test --once && echo "tests pass after the change"
```

`--once` implies `--postpone`. If watchexec stops before anything changes, mise
exits with 1. To keep watching but stop at the first failed run, pass
`--exit-on-error` without `--once`.

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

## Run tasks at another revision

`--at REV` runs tasks as they are at another git commit, without touching your
checkout. It answers questions like "does this test fail on `main` too?":

```sh
mise run --at main test
mise run --at HEAD~1 bench > before.txt
```

mise checks REV out into a detached
[`git worktree`](https://git-scm.com/docs/git-worktree) under its temporary
directory, runs the tasks there with that revision's own config, tasks and
tools, and removes the worktree afterwards, also when a task fails, the tasks
time out (`--timeout`) or you press Ctrl-C. The run exits with the tasks'
status. Uncommitted changes in your checkout play no part: REV is a commit.

- The tasks run in the same directory, relative to the repository root, as
  the one you ran mise from. The worktree directory has your checkout's name.
- Submodules are initialized at REV, using your clones of them as a reference,
  so only objects they lack are fetched. Git hooks do not run, neither your
  checkout's nor any that REV keeps in the repository.
- Ignored files are not there: a fresh worktree has no `target/`,
  `node_modules/` or `.env`. Pass `--at-setup CMD` to prepare them. It runs in
  the worktree's copy of the directory you ran mise from (not a task's `dir`)
  before the tasks start, as a plain shell command outside
  mise's environment, with `MISE_RUN_AT_SOURCE` set to your checkout and
  `MISE_RUN_AT_REV` to the commit. The tasks get both variables too:

  ```sh
  mise run --at main --at-setup 'cp -R "$MISE_RUN_AT_SOURCE/node_modules" .' test
  ```

  With `--dry-run`, mise prints the setup command instead of running it.
  `--timeout` limits only the tasks, not the setup command.

- A `mise run --at` inside a `--at` run (where `MISE_RUN_AT_REV` is set) is
  refused.

- Config is looked up from the worktree, so a `mise.toml` in a directory above
  your checkout (other than the global config) does not apply.

- [Daemons](/daemons.html) that the tasks require keep running after the run,
  as they do after any `mise run`, but their project is gone with the worktree.
  `mise daemons prune` stops them and removes their state.

The config at REV is the config of the same project, at another commit, so it
shares the [trust](/cli/trust.html) of the repository's main checkout, as any
linked worktree does. If the main checkout is not trusted, `--at` refuses to
run before anything starts. It never trusts the config at REV on its own: not
by a prompt, not with `--yes`, and not the way `mise run` trusts the config of
the directory you run it in. In [paranoid mode](/paranoid.html), which ties
trust to a file's path and contents, a config at REV is never trusted, because
the worktree is a new path on every run.

## Task environment {#task-environment}

Tasks run with the project's [environment variables](/environments/) and tools,
plus these variables:

| Variable                    | Value                                                                                                                                                                                                  |
| --------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `MISE_ORIGINAL_CWD`         | The directory where you ran mise.                                                                                                                                                                      |
| `MISE_CONFIG_ROOT`          | The [config root](/configuration.html#config-root) of the file that defines the task, such as `~/proj` for both `~/proj/mise.toml` and `~/proj/.config/mise.toml`.                                     |
| `MISE_PROJECT_ROOT`         | The root of the project that defines the task, whichever directory you run it from. In a monorepo, the subproject's directory. For a global or remote task, the project you run it in.                 |
| `MISE_MONOREPO_ROOT`        | The directory whose config sets `monorepo_root = true`. Set only in a [monorepo](/tasks/monorepo.html).                                                                                                |
| `MISE_TASK_NAME`            | The task's name as `mise tasks ls` shows it. A file task's name has no script extension unless another task already has the shorter name; see [file task names](/tasks/file-tasks.html#task-grouping). |
| `MISE_TASK_FILE`            | The task's script: the file task itself, or the file named by a TOML task's `file`. For a TOML task with an inline `run`, the config file that defines it.                                             |
| `MISE_TASK_DIR`             | The directory that contains `MISE_TASK_FILE`.                                                                                                                                                          |
| `MISE_TASK_COLOR`           | The ANSI sequence for the task's label color. Empty when colors are off, the task is quiet, or the output style shows no label (`interleave`, `silent`).                                               |
| `MISE_TASK_LOCKS`           | The [locks](/tasks/task-configuration.html#lock) the task holds, comma-separated, including those inherited from a parent task. Set only when the task holds or inherits a lock.                       |
| `MISE_ENV`                  | The active [config environments](/configuration/environments.html), comma-separated, when any are set.                                                                                                 |
| `TRACEPARENT`, `TRACESTATE` | The task's trace context, when [OpenTelemetry](/tasks/opentelemetry.html#trace-propagation) export is on.                                                                                              |
| `usage_*`                   | The values of the task's [arguments and flags](/tasks/task-arguments.html).                                                                                                                            |

For a TOML task in `~/proj/.config/mise.toml`, `MISE_TASK_DIR` is
`~/proj/.config`, while `MISE_CONFIG_ROOT` is `~/proj`. When you print with
`MISE_TASK_COLOR`, reset the color after your text:

```sh
printf '%smessage\033[0m\n' "$MISE_TASK_COLOR"
```
