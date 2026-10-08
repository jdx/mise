---
description: "Look up every task property, task_config option, task environment variable, and setting that controls mise tasks."
socialDescription: "Look up every task property, task_config option, and setting for mise tasks."
---

# Task configuration reference

Find every property a task accepts, the `[task_config]` options that set
defaults for a config root, the environment variables mise gives each task, and
the settings that change how tasks run. New to tasks? Start with
[TOML tasks](/tasks/toml-tasks.html) or [file tasks](/tasks/file-tasks.html).

Set properties in a `[tasks.<name>]` table in `mise.toml`, in a
[TOML task file](/tasks/task-discovery.html#included-toml-files), or in `#MISE`
comments at the top of a file task. File-task headers accept the same
properties except `run`, `run_windows`, `file`, `vars`, `timeout`, and the
`deny_*` and `allow_*` [sandbox properties](#sandbox). Arguments use `#USAGE`
lines instead of `usage`. mise ignores those keys in a header with an
`unknown field(s)` warning. To set them for a file task, use a
`[tasks.<name>]` block; see
[configuring file tasks from TOML](/tasks/task-discovery.html#configuring-file-tasks-from-toml).
To share properties between tasks, use [task templates](/tasks/templates.html).

::: warning Experimental
Properties and options marked experimental require this setting. Without it,
mise reports an error when a task uses them.

```toml
[settings]
experimental = true
```

:::

## Task properties

| Group                                                     | Properties                                                                                                                                                                                                                     |
| --------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| [Command](#command)                                       | [`run`](#run), [`run_windows`](/tasks/task-configuration.html#run-windows), [`file`](#file), [`shell`](#shell), [`dir`](#dir), [`usage`](#usage), [`raw_args`](/tasks/task-configuration.html#raw-args), [`extends`](#extends) |
| [Description and visibility](#description-and-visibility) | [`description`](#description), [`alias`](#alias), [`hide`](#hide), [`confirm`](#confirm)                                                                                                                                       |
| [Ordering](#ordering)                                     | [`depends`](#depends), [`depends_post`](/tasks/task-configuration.html#depends-post), [`wait_for`](/tasks/task-configuration.html#wait-for), [`daemons`](/tasks/task-configuration.html#daemons)                               |
| [Environment and tools](#environment-and-tools)           | [`env`](#env), [`vars`](#task-vars), [`secrets`](/tasks/task-configuration.html#secrets), [`tools`](#tools)                                                                                                                    |
| [Skipping work](#skipping-work)                           | [`sources`](#sources), [`outputs`](#outputs), [`watch`](#watch), [`cache`](/tasks/task-configuration.html#cache)                                                                                                               |
| [Execution](#execution)                                   | [`timeout`](#timeout), [`raw`](#raw), [`interactive`](#interactive)                                                                                                                                                            |
| [Output](#output-properties)                              | [`output`](#output), [`quiet`](#quiet), [`silent`](#silent)                                                                                                                                                                    |
| [Sandbox](#sandbox)                                       | `deny_all`, `deny_read`, `deny_write`, `deny_net`, `deny_env`, `allow_read`, `allow_write`, `allow_net`, `allow_env`, `pass_through_env`                                                                                       |
| [Deprecated](#deprecated)                                 | [`rust_cache`](/tasks/task-configuration.html#rust-cache)                                                                                                                                                                      |

The rest of the page covers [`[task_config]` options](#task-config-options),
[environment variables mise sets](#environment-variables-mise-sets), and
[settings](#settings).

## Command

### `run`

- **Type**: `string | (string | { task: string, args?: string[], env?: { [key]: string } } | { tasks: string[] })[]`

The command to run. A string runs one script. An array runs its entries in
order and stops at the first one that fails.

```mise-toml
[tasks.test]
run = "cargo test"

[tasks.ci]
run = ["cargo fmt --check", "cargo clippy", "cargo test"]
```

An array entry can also run other tasks. `{ task = "name" }` runs one task,
with optional `args` and `env`, and `{ tasks = [...] }` runs several in
parallel. Each referenced task runs with its own dependencies.

```mise-toml
[tasks.release]
run = [
  { task = "build", args = ["--release"], env = { RUSTFLAGS = "-C opt-level=3" } },
  { tasks = ["test", "lint"] }, # run in parallel
  "./scripts/publish.sh",
]
```

These steps are not [`depends`](#depends) edges, so
[`mise tasks deps`](/cli/tasks/deps.html) does not show them.

A task can use [`file`](#file) instead of `run`, inherit `run` from a template
with [`extends`](#extends), or have only `depends` to group other tasks. A task
that needs nothing but `run` can be written in one line, such as
`tasks.lint = "eslint ."` or `tasks.ci = ["cargo fmt --check", "cargo test"]`.

### `run_windows`

- **Type**: same as [`run`](#run)

On Windows, mise runs this instead of `run`. It accepts the same forms as
`run`. Without it, Windows runs `run`.

```mise-toml
[tasks.build]
run = "cargo build"
run_windows = "cargo build --features windows"
```

A file task has no `run_windows`; see [Windows](/tasks/file-tasks.html#windows)
for pairing a script with a Windows version.

### `file`

- **Type**: `string`

Run a script file instead of an inline `run` command. mise resolves a relative
path from the task's config root, the same directory used as the default
[`dir`](#dir) (for `~/src/myproj/.config/mise.toml`, this is `~/src/myproj`).
The path supports Tera templates.

```toml
[tasks.release]
description = "Cut a new release"
file = "scripts/release.sh"
```

`file` also accepts HTTP(S) URLs and
[`git::` URLs](/tasks/task-discovery.html#git-url-syntax). See
[remote tasks](/tasks/toml-tasks.html#remote-tasks) for caching and security.

### `shell`

- **Type**: `string`
- **Default**: [`task_config.shell`](#task_config.shell) if set, otherwise the
  [`unix_default_inline_shell_args`](/configuration/settings.html#unix_default_inline_shell_args)
  or [`windows_default_inline_shell_args`](/configuration/settings.html#windows_default_inline_shell_args)
  setting

The interpreter command, with its arguments, that runs the task's `run`
scripts:

```mise-toml
[tasks.lint]
shell = "bash -c"
run = "[[ -f Cargo.lock ]] && cargo clippy"
```

A [shebang](/tasks/toml-tasks.html#shell-shebang) on the first line of a `run`
script also selects the interpreter, and lets editors highlight the script.

When the shell is PowerShell (`pwsh` or `powershell`), mise passes `-NoProfile`
so your PowerShell profile is not loaded, matching the non-interactive behavior
of `sh -c`. This keeps a profile that changes `PATH`, such as a mise activation
snippet, from shadowing the task's tools. Set
[`windows_powershell_no_profile`](/configuration/settings.html#windows_powershell_no_profile)
to `false` if your tasks depend on the profile.

mise runs an executable file directly, so `shell` applies to a file task or a
`file` script only when mise starts the interpreter itself, for example for a
script on Windows or with
[`use_file_shell_for_executable_tasks`](/configuration/settings.html#use_file_shell_for_executable_tasks)
set to `true`.

### `dir`

- **Type**: `string`
- **Default**: [`task_config.dir`](#task_config.dir) if set, otherwise
  <code v-pre>{{ config_root }}</code>, the project directory (for
  `~/src/myproj/.config/mise.toml`, that is `~/src/myproj`)

The directory the task runs in. The value supports Tera templates. Set it to
<code v-pre>{{ cwd }}</code> to run the task in the directory you called mise
from:

```mise-toml
[tasks.test]
dir = "{{ cwd }}"
run = "cargo test"
```

The directory you called mise from is also available to every task as
`MISE_ORIGINAL_CWD`.

### `usage`

- **Type**: `string`

A [usage spec](/tasks/task-arguments.html) that declares the task's arguments
and flags. mise parses them, shows them in `--help` and completions, and
exposes each value as `$usage_<name>` and <code v-pre>{{ usage.name }}</code>.
File tasks declare arguments with `#USAGE` comment lines instead.

```mise-toml
[tasks.test]
usage = '''
arg "<file>" help="The file to test" default="src/main.rs"
'''
run = 'cargo test ${usage_file?}'
```

An argument or flag can take its value from an environment variable with
`env="..."`; see
[environment variable backing](/tasks/task-arguments.html#environment-variable-backing).

### `raw_args`

- **Type**: `bool`
- **Default**: `false`

When `true`, mise does not parse the task's arguments at all. Every argument,
including `--help` and `-h`, goes to the command unchanged. Use it for a task
that wraps a tool with its own argument parser, such as `next build`, Django's
`manage.py`, or a Python script that uses `argparse`:

```mise-toml
[tasks.manage]
raw_args = true
run = "python manage.py"
```

```sh
mise run manage --help          # forwarded to manage.py
mise run manage migrate --fake  # every flag reaches manage.py unchanged
```

Without `raw_args`, mise answers `--help` with its own task help. For a single
call, `mise run task -- --help` also skips mise's argument parser for `--help`
and `-h`. Arguments after that separator belong to the task, so
`mise run task -- -- --help` forwards `-- --help`.

### `extends`

- **Type**: `string`

The name of a [task template](/tasks/templates.html) to inherit properties
from. Properties set on the task override the template's, following the
[merge rules](/tasks/templates.html#merge-semantics).

```mise-toml
[task_templates."python:test"]
run = "uv run pytest"
tools = { python = "3.14", uv = "latest" }

[tasks.test]
extends = "python:test"
env = { PYTHONPATH = "src" }
```

A file task names its template with `#MISE extends="python:test"`.

## Description and visibility

### `description`

- **Type**: `string`

Shown by `mise tasks ls`, the `mise run` picker, the task's `--help`, and shell
completions.

```mise-toml
[tasks.build]
description = "Build the CLI"
run = "cargo build"
```

### `alias`

- **Type**: `string | string[]`

Other names that run the task:

```mise-toml
[tasks.build]
alias = "b" # mise run b
run = "cargo build"
```

A task's name takes precedence over an alias, so if a task named `b` exists,
`mise run b` runs it, even when the alias comes from a parent directory's
config.

### `hide`

- **Type**: `bool`
- **Default**: `false`

Hide the task from help output, completions, and `mise tasks ls`, for example
for internal or deprecated tasks. `mise tasks ls --hidden` lists hidden tasks,
and `mise run` still runs them.

```mise-toml
[tasks.internal]
hide = true
run = "echo my internal task"
```

### `confirm`

- **Type**: `string | { message: string, default?: "yes" | "no", yes?: string, no?: string }`

A prompt shown before the task's own command runs, for tasks that are
destructive or slow. The task's [`depends`](#depends) have already run by
then. To ask first, put `confirm` on those tasks, or run them as
`run = [{ task = "..." }]` steps, which come after the prompt.

```mise-toml
[tasks.deploy]
confirm = { message = "Deploy to production?", yes = "Deploy", no = "Cancel", default = "no" }
run = "./deploy.sh"
```

`yes` and `no` change the labels of the two answers, and `default` sets the
answer selected first (`yes` unless set). Piped answers
(`echo y | mise run deploy`) still accept `y` and `n`. `mise --yes` or
`MISE_YES=1` accepts without asking. When there is no terminal to ask and no
piped answer, the task fails.

The message and the labels support Tera templates and can use the task's
arguments:

```mise-toml
[tasks.deploy]
usage = '''
arg "<environment>" help="Environment to deploy to"
flag "--force" help="Force deployment"
'''
confirm = "Deploy to {{ usage.environment }}?{% if usage.force %} (forced){% endif %}"
run = "./deploy.sh ${usage_environment}"
```

## Ordering

For how mise schedules tasks from these properties, see
[Dependencies and execution order](/tasks/architecture.html).

### `depends`

- **Type**: `string | (string | string[] | { task: string, args?: string[], env?: { [key]: string }, optional?: bool })[]`

Tasks that must finish before this task runs. When several tasks share a
dependency, it runs once. mise runs whatever it can in parallel, up to
[`jobs`](/configuration/settings.html#jobs) tasks at a time.

Each entry is a task name, an alias, or a
[pattern](/tasks/running-tasks.html#wildcards) such as `lint:*`, optionally
followed by arguments. Write it as one string (`"build --release"`), as an
array (`["build", "--release"]`), or as a table
(`{ task = "build", args = ["--release"] }`). In a monorepo, use `//path:task`
names; see [Monorepo tasks](/tasks/monorepo.html).

```mise-toml
[tasks.build]
run = "cargo build"

[tasks.test]
depends = ["build", "lint:*"]
run = "cargo test"
```

[`mise tasks deps`](/cli/tasks/deps.html) shows the graph that `depends`,
`depends_post`, and `wait_for` declare.

#### Passing environment variables to dependencies

Set variables for one dependency with a `VAR=value` prefix or an `env` table.
They apply only to that dependency, not to this task or its other
dependencies:

```mise-toml
[tasks.test]
depends = [
  "NODE_ENV=test setup",
  { task = "build", args = ["--release"], env = { RUSTFLAGS = "-C opt-level=3" } },
]
run = "npm test"
```

#### Passing parent task arguments to dependencies

Forward this task's arguments to a dependency with
<code v-pre>{{ usage.name }}</code> templates. Both tasks need a `usage` spec
for the arguments they accept:

```mise-toml
[tasks.build]
usage = 'arg "<app>"'
run = 'echo "building {{ usage.app }}"'

[tasks.deploy]
usage = 'arg "<app>"'
depends = [{ task = "build", args = ["{{ usage.app }}"] }]
run = 'echo "deploying {{ usage.app }}"'
```

`mise run deploy myapp` passes `myapp` to both `deploy` and its `build`
dependency. The string form works too
(<code v-pre>depends = ["build {{ usage.app }}"]</code>), and so do flags
(<code v-pre>args = ["--target", "{{ usage.target }}"]</code>). Each task in a
chain can forward its own resolved arguments to its dependencies.

#### Optional dependencies

Set `optional = true` on a table entry to run the matching tasks when they
exist, without failing when the name or pattern matches nothing. An invalid
pattern is still an error.

```toml
[tasks.test]
depends = [
  { task = "//...:test", optional = true },
  { task = "//...:test:*", optional = true },
]
```

### `depends_post`

- **Type**: same as [`depends`](#depends)

Like `depends`, but these tasks run after this task finishes. Use it for
cleanup or reporting that must follow the task:

```mise-toml
[tasks.test]
run = "npm test"
depends_post = ["stop-services"]
```

Post-dependencies run whether this task succeeds or fails, as long as it
started. If one of its `depends` fails first, they are skipped. Their own
dependencies also wait for this task, so a whole cleanup chain runs after the
main work. A task listed in both `depends` and `depends_post` runs twice, once
before and once after. Entries accept the same arguments, environment
variables, and `optional` flag as `depends`.

### `wait_for`

- **Type**: same as [`depends`](#depends)

Like `depends`, this waits for the listed tasks to finish before running.
Unlike `depends`, it does not add them to the run; it only waits for them when
they are already scheduled.

```mise-toml
[tasks.lint]
wait_for = ["render"] # render writes JS files; if it is running, wait for it
run = "eslint ."
```

Entries accept the same arguments, environment variables, and `optional` flag
as `depends`. Use `optional = true` to allow a name or pattern that matches no
configured task. Matching depends on what the entry specifies:

- `wait_for = ["setup"]` matches `setup` by name, whatever its arguments or
  environment. It waits for a `setup` started by `depends = ["DEBUG=1 setup"]`.
- `wait_for = ["setup arg1"]` or `wait_for = ["DEBUG=1 setup"]` matches only a
  `setup` running with those exact arguments or environment variables.

### `daemons` <Badge type="warning" text="experimental" />

- **Type**: `bool | string | string[]`

[Project daemons](/daemons.html) that must be running and ready before the task
runs. Requires pitchfork 2.25.0 or later.

| Value                   | Requirement                                      |
| ----------------------- | ------------------------------------------------ |
| `"postgres"`            | One named daemon.                                |
| `["postgres", "redis"]` | Each named daemon.                               |
| `true`                  | All daemons in the task's project configuration. |
| `false` or omitted      | No daemon requirement.                           |

```mise-toml
[daemons]
postgres = "18"

[tasks.test]
daemons = "postgres"
run = "npm test"
```

mise starts missing daemons and waits until they are ready before the task
runs. They keep running afterwards; stop them with
[`mise daemons stop`](/cli/daemons/stop.html).

Names must match `[daemons]` entries in the task's own project configuration
hierarchy, including inherited declarations. In a monorepo, a dependency task
in another project resolves its names there, not in the calling project's
config. An unknown name fails the run.

`--skip-deps` and the `task.skip_depends` setting skip daemon requirements.
`--dry-run` still checks the names and the experimental setting but starts
nothing. Safe mode blocks task daemon startup.

A subtask reached through a `run = [{ task = "..." }]` entry is resolved after
the run has started, so its own `daemons` are not started. Declare the
requirement on the task you invoke. For setup and readiness checks, see the
[daemon guide](/daemons.html#tasks-that-require-daemons).

## Environment and tools

### `env`

- **Type**: `{ [key]: string | int | bool | directive }`, the same value forms
  as top-level [`[env]`](/environments/), including directive tables such as
  `{ required = true }` and `_.file` or `_.path` directives

Environment variables for this task's commands. They are not passed to its
`depends` tasks. A task started from `run`, as `mise run other-task` is here,
inherits them like any child process.

```mise-toml
[tasks.test]
env.TEST_ENV_VAR = "ABC"
run = [
  "echo $TEST_ENV_VAR",
  "mise run other-task",
]
```

A value can reference a secret with <code v-pre>{{ secrets.NAME }}</code>
<Badge type="warning" text="experimental" />. mise composes the value when the
task starts, and the reference grants the key to this task. See
[compose values](/environments/secrets/fnox.html#compose-values). To hide
values in task output, see [redaction](/environments/secrets/#redaction).

### `vars` {#task-vars}

- **Type**: `{ [key]: string | int | bool | directive }`

Values available as <code v-pre>{{ vars.NAME }}</code> when mise renders this
task. Top-level [`[vars]`](/configuration/vars.html) are available in every
task; task-local values override them and vars inherited from a task template.
Vars are not exported as environment variables; use [`env`](#env) for values
the task process should see.

```mise-toml
[vars]
mode = "headless"

[tasks.test]
vars = { mode = "headed" }
run = "echo --mode={{ vars.mode }}"
```

`mise run test` prints `--mode=headed`. Other tasks still use the config value,
`headless`, unless they define their own override.

Overrides apply to references in the task's templated fields, including
inherited fields. They do not recalculate top-level vars that were already
resolved during config loading. See
[variable resolution](/configuration/vars.html#what-a-task-local-var-can-change)
for an example, and
[task template vars](/tasks/templates.html#parameterizing-a-template-with-vars)
for sharing a command with different values in each task. For value directives,
see [configuration variables](/configuration/vars.html#value-directives).

### `secrets` <Badge type="warning" text="experimental" />

- **Type**: `string | string[]`

Secret keys this task receives when it starts, resolved from the project's
[secrets source](/environments/secrets/fnox.html) (`[secrets.fnox]`). Only this
task gets them: dependencies, post-dependencies, and `run = [{ task = "..." }]`
subtasks receive only their own lists.

```mise-toml
[secrets.fnox]

[tasks.deploy]
secrets = ["DEPLOY_KEY", "DATABASE_URL"]
run = "./deploy.sh"
```

mise releases before 2026.10.4 reject this field, so set
`min_version = "2026.10.4"` if older clients read the config.

mise redacts the values from the task's output and ignores `--raw` for the
task. Set [`raw`](#raw) or [`interactive`](#interactive) on the task itself to
give it the terminal when mise runs in one; its output is then not redacted. A
task that receives secrets skips the [artifact cache](/tasks/caching.html),
because cached logs or outputs could contain them. Freshness checks still
apply.

`secrets` is not allowed in task templates or `monorepo.task_defaults`. It is
not available to tasks defined in global or system config or in a config file
in or above your home directory, to remote tasks, or to tasks started by hooks,
`watch_files`, daemons, or `mise bootstrap`. Grant
secrets to a global or remote task for one run with `mise run --secrets KEY` or
`mise run --secrets-all`.

### `tools`

- **Type**: `{ [key]: string | { version | path | prefix | ref: string, ...tool options } }`

Tools to install and activate for this task only. They do not apply to its
dependencies.

```mise-toml
[tasks.build]
tools.rust = "1.95"
run = "cargo build"
```

A table takes exactly one of `version`, `path`, `prefix`, or `ref`, plus tool
options, as in [`[tools]`](/dev-tools/).

Run [`mise lock`](/dev-tools/mise-lock.html) to resolve task tools into the
owning config's lockfile. It reads the task definition without running the task
or installing its tools.

Run `mise install --include-task-tools` to install the tools of every task in
the current scope without running any task. Use it to prepare CI caches or
container images; add `--monorepo` to include every configured monorepo root.

## Skipping work

### `sources`

- **Type**: `string | string[]`

Files the task reads. When `sources` is set, mise skips the task if the newest
source file is older than the newest output (outputs default to
`{ auto = true }`), and the set of source files, with their sizes and
timestamps, matches what mise recorded after the last successful run. A missing
output always makes the task run, and so does `mise run --force`. To compare
file contents instead of timestamps, set
[`task.source_freshness_hash_contents`](/configuration/settings.html#task.source_freshness_hash_contents).
To treat equal timestamps as up to date, set
[`task.source_freshness_equal_mtime_is_fresh`](/configuration/settings.html#task.source_freshness_equal_mtime_is_fresh).
See [what makes a task stale](/tasks/caching.html#what-makes-a-task-stale) for
the full rules.

```mise-toml
[tasks.build]
run = "cargo build"
sources = ["Cargo.toml", "src/**/*.rs"]
outputs = ["target/debug/mycli"]
```

This runs `cargo build` only when `mise.toml`, `Cargo.toml`, or a `.rs` file
under `src` changed since the last build. The files that define the task, such
as its config file or script, always count as sources, so editing the task
definition also makes it run.

Entries are paths or glob patterns such as `src/**/*.rs`. Brace alternatives
such as `src/**/*.{js,ts}` work in freshness checks, in
[`mise watch`](/cli/watch.html), which uses `sources` to decide what to watch,
and in [`task_source_files()`](/templates.html#task-source-files). mise checks
the timestamp of every matched file, so very broad globs slow down every run.
When the patterns match no files, mise warns that the task has sources defined
but no matching files were found. This usually means a pattern has a typo.

Relative entries resolve from the task's directory (its [`dir`](#dir), or the
project root when it has none) and may use `..` to reach files above it, such
as a `node_modules` directory at the root of a monorepo:

```mise-toml
[tasks.build]
dir = "packages/web"
run = "npm run build"
sources = ["src/**/*.ts", "../../node_modules/**"]
outputs = ["dist"]
```

`sources` and `outputs` can use the task's parsed [usage](#usage) arguments.
mise renders them for each invocation before it checks freshness or the
artifact cache:

```mise-toml
[tasks.compile]
usage = 'arg "<target>"'
run = "compile {{ usage.target }} --output dist/{{ usage.target }}"
sources = ["src/{{ usage.target }}/**"]
outputs = ["dist/{{ usage.target }}"]
```

Entries can also reference a named group with `@group:<name>`; see
[`task_config.input_groups`](#task_config.input_groups).
[`task_config.global_inputs`](#task_config.global_inputs) adds sources to every
task under a config root.

#### Excluding sources

Entries in `sources` that start with `!` are excluded, following the convention
of gitignore, watchexec, and rsync. Exclusions affect the freshness check,
`task_source_files()`, and which files `mise watch` watches.

```mise-toml
[tasks.build]
sources = ["src/**/*.ts", "!src/**/*.test.ts", "!src/**/*.spec.ts", "tsconfig.json"]
run = "npm run build"
```

mise evaluates entries in order, and the last matching entry wins, so a later
entry can include a file that an earlier `!` excluded. For example,
`["src/**/*.ts", "!src/**/*.test.ts", "src/keep.test.ts"]` excludes every
`*.test.ts` file except `src/keep.test.ts`. To match a literal path that starts
with `!`, escape it as `\!` (`"\\!important.txt"` in a TOML basic string).

#### Dependency invalidation

When a dependency with `sources` runs or restores its outputs, every task that
depends on it runs too; see [Dependencies](/tasks/caching.html#dependencies).

### `outputs`

- **Type**: `string | string[] | { auto: true }`
- **Default**: `{ auto = true }`

The files or directories the task creates. They are the other half of the
[`sources`](#sources) freshness check, and the files the
[artifact cache](/tasks/caching.html) stores and restores.

Entries that start with `!` exclude matching outputs. As with `sources`, mise
evaluates entries in order, a later entry can include a path again, and `\!`
escapes a literal leading `!`. Output globs support brace alternatives such as
`dist/{client,server}/**`.

```mise-toml
[tasks.build]
run = "npm run build"
sources = ["src/**"]
outputs = ["dist", "!dist/**/*.map", "!dist/.vite/**"]
```

Excluded files take no part in freshness checks and are not stored in
artifact-cache entries. When mise restores a cached artifact, it keeps excluded
files that already exist under an output directory.

With the default, `outputs = { auto = true }`, you can use `sources` without
naming an output file, and `outputs = []` declares that the task writes no
files. See [Outputs](/tasks/caching.html#outputs) for how each form affects
freshness checks, and [Requirements](/tasks/caching.html#requirements) for the
outputs the artifact cache needs.

### `watch`

- **Type**: `{ no_vcs_ignore: bool }`
- **Default**: `{ no_vcs_ignore = false }`

Options for running the task through [`mise watch`](/cli/watch.html). By
default, `mise watch` respects VCS ignore files such as `.gitignore`, even for
an ignored path listed in `sources`. Set `watch.no_vcs_ignore` for a task that
watches generated files that are deliberately kept out of version control:

```mise-toml
[tasks.generate]
run = "process generated/output.json"
sources = ["generated/output.json"]
watch = { no_vcs_ignore = true }
```

This is the same as passing `--no-vcs-ignore` to watchexec. watchexec applies
ignore options to the whole watch process, so when you watch several tasks
together and any of them sets this option, VCS ignores are off for all of them.
Keep `sources` narrow: watching large build, distribution, or dependency
directories without VCS ignores can add a lot of filesystem scanning.

### `cache` <Badge type="warning" text="experimental" />

- **Type**: `{ enabled?: bool, audit?: bool, env?: string[], command_inputs?: string[] }`
- **Default**: [`task_config.cache`](#task_config.cache) if set, otherwise
  `{ enabled = false, audit = false, env = [], command_inputs = [] }`

Cache a successful run's outputs and logs, and restore them when the same
inputs come back. See [Task caching](/tasks/caching.html) for requirements,
setup, and debugging, and [Remote task cache](/tasks/remote-cache.html) to
share results between machines.

- `enabled`: store and restore this task's results.
- `env`: names of inherited environment variables whose values become part of
  the cache key.
- `command_inputs`: commands whose output becomes part of the cache key.
- `audit`: on Linux with `strace`, report project files the task read or wrote
  outside its declared sources and outputs.

```mise-toml
[tasks.build]
run = "npm run build"
sources = ["src/**", "package.json", "pnpm-lock.yaml"]
outputs = ["dist"]
cache = { enabled = true, env = ["NODE_ENV"], command_inputs = ["node --version"] }
```

## Execution

### `timeout`

- **Type**: `string`
- **Default**: unset

Maximum execution time for each command the task runs, as a duration such as
`30s`, `5m`, or `1h`. The value supports Tera templates. The task fails if any
command does not finish within the limit, even if the command exits
successfully once stopped. The limit applies separately to each script entry in
`run` (and to a file task's script), so a task with several commands can take
longer in total. `{ task = ... }` and `{ tasks = [...] }` steps follow the
referenced tasks' own timeouts.

```mise-toml
[tasks.integration-test]
run = "./scripts/integration-test.sh"
timeout = "10m"
```

To limit a whole run, use [`mise run --timeout`](/cli/run.html) or the
[`task.timeout`](/configuration/settings.html#task.timeout) setting. When both
a run-wide and a per-task limit apply, the shorter one wins, and `--timeout`
overrides the setting. On Unix, either limit stops the task's processes with
SIGTERM, then SIGKILL 5 seconds later. On Windows, a per-task timeout sends
Ctrl+C and ends the process tree 5 seconds later, which also stops a program
that ignores Ctrl+C, such as a batch file waiting at
`Terminate batch job (Y/N)?`. The run-wide timeout ends the process tree on
Windows at once. The run-wide timeout does not stop the processes of
[`raw`](#raw) tasks.

### `raw`

- **Type**: `bool`
- **Default**: `false`

Connect each of the task's commands directly to your terminal's stdin, stdout,
and stderr. mise does not prefix, capture, or
[redact](/environments/secrets/#redaction) the output, and the task skips the
[artifact cache](/tasks/caching.html).

While a raw command runs, mise starts no other command, so you do not have to
keep other tasks out of its way. The lock is held per command, so other tasks
can run between this task's commands. If you need a whole task to run without
interruption, use [`interactive = true`](#interactive), which holds an
exclusive lock across the task's script commands. The lock is released while a
`{ task = ... }` or `{ tasks = [...] }` step runs.

### `interactive`

- **Type**: `bool`
- **Default**: `false`

Give the task your terminal for its whole run. Like [`raw`](#raw), it connects
stdin, stdout, and stderr directly, but mise holds an exclusive lock from the
task's first script command to its last, so no other task's commands run in the
meantime. Other tasks still run in parallel before and after it. As with `raw`,
mise does not [redact](/environments/secrets/#redaction) the output, and the
task skips the [artifact cache](/tasks/caching.html). `mise run --raw` goes
further: it makes every task raw and runs one task at a time.

## Output {#output-properties}

### `output`

- **Type**: `"prefix" | "interleave" | "keep-order" | "replacing" | "timed" | "quiet" | "silent"`
- **Default**: the [`task.output`](/configuration/settings.html#task.output)
  setting

The output style for this task, the per-task form of `task.output`. Styles
combine freely with the [`quiet`](#quiet) and [`silent`](#silent) properties,
which control how much is shown: `output = "prefix"` with `quiet = true` keeps
the task-name prefixes and hides mise's own messages. The `quiet` and `silent`
values bundle a style with that verbosity and remain for compatibility.

::: warning Deprecated
The `quiet` output value is deprecated. Warnings begin in mise `2026.9.3`, and
support will be removed in `2027.9.3`. Use `output = "interleave"` with
`quiet = true` instead. For a global default, set `task.output = "interleave"`
and `task.quiet = true` under `[settings]`.
:::

### `quiet`

- **Type**: `bool`
- **Default**: `false`

Hide mise's own output for the task, such as the command line it prints
(`[build] $ cargo build`), and show only what the task prints. To hide the
task's own output too, use [`silent`](#silent). `quiet` does not change the
[`output`](#output) style.

### `silent`

- **Type**: `bool | "stdout" | "stderr"`
- **Default**: `false`

Hide all output from the task. With `"stdout"` or `"stderr"`, hide only that
stream.

## Sandbox

These properties restrict what the task's commands can read, write, reach on
the network, and inherit from the environment. An `allow_*` list also turns on
the matching restriction. Relative paths resolve from the task's working
directory. Support and implicit exceptions differ by platform; see
[Sandboxing](/sandboxing.html).

| Property                                                        | Type       | Effect                                                                                                                                                     |
| --------------------------------------------------------------- | ---------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `deny_all`                                                      | `bool`     | Block filesystem reads and writes, network access, and inherited environment variables. `allow_*` entries add exceptions.                                  |
| `deny_read`                                                     | `bool`     | Block filesystem reads, except the system and mise paths the task needs to run.                                                                            |
| `deny_write`                                                    | `bool`     | Block filesystem writes, except implicitly writable paths such as the temporary directory.                                                                 |
| `deny_net`                                                      | `bool`     | Block network access.                                                                                                                                      |
| `deny_env`                                                      | `bool`     | Drop inherited environment variables except `PATH`, `HOME`, `USER`, `SHELL`, `TERM`, `COLORTERM`, and `LANG`.                                              |
| `allow_read`                                                    | `string[]` | Allow reads from these paths and block other reads.                                                                                                        |
| `allow_write`                                                   | `string[]` | Allow writes to these paths, which also become readable, and block other writes.                                                                           |
| `allow_net`                                                     | `string[]` | Not supported: on Linux and macOS a task that sets it fails before it runs. See [access to particular hosts](/sandboxing.html#access-to-particular-hosts). |
| `allow_env`                                                     | `string[]` | Keep these inherited variables, with `*` wildcards such as `MYAPP_*`, and drop the others.                                                                 |
| `pass_through_env` <Badge type="warning" text="experimental" /> | `string[]` | Keep these inherited variables, with `*` wildcards, when environment inheritance is denied, without adding their values to the task cache key.             |

The `deny_*` properties default to `false` and the lists to `[]`.

```mise-toml
[tasks.lint]
run = "eslint ."
deny_all = true
allow_read = ["."]
allow_write = ["./node_modules/.cache"]
allow_env = ["NODE_*"]
```

`pass_through_env` has no effect unless environment sandboxing is active,
through `deny_env`, `deny_all`, `allow_env`, or an equivalent CLI flag or
setting. Use it for values such as short-lived credentials that must not affect
the cache key, and not for values that change generated outputs or logs. Use
[`cache.env`](/tasks/task-configuration.html#cache) when a change to a variable should invalidate the cache.
See [environment variables and cache keys](/tasks/caching.html#environment-variables-and-cache-keys).

## Deprecated

### `rust_cache` <Badge type="danger" text="deprecated" />

- **Type**: `bool | { enabled?: bool }`
- **Default**: `false`

No longer does anything. An enabled value prints a migration warning, and mise
2027.8.14 removes the field. Remove it and run Cargo through
[Mr Boxington](https://mr-boxington.jdx.dev/getting-started) (`mbx`) instead.
If mise manages Rust, run
`mise use --tool-option mr_boxington=true rust mr-boxington`; see
[share Cargo builds with Mr Boxington](/lang/rust.html#share-cargo-builds-with-mr-boxington).
Otherwise, configure a [`cargo` command wrapper](/dev-tools/shims.html#command-wrappers).

## `[task_config]` options {#task-config-options}

Options in the top-level `[task_config]` table set defaults for the tasks under
one config root: the tasks its config files define, the files they include, and
the file tasks in its default task directories. For example, the
`[task_config]` in `~/src/myproject/mise.toml` applies to the file task
`~/src/myproject/mise-tasks/build`. Set `cascade = true` to apply the table to
descendant config roots too.

### `task_config.cascade` {#task_config.cascade}

- **Type**: `bool`
- **Default**: `false`

Apply this config's `[task_config]` values to descendant config roots.
Descendant values override individual inherited fields, and a descendant can
set `cascade = false` to stop inheriting the table.

```toml
[task_config]
cascade = true
shell = "bash -c"
```

Cascading applies to `dir`, `shell`, `cache`, `global_env`,
`global_pass_through_env`, `global_inputs`, `input_groups`, `includes`,
`excludes`, and the deprecated `rust_cache`. Inherited include paths and task
inputs stay relative to the config root that defined them.

A descendant's non-empty `global_inputs` replaces the inherited value.
Descendant `input_groups` merge with inherited groups by name, and the nearest
definition wins when a name appears more than once. This also applies to group
references in inherited `global_inputs`. Each group stays relative to the
config root that defined it.

### `task_config.dir` {#task_config.dir}

- **Type**: `string`
- **Default**: <code v-pre>{{ config_root }}</code>

The default [`dir`](#dir) for tasks under this config root. A task's own `dir`,
or one from its template, takes precedence.

```toml
[task_config]
dir = "{{ cwd }}"
```

### `task_config.shell` {#task_config.shell}

- **Type**: `string`
- **Default**: the
  [`unix_default_inline_shell_args`](/configuration/settings.html#unix_default_inline_shell_args)
  or [`windows_default_inline_shell_args`](/configuration/settings.html#windows_default_inline_shell_args)
  setting

The default [`shell`](#shell) for tasks under this config root. A task's own
`shell`, including one inherited from a task template, takes precedence.

```toml
[task_config]
shell = "bash -c"
```

Unlike the global-only inline shell settings, this default applies only to
project tasks. It does not change the interpreter for hooks, tool
installation, or tasks from another config root.

### `task_config.includes` {#task_config.includes}

- **Type**: `string[]`
- **Default**: the
  [default task directories](/tasks/task-discovery.html#default-task-directories)

The TOML task files and task directories to load for this config root. The
list replaces the default task directories rather than adding to them. Entries
can be paths, glob patterns, `git::` URLs, or `oci::` references, and mise
renders them as Tera templates.

```toml
[task_config]
includes = [
  "tasks.toml", # a TOML task file
  "mytasks",    # a directory of task files
]
```

See [Task discovery and precedence](/tasks/task-discovery.html#include-task-files-and-directories)
for which list applies, how to override included tasks, and
[remote git](/tasks/task-discovery.html#remote-git-includes) and
[OCI](/tasks/task-discovery.html#remote-oci-includes) includes.

### `task_config.excludes` {#task_config.excludes}

- **Type**: `string[]`
- **Default**: `[]`

Paths or glob patterns, relative to the config root, that task discovery skips.
Exclusions apply to the default task directories and to paths selected by
`includes`.

```toml
[task_config]
excludes = [".mise/tasks/python/pyproject.toml", ".mise/tasks/generated"]
```

The closest config that sets `excludes` replaces inherited exclusions. See
[exclude paths from discovery](/tasks/task-discovery.html#exclude-paths-from-discovery).

### `task_config.cache` <Badge type="warning" text="experimental" /> {#task_config.cache}

- **Type**: same as [`cache`](/tasks/task-configuration.html#cache)
- **Default**: unset

The default artifact-cache configuration for tasks under this config root.
Only cache-eligible tasks inherit it: tasks with sources and either explicit
output paths or `outputs = []`. A task's own or template `cache`, including
`cache = { enabled = false }`, takes precedence.

```toml
[task_config.cache]
enabled = true
env = ["NODE_ENV", "CI"]
command_inputs = ["node --version"]
```

### `task_config.global_inputs` <Badge type="warning" text="experimental" /> {#task_config.global_inputs}

- **Type**: `string[]`
- **Default**: `[]`

Source paths and glob patterns, relative to the config root, that mise adds to
every task under this config root. Use it for repository-wide configuration and
lockfiles that should invalidate every task without repeating them in each
task's `sources`. Entries can reference an
[input group](#task_config.input_groups) with `@group:<name>`.

```toml
[task_config]
global_inputs = ["mise.toml", ".github/tool-versions", "@group:lockfiles"]

[task_config.input_groups]
lockfiles = ["Cargo.lock", "pnpm-lock.yaml"]
```

A task with no `sources` of its own also receives these inputs and gets
`outputs = { auto = true }`, so mise skips it while the global inputs are
unchanged. Use `mise run --force` to run it anyway.

### `task_config.input_groups` <Badge type="warning" text="experimental" /> {#task_config.input_groups}

- **Type**: `{ [name]: string[] }`
- **Default**: `{}`

Named lists of source patterns that tasks reference from `sources` or
`global_inputs` with `@group:<name>`. Groups can reference other groups;
undefined references and cycles are configuration errors.

```mise-toml
[task_config.input_groups]
toolchain = ["rust-toolchain.toml", "Cargo.lock"]
rust = ["Cargo.toml", "src/**/*.rs", "@group:toolchain"]

[tasks.build]
run = "cargo build"
sources = ["@group:rust"]
outputs = ["target/debug/mycli"]
```

Group entries resolve relative to the config root of the file that defines
them, even when a task sets a different `dir`. Entries written directly in a
task's `sources` stay relative to the task's directory.

### `task_config.global_env` <Badge type="warning" text="experimental" /> {#task_config.global_env}

- **Type**: `string[]`
- **Default**: `[]`

Names of inherited environment variables that mise adds to the cache key of
every task under this config root that has a [`cache`](/tasks/task-configuration.html#cache) configuration.
They add to each task's `cache.env` rather than replacing it.

```toml
[task_config]
global_env = ["CI", "NODE_ENV"]
```

### `task_config.global_pass_through_env` <Badge type="warning" text="experimental" /> {#task_config.global_pass_through_env}

- **Type**: `string[]`
- **Default**: `[]`

Inherited environment variables to keep for every task under this config root
when environment inheritance is denied, without adding their values to cache
keys. They add to each task's [`pass_through_env`](#sandbox).

```toml
[task_config]
global_pass_through_env = ["CI_JOB_TOKEN"]
```

### `task_config.rust_cache` <Badge type="danger" text="deprecated" /> {#task_config.rust_cache}

No longer does anything. Remove it and
[share Cargo builds with Mr Boxington](/lang/rust.html#share-cargo-builds-with-mr-boxington)
instead; see [`rust_cache`](/tasks/task-configuration.html#rust-cache).

## Monorepo tasks {#monorepo-support}

Task names such as `//projects/frontend:build` need a monorepo root: set
`monorepo_root = true` and list the project directories in
`[monorepo].config_roots` in the root `mise.toml`. See
[Monorepo tasks](/tasks/monorepo.html).

## Environment variables mise sets

mise adds variables such as `MISE_TASK_NAME`, `MISE_CONFIG_ROOT`, and
`MISE_TASK_COLOR` to every task's environment, alongside the variables from
[`[env]`](/environments/) and the task's own [`env`](#env). See
[Task environment](/tasks/running-tasks.html#task-environment) for the full
list. When [OpenTelemetry](/tasks/opentelemetry.html) export is on, tasks also
receive their trace context in `TRACEPARENT` and `TRACESTATE`, as described in
[trace propagation](/tasks/opentelemetry.html#trace-propagation).

## Settings

These settings change how tasks run. Set them under `[settings]` in
`~/.config/mise/config.toml`, or per project in `mise.toml` for settings that
are not global-only. See [Settings](/configuration/settings.html) for types,
defaults, and environment variables.

| Setting                                                                                                                                                                                                                                                               | Effect                                                                                |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| [`jobs`](/configuration/settings.html#jobs)                                                                                                                                                                                                                           | How many tasks run in parallel.                                                       |
| [`task.output`](/configuration/settings.html#task.output)                                                                                                                                                                                                             | The default output style.                                                             |
| [`task.quiet`](/configuration/settings.html#task.quiet)                                                                                                                                                                                                               | Hide mise's own output for every task.                                                |
| [`task.timings`](/configuration/settings.html#task.timings)                                                                                                                                                                                                           | Print each task's elapsed time when it finishes.                                      |
| [`task.show_full_cmd`](/configuration/settings.html#task.show_full_cmd)                                                                                                                                                                                               | Print full command lines instead of truncating them.                                  |
| [`task.timeout`](/configuration/settings.html#task.timeout)                                                                                                                                                                                                           | A time limit for a whole `mise run`.                                                  |
| [`task.skip`](/configuration/settings.html#task.skip)                                                                                                                                                                                                                 | Tasks that `mise run` skips.                                                          |
| [`task.skip_depends`](/configuration/settings.html#task.skip_depends)                                                                                                                                                                                                 | Run only the named tasks, without their dependencies.                                 |
| [`task.run_auto_install`](/configuration/settings.html#task.run_auto_install)                                                                                                                                                                                         | Install missing tools before tasks run.                                               |
| [`task.source_freshness_hash_contents`](/configuration/settings.html#task.source_freshness_hash_contents)                                                                                                                                                             | Compare source contents instead of timestamps.                                        |
| [`task.source_freshness_equal_mtime_is_fresh`](/configuration/settings.html#task.source_freshness_equal_mtime_is_fresh)                                                                                                                                               | Treat equal source and output timestamps as up to date.                               |
| [`task.disable_paths`](/configuration/settings.html#task.disable_paths)                                                                                                                                                                                               | Paths mise does not search for tasks.                                                 |
| [`task.remote_no_cache`](/configuration/settings.html#task.remote_no_cache)                                                                                                                                                                                           | Fetch remote task files and includes on every run.                                    |
| [`task.disable_spec_from_run_scripts`](/configuration/settings.html#task.disable_spec_from_run_scripts)                                                                                                                                                               | Take arguments only from `usage`, not from `run` scripts.                             |
| [`raw`](/configuration/settings.html#raw)                                                                                                                                                                                                                             | Connect every task to the terminal, as `mise run --raw` does.                         |
| [`unix_default_inline_shell_args`](/configuration/settings.html#unix_default_inline_shell_args), [`windows_default_inline_shell_args`](/configuration/settings.html#windows_default_inline_shell_args)                                                                | The shell for `run` scripts.                                                          |
| [`unix_default_file_shell_args`](/configuration/settings.html#unix_default_file_shell_args), [`windows_default_file_shell_args`](/configuration/settings.html#windows_default_file_shell_args)                                                                        | The shell for file tasks that mise starts itself.                                     |
| [`use_file_shell_for_executable_tasks`](/configuration/settings.html#use_file_shell_for_executable_tasks)                                                                                                                                                             | Start executable file tasks through a shell instead of running them directly.         |
| [`windows_executable_extensions`](/configuration/settings.html#windows_executable_extensions)                                                                                                                                                                         | File extensions that make a file a task on Windows.                                   |
| [`windows_powershell_no_profile`](/configuration/settings.html#windows_powershell_no_profile)                                                                                                                                                                         | Skip PowerShell profiles when mise starts PowerShell.                                 |
| [`task.cache_dir`](/configuration/settings.html#task.cache_dir), [`task.cache_max_age`](/configuration/settings.html#task.cache_max_age), [`task.cache_max_size`](/configuration/settings.html#task.cache_max_size)                                                   | Where the artifact cache lives and how long it keeps entries (experimental).          |
| [`task.cache.remote_url`](/configuration/settings.html#task.cache.remote_url) and the other `task.cache.*` settings                                                                                                                                                   | The [remote task cache](/tasks/remote-cache.html) (experimental).                     |
| [`task.monorepo_depth`](/configuration/settings.html#task.monorepo_depth), [`task.monorepo_exclude_dirs`](/configuration/settings.html#task.monorepo_exclude_dirs), [`task.monorepo_respect_gitignore`](/configuration/settings.html#task.monorepo_respect_gitignore) | Deprecated automatic monorepo discovery, used only without `[monorepo].config_roots`. |
| [`task.auto_infer`](/configuration/settings.html#task.auto_infer)                                                                                                                                                                                                     | Workspace providers that mise infers tasks from (experimental).                       |
| [`otel.enabled`](/configuration/settings.html#otel.enabled), [`otel.logs`](/configuration/settings.html#otel.logs)                                                                                                                                                    | [OpenTelemetry](/tasks/opentelemetry.html) export (experimental).                     |
