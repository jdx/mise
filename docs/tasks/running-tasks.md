---
description: "Find and run project tasks, pass arguments, and control execution."
---

# Running Tasks

## Find and run tasks

List available tasks with `mise tasks`. To include tasks hidden with
`hide=true`, pass `--hidden`.

### `mise run` shorthand {#mise-run-shorthand}

Run a named task with `mise tasks run <task>`, `mise run <task>`, `mise r <task>`,
or `mise <task>`. Use `mise run <task>` in scripts and documentation. A future
mise command can shadow the direct form.

For interactive use, an alias such as `alias mr='mise run'` can save typing.

## Pass arguments and select tasks

Pass arguments after the task name:

```bash
mise run build --release
```

For a precise, validated task interface, define arguments and flags with the
[`usage` field](/tasks/task-arguments#usage-field). Without a `usage` specification,
mise forwards extra arguments according to the task's form:

- If `run` is an array, the arguments go only to its last entry.
- For a regular inline command, arguments are appended as literal arguments
  (shell-quoted when a shell is used).
- A [shebang task](/tasks/toml-tasks#shell-shebang) runs as a script file, so
  its interpreter exposes arguments normally—for example, as `$1` and `$@` in
  Bash.

Put mise flags before the task name: `mise run --silent build`. A flag after
the task name belongs to the task, so `mise run build --silent` fails with
`unexpected word: --silent` unless the task defines it. Tasks can define flags
that share names with mise flags, such as `--env`.

::: tip
Task arguments and flags provide validation, parsing, autocomplete, and
documentation.

- [Arguments in File Tasks](/tasks/file-tasks#arguments)
- [Arguments in TOML Tasks](/tasks/toml-tasks#arguments)

Autocomplete works when mise's shell completions are installed and enabled.
Generate Markdown documentation with [`mise generate task-docs`](/cli/generate/task-docs).
:::

### Run several tasks

Separate tasks and their arguments with `:::`:

```bash
mise run build arg1 arg2 ::: test arg3 arg4
```

### Run the default task

When no task is specified, mise runs `default` when that task is defined.
Otherwise, an interactive terminal opens the task selector. You can also alias
another task to `default`:

```bash
mise run
```

## Control execution

### Parallelism and output

Tasks run with a maximum of four parallel jobs by default. Set `--jobs`, the
`jobs` setting, or `MISE_JOBS` to choose another limit. Output normally prints
one line at a time with the task label, which keeps parallel output readable.
With `--jobs 1`, mise uses `interleave` output.

To print stdout and stderr directly, use `--output interleave`, the
`task.output` setting, or `MISE_TASK_OUTPUT=interleave`.

The output _style_ (`prefix`, `interleave`, `keep-order`, …) is separate from
_verbosity_ (`--quiet`/`--silent`, the `quiet`/`silent` settings, or per-task
`quiet`/`silent` fields). For example, `MISE_TASK_OUTPUT=prefix` with
`--quiet` keeps task-name prefixes and hides mise's messages. Use
`--output interleave --quiet` for un-prefixed, quiet output. To make every task
quiet without changing other mise commands, set `task.output = "interleave"`
and `task.quiet = true` under `[settings]`.

::: warning Deprecated
The `quiet` output value is deprecated. Warnings begin in mise `2026.9.3`, and support will be
removed in `2027.9.3`. Combine `interleave` with the task-scoped or command-line quiet option
instead.
:::

### Interactive input

Stdin is not connected by default. Set `interactive = true` for a task that needs
the terminal; it has exclusive terminal access for the duration of the task.
`raw = true` takes exclusive access per command instead. Both bypass output
redaction and artifact caching. See [terminal I/O options](./task-configuration.html#interactive).

### Shell execution

On Unix, mise can execute simple inline commands such as `node build.js` directly,
without starting the default `sh`. Shell syntax, quoting, expansion, builtins,
ambiguous executable lookup, and environments containing `ENV` or `BASH_ENV`
keep using the shell. Sandboxed and audited tasks also retain their shell.
Windows execution is unchanged.

:::warning Custom shell wrappers
An explicit task `shell`, `mise run --shell`, or `unix_default_inline_shell_args`
setting always forces shell execution, even when it names the default shell.
A wrapper named `sh` on `PATH` may be bypassed; configure it explicitly if it
must run.
:::

The same optimization applies to mise-owned inline hooks, templates,
dependency commands, installation commands, credentials, and task cache inputs.

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

## Execution order

You can use [depends](/tasks/task-configuration.html#depends), [wait_for](/tasks/task-configuration.html#wait-for), and [depends_post](/tasks/task-configuration.html#depends-post) to control the order of execution.

List the declared graph with `mise tasks deps [tasks]...`. It includes
`depends`, `wait_for`, and `depends_post`. Task references in a `run` array
(`{ task = "..." }` / `{ tasks = [...] }`) are execution steps, so they do not
appear in the graph.

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
