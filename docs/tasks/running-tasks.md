# Running Tasks

List available tasks with `mise tasks`. To show tasks hidden with `hide=true`, use the `--hidden` option.

List declared dependencies of tasks with `mise tasks deps [tasks]...`.
That graph is built from [`depends`](/tasks/task-configuration.html#depends),
[`wait_for`](/tasks/task-configuration.html#wait-for), and
[`depends_post`](/tasks/task-configuration.html#depends-post).
Task references inside a `run` array (`{ task = "..." }` / `{ tasks = [...] }`)
are execution steps, so they do not appear there.

Run a task with `mise tasks run <task>`, `mise run <task>`, `mise r <task>`, or simply `mise <task>`. Never
use that last form in scripts or documentation: if a future mise version adds a command with the same name,
the task will be shadowed and must be run with one of the other forms.

For interactive use, an alias such as `alias mr='mise run'` can save typing.

By default, tasks execute with a maximum of 4 parallel jobs. Customize this with the `--jobs` option,
the `jobs` setting, or the `MISE_JOBS` environment variable. Output is normally printed line by line, prefixed
with the task label; this avoids interleaving output from parallel executions. However, if
`--jobs` is `1`, the output mode is set to `interleave`.

To print stdout/stderr directly, use `--output interleave`, the `task.output` setting, or `MISE_TASK_OUTPUT=interleave`.

The output _style_ (`prefix`, `interleave`, `keep-order`, …) is independent of _verbosity_
(`--quiet`/`--silent`, the `quiet`/`silent` settings, or the per-task `quiet`/`silent` fields).
They combine: e.g. `MISE_TASK_OUTPUT=prefix` with `--quiet` keeps the task-name prefixes while
suppressing mise's own messages. `--quiet` no longer forces un-prefixed output — use
`--output quiet` (or `-o interleave`) if you want the old un-prefixed behavior.

Stdin is not connected by default. Set `interactive = true` for a task that needs
the terminal; it has exclusive terminal access for the duration of the task.
`raw = true` takes exclusive access per command instead. Both bypass output
redaction and artifact caching. See [terminal I/O options](./task-configuration.html#interactive).

Extra arguments are passed to the task. For example, to run in release mode:

```bash
mise run build --release
```

For a precise, validated task interface, define arguments and flags with the
[`usage` field](/tasks/task-arguments#usage-field). Without a `usage` specification, extra arguments
are forwarded according to how the task is executed:

- If `run` is an array, the arguments are passed only to its last entry.
- For a regular inline shell command, the arguments are appended to the command text.
- A [shebang task](/tasks/toml-tasks#shell-shebang) is executed as a script file, so its interpreter
  exposes the arguments normally—for example, as `$1` and `$@` in Bash.

Because everything after the task name belongs to the task, mise's own flags have to come
_before_ it—`mise run --silent build` rather than `mise run build --silent`, which passes
`--silent` to the task and fails with `unexpected word: --silent` unless the task defines it.
This also means a task is free to define a flag that shares a name with a mise flag, e.g. a
task with its own `--env`.

:::tip
You can define arguments and flags for tasks, which provides validation, parsing, autocomplete, and documentation.

- [Arguments in File Tasks](/tasks/file-tasks#arguments)
- [Arguments in TOML Tasks](/tasks/toml-tasks#arguments)

Autocomplete works automatically for tasks when mise's shell completions are installed and enabled.

Markdown documentation can be generated with [`mise generate task-docs`](/cli/generate/task-docs).
:::

Multiple tasks and their arguments can be separated with the `:::` delimiter:

```bash
mise run build arg1 arg2 ::: test arg3 arg4
```

If no task is specified, mise runs the task named "default", if you've defined one. You can also alias a different task to "default".

```bash
mise run
```

## Task Grouping

Tasks can be grouped semantically using name prefixes separated by `:`.
For example, all testing-related tasks might begin with `test:`. Nested groups
further refine grouping and simplify pattern matching.
For example, `mise run test:**:local` matches `test:units:local`,
`test:integration:local`, and `test:e2e:happy:local`
(see [Wildcards](#wildcards) for more information).

::: tip
Since TOML keys can't contain colons without quoting, use quoted keys in `mise.toml`:

```toml
[tasks."test:unit"]
run = 'cargo test --lib'
```

:::

## Wildcards

Glob-style wildcards are supported when running tasks or specifying task
dependencies.

Available wildcard patterns:

- `?` matches any single character
- `*` matches 0 or more characters within a single `:`-delimited group
- `**` matches 0 or more complete `:`-delimited groups
- `{glob1,glob2,...}` matches any of the comma-separated glob patterns
- `[ab,...]` matches any of the characters or ranges `[a-z]`
- `[!ab,...]` matches any character not in the character set

### Examples

`mise run 'generate:{completions,docs:*}'`

For grouped tasks, use `*` when exactly one group may vary and `**` when the
match may cross multiple groups:

```bash
# Matches test:units:local, but not test:e2e:happy:local
mise run 'test:*:local'

# Matches both test:units:local and test:e2e:happy:local
mise run 'test:**:local'
```

If a pattern relied on `*` matching nested task groups in an older mise
version, replace it with `**` to keep the recursive behavior.

And with dependencies:

```toml
[tasks."lint:eslint"] # using a ":" means we need to add quotes
run = "eslint ."
[tasks."lint:prettier"]
run = "prettier --check ."
[tasks.lint]
depends = ["lint:*"]
wait_for = ["render"] # does not add as a dependency, but if it is already running, wait for it to finish
```

## Running on file changes

It's often handy to execute a task only if the files it uses have changed. For example, you might only want
to run `cargo build` if a `.rs` file changes. This can be done with the following config:

```toml
[tasks.build]
description = 'Build the CLI'
run = "cargo build"
sources = ['Cargo.toml', 'src/**/*.rs'] # skip running if these files haven't changed
outputs = ['target/debug/mycli']
```

Now if `target/debug/mycli` exists and is newer than `Cargo.toml` and every matching `.rs` file, the task is skipped. This uses last-modified timestamps.
The task definition is also an input. Missing declared outputs cause the task to
run again. For content-based reuse that can restore deleted outputs, see
[task caching](./caching.html).

## Watching files

Run a task when its sources change with [`mise watch`](/cli/watch.html):

```bash
mise watch build
```

`mise watch` uses `watchexec`. Add it to your project with `mise use watchexec`
or install it separately on `PATH`. Declare the task's `sources` to limit the
watched files. Without a task name, mise watches the `default` task.

## `mise run` shorthand

Tasks can be run with `mise run <TASK>` or `mise <TASK>`—as long as the name doesn't conflict with a mise command.
Because mise may later add a command with a conflicting name, it's recommended to use `mise run <TASK>` in
scripts and documentation.

## Execution order

You can use [depends](/tasks/task-configuration.html#depends), [wait_for](/tasks/task-configuration.html#wait-for), and [depends_post](/tasks/task-configuration.html#depends-post) to control the order of execution.

```toml
[tasks.build]
run = "echo 'build'"

[tasks.test]
run = "echo 'test'"
depends = ["build"]
```

This ensures the `build` task runs before the `test` task.

You can also define a mise task to run other tasks in parallel or in series:

```toml
[tasks.example1]
run = "echo 'example1'"

[tasks.example2]
run = "echo 'example2'"

[tasks.example3]
run = "echo 'example3'"

[tasks.one_by_one]
run = [
    { task = "example1" }, # will wait for example1 to finish before running the next step
    { tasks = ["example2", "example3"] }, # these 2 are run in parallel
]
```

`mise run one_by_one` runs that pipeline, but `mise tasks deps one_by_one` still
shows it as a leaf. Those `{ task }` / `{ tasks }` entries are this task's own `run`
steps, not graph edges. The nested tasks still run, including their own
`depends`. Rewriting them as `depends = ["example1", "example2", "example3"]`
would put them in the graph, but it would also drop the sequential/parallel
ordering above: `depends` only requires those tasks to finish first, with no
order among them.
