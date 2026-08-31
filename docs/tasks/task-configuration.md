# Task Configuration

This is an exhaustive list of all the configuration options available for tasks in `mise.toml` or
as file tasks.

## Task properties

All examples are in toml-task format instead of file, however they apply in both except where otherwise noted.

### `run`

- **Type**: `string | (string | { task: string, args?: string[], env?: { [key]: string } } | { tasks: string[] })[]`

The command(s) to run. This is the only required property for a task.

You can mix scripts with task references, and pass optional `args` and `env` to referenced tasks:

```mise-toml
[tasks.grouped]
run = [
  { task = "t1" },          # run t1 (with its dependencies)
  { task = "build", args = ["--release"], env = { RUSTFLAGS = "-C opt-level=3" } },
  { tasks = ["t2", "t3"] }, # run t2 and t3 in parallel (with their dependencies)
  "echo end",               # then run a script
]
```

`{ task }` and `{ tasks }` are execution steps for this task, not
[`depends`](#depends). They still run with their own dependencies.
`mise tasks deps` does not include them as graph edges.
See [`mise tasks deps`](/cli/tasks/deps.html).

Simple forms still work and are equivalent:

```mise-toml
tasks.a = "echo hello"
tasks.b = ["echo hello"]
tasks.c.run = "echo hello"
[tasks.d]
run = "echo hello"
[tasks.e]
run = ["echo hello"]
```

### `run_windows`

- **Type**: `string | (string | { task: string, args?: string[], env?: { [key]: string } } | { tasks: string[] })[]`

Windows-specific variant of `run` supporting the same structured syntax:

```mise-toml
[tasks.build]
run = "cargo build"
run_windows = "cargo build --features windows"
```

### `file`

- **Type**: `string`

Execute an external script instead of an inline `run` command. Relative paths are resolved from the
directory containing the task's config file. The path supports Tera templates.

```mise-toml
[tasks.release]
description = "Cut a new release"
file = "scripts/release.sh"
```

`file` also accepts HTTP(S) URLs and `git::` sources. See [Using a file or remote
script](/tasks/toml-tasks.html#using-a-file-or-remote-script) for the supported formats and security
considerations.

### `description`

- **Type**: `string`

A description of the task. This is used in (among other places)
the help output, completions, `mise run` (without arguments), and `mise tasks`.

```mise-toml
[tasks.build]
description = "Build the CLI"
run = "cargo build"
```

### `alias`

- **Type**: `string | string[]`

An alias for the task so you can run it with `mise run <alias>` instead of the full task name.

```mise-toml
[tasks.build]
alias = "b" # run with `mise run b`
run = "cargo build"
```

### `depends`

- **Type**: `string | (string | string[] | { task: string, args?: string[], env?: { [key]: string }, optional?: bool })[]`

Tasks that must be run before this task. This is a list of task names or aliases. Arguments can be
passed to the task, e.g.: `depends = ["build --release"]`. If multiple tasks have the same dependency,
that dependency will only be run once. mise will run whatever it can in parallel (up to [`--jobs`](/cli/run))
through the use of `depends` and related properties.

[`mise tasks deps`](/cli/tasks/deps.html) visualizes this declared graph
(`depends`, `wait_for`, `depends_post`), not task references inside `run`.

```mise-toml
[tasks.build]
run = "cargo build"
[tasks.test]
depends = ["build"]
run = "cargo test"
```

#### Passing environment variables to dependencies

You can pass environment variables to specific dependencies using two syntaxes:

**Shell-style inline:**

```mise-toml
[tasks.test]
depends = ["NODE_ENV=test setup"]
run = "npm test"

[tasks.setup]
run = 'echo "Setting up for $NODE_ENV"'
```

**Structured object format:**

```mise-toml
[tasks.test]
depends = [
  { task = "setup", env = { NODE_ENV = "test", DEBUG = "true" } }
]
run = "npm test"
```

The structured format also supports combining env vars with arguments:

```mise-toml
[tasks.deploy]
depends = [
  { task = "build", args = ["--release"],
    env = { RUSTFLAGS = "-C opt-level=3" } }
]
run = "./deploy.sh"
```

String and structured dependencies can be mixed in the same array:

```mise-toml
[tasks.check]
depends = [
  "lint",
  { task = "test", env = { CI = "true" } },
]
run = "echo checks complete"
```

Note: These environment variables are passed only to the specified dependency, not to the current task or other dependencies.

#### Optional dependencies

Set `optional = true` on a structured dependency to run matching tasks when they exist without
failing when the task name or pattern has no matches. Invalid task patterns still produce an error.

```mise-toml
[tasks.test]
depends = [
  { task = "//...:test", optional = true },
  { task = "//...:test:*", optional = true },
]
```

#### Passing parent task arguments to dependencies

You can forward a parent task's arguments to its dependencies using <span v-pre>`{{usage.*}}`</span> templates.
Both the parent and child tasks must define a `usage` spec for the arguments they accept:

```mise-toml
[tasks.build]
usage = 'arg "<app>"'
run = 'echo "building {{usage.app}}"'

[tasks.deploy]
usage = 'arg "<app>"'
depends = [{ task = "build", args = ["{{usage.app}}"] }]
run = 'echo "deploying {{usage.app}}"'
```

Running `mise run deploy myapp` passes `"myapp"` to both `deploy` and its `build` dependency.

This also works with the string syntax:

```mise-toml
[tasks.deploy]
usage = 'arg "<app>"'
depends = ["build {{usage.app}}"]
run = 'echo "deploying {{usage.app}}"'
```

And with flags:

```mise-toml
[tasks.compile]
usage = 'flag "--target <target>"'
run = 'echo "compiling for $usage_target"'

[tasks.package]
usage = 'flag "--target <target>"'
depends = [{ task = "compile", args = ["--target", "{{usage.target}}"] }]
run = 'echo "packaging for $usage_target"'
```

Arguments flow through dependency chains — if A depends on B which depends on C, each task can
forward its resolved arguments to its own dependencies.

### `depends_post`

- **Type**: `string | (string | string[] | { task: string, args?: string[], env?: { [key]: string }, optional?: bool })[]`

Like `depends` but these tasks run _after_ this task and its dependencies complete. For example, you
may want a `postlint` task that you can run individually without also running `lint`:

```mise-toml
[tasks.lint]
run = "eslint ."
depends_post = ["postlint"]
[tasks.postlint]
run = "echo 'linting complete'"
```

Supports the same argument, environment variable, and optional dependency syntax as `depends`.
Dependencies of a `depends_post` task also wait until the parent task finishes, so an entire cleanup
chain runs after the main work. Mise runs the full subtree if the parent started, even when the
parent fails, but skips it when a regular dependency fails before the parent can start. The same
task may be referenced by both `depends` and `depends_post`; in that case it runs once before the
parent and once afterward.

### `wait_for`

- **Type**: `string | (string | string[] | { task: string, args?: string[], env?: { [key]: string }, optional?: bool })[]`

Similar to `depends`, it will wait for these tasks to complete before running. Unlike `depends`,
`wait_for` does not add matching tasks to the run; it only waits for them when they are already
scheduled. To allow a task name or pattern to have no configured matches, use `optional = true`.

```mise-toml
[tasks.lint]
wait_for = ["render"] # creates some js files, so if it's running, wait for it to finish
run = "eslint ."
```

Supports the same argument, environment variable, and optional dependency syntax as `depends`.

`wait_for` matches tasks differently depending on whether args or env vars are specified:

- `wait_for = ["setup"]` — matches by name, regardless of args or env overrides. If another task runs `depends = ["DEBUG=1 setup"]`, this will still match and wait for it.
- `wait_for = ["setup arg1"]` or `wait_for = ["DEBUG=1 setup"]` — matches only tasks running with that exact args/env configuration.

### `env`

- **Type**: `{ [key]: string | int | bool }`

Environment variables specific to this task. These will not be passed to `depends` tasks.

```mise-toml
[tasks.test]
env.TEST_ENV_VAR = "ABC"
run = [
    "echo $TEST_ENV_VAR",
    "mise run some-other-task", # running tasks like this _will_ have TEST_ENV_VAR set of course
]
```

### `vars` {#task-vars}

- **Type**: `{ [key]: string | int | bool | directive }`

Variables specific to this task. Task-local vars override config vars while rendering the task, but
are not exported to the task process as environment variables.

```mise-toml
[vars]
mode = "headless"

[tasks.test]
vars = { mode = "headed" }
run = "./scripts/test-e2e.sh --{{ vars.mode }}"
```

See [Vars](#vars) for supported value-producing directives, precedence, and redaction.

### `tools`

- **Type**: `{ [key]: string }`

Tools to install and activate before running the task. This is useful for tasks that require a specific tool to be
installed or a tool with a different version. It will only be used for that task, not dependencies.

```mise-toml
[tasks.build]
tools.rust = "1.50.0"
run = "cargo build"
```

Run [`mise lock`](/dev-tools/mise-lock.html) to resolve task-specific tools into the owning
config's lockfile before running the task. This reads the task definition without executing the
task or installing its tools.

Run `mise install --include-task-tools` to install tools for every task in the current scope without
executing task commands or dependencies. This is useful for preparing CI caches or container images;
combine it with `--monorepo` to include every configured monorepo root.

### `dir`

- **Type**: `string`
- **Default**: <code v-pre>"{{ config_root }}"</code> - the directory containing `mise.toml`, or in the case of something like `~/src/myproj/.config/mise.toml`, it will be `~/src/myproj`.

The directory to run the task from. The most common way this is used is when you want the task to execute
in the user's current directory:

```mise-toml
[tasks.test]
dir = "{{cwd}}"
run = "cargo test"
```

### `hide`

- **Type**: `bool`
- **Default**: `false`

Hide the task from help, completion, and other output like `mise tasks`. Useful for deprecated or internal
tasks you don't want others to easily see.

```mise-toml
[tasks.internal]
hide = true
run = "echo my internal task"
```

### `confirm`

- **Type**: `string` | `{ message: string, default: string }`

A message to show before running the task. This is useful for tasks that are destructive or take a long
time to run. The user will be prompted to confirm before the task's own `run` command executes.

::: warning
`confirm` only guards the task's own `run` command. Dependencies (`depends`) will execute **before** the confirmation prompt appears. If you need confirmation before dependencies run, add `confirm` to the dependency tasks themselves, or use `run = [{ task = "..." }]` instead of `depends`.
:::

```mise-toml
[tasks.release]
confirm = { message = "Are you sure you want to cut a release?", default = "no" }
description = 'Cut a new release'
file = 'scripts/release.sh'
```

The confirm message supports Tera templates and can reference usage arguments:

```mise-toml
[tasks.deploy]
usage = '''
arg "<environment>" help="Environment to deploy to"
flag "--force" help="Force deployment"
'''
confirm = "Deploy to {{ usage.environment }}?{% if usage.force %} (forced){% endif %}"
run = "deploy.sh ${usage_environment}"
```

### `raw`

- **Type**: `bool`
- **Default**: `false`

Connects the task directly to the shell's stdin/stdout/stderr. This is useful for tasks that need to
accept input or output in a way that mise's normal task handling doesn't support.

A raw command holds an exclusive lock for as long as it runs, so mise will not run another command
alongside it and you do not have to keep other tasks out of the way yourself. The lock is taken per
command rather than per task, so two raw tasks can still take turns between their individual
commands. If you need a whole task to run without interruption, search/file a ticket for a property
like `single = true`.

### `raw_args`

- **Type**: `bool`
- **Default**: `false`

When `true`, mise does not parse arguments to the task at all — every argument
is passed through verbatim to the underlying command, including `--help`/`-h`.
Use this for tasks that act as a thin proxy for a tool which already has its
own argument parser (e.g. `next build`, Django `manage.py`, Python scripts
using `argparse`):

```toml
[tasks.manage]
raw_args = true
run = 'python manage.py'
```

```sh
mise run manage --help          # forwarded to manage.py, not intercepted by mise
mise run manage migrate --fake  # all flags reach manage.py unchanged
```

Without `raw_args`, mise intercepts `--help` and prints its own task help. As
an ad-hoc alternative for individual invocations, you can also use
`mise run task -- --help` — the `--` separator now bypasses mise's usage
parser specifically for `--help`/`-h`. Arguments after that separator belong
to the task, so `mise run task -- -- --help` forwards `-- --help` to the task.

### `interactive`

- **Type**: `bool`
- **Default**: `false`

Connects the task directly to the shell's stdin/stdout/stderr. Interactive tasks acquire an exclusive lock,
ensuring sole access to standard I/O — while an interactive task is running, all other tasks (both interactive
and non-interactive) are blocked. Non-interactive tasks can still run in parallel with each other. This is more
targeted than the broad `raw` setting which forces single-threaded execution globally (by setting `jobs = 1`).

### `sources`

- **Type**: `string | string[]`

Files or directories that this task uses as input, if this and `outputs` is defined, mise will skip
executing tasks where the modification time of the oldest output file is newer than the modification
time of the newest source file. This is useful for tasks that are expensive to run and only need to
be run when their inputs change.

The task itself will be automatically added as a source, so if you edit the definition that will also
cause the task to be run.

This is also used in `mise watch` to know which files/directories to watch.

This can be specified with relative paths and/or with glob patterns, e.g.: `src/**/*.rs`. Brace
alternatives such as `src/**/*.{js,ts}` are supported by freshness checks, `mise watch`, and
`task_source_files()`.
Ensure you don't go crazy with adding a ton of files in a glob though—mise has to scan each and every one to check
the timestamp.

```mise-toml
[tasks.build]
run = "cargo build"
sources = ["Cargo.toml", "src/**/*.rs"]
outputs = ["target/debug/mycli"]
```

Running the above will only execute `cargo build` if `mise.toml`, `Cargo.toml`, or any ".rs" file in the `src` directory
has changed since the last build.

Relative entries are resolved from the task directory (the task's `dir`, or the project root when it
has none) and may use `..` to reach files above it, such as a `node_modules` directory shared at the
root of a monorepo:

```mise-toml
[tasks.build]
dir = "packages/web"
run = "npm run build"
sources = ["src/**/*.ts", "../../node_modules/**"]
outputs = ["dist"]
```

The [`task_source_files`](../templates.md#task-source-files) function can be used to iterate over a task's
`sources` within its template context.

#### Excluding sources

Entries in `sources` prefixed with `!` are excluded, matching the convention
used by gitignore, watchexec, and rsync. Exclusions affect the freshness
check, the `task_source_files` template function, and which files
`mise watch` watches for changes.

```mise-toml
[tasks.build]
sources = ["src/**/*.ts", "!src/**/*.test.ts", "!src/**/*.spec.ts", "tsconfig.json"]
run = "npm run build"
```

Entries are evaluated in order, and the latest matching entry wins. A later
non-negated entry can re-include a file an earlier `!` excluded — for example,
`["src/**/*.ts", "!src/**/*.test.ts", "src/keep.test.ts"]` excludes all
`*.test.ts` files except `src/keep.test.ts`.

To include a literal path that begins with `!`, escape the prefix as `\!`
(e.g. `"\\!important.txt"` in TOML).

#### Reusable and global inputs <Badge type="warning" text="experimental" />

Use `[task_config.input_groups]` to define source patterns once and reuse them across tasks. Reference
a group from `sources` with `@group:<name>`. Groups can reference other groups; undefined references
and cycles are configuration errors.

Group entries are resolved relative to the config file that defines them, even when a task uses a
different `dir`. Ordinary entries written directly in `sources` remain relative to the task directory.

```mise-toml
[settings]
experimental = true

[task_config.input_groups]
toolchain = ["rust-toolchain.toml", "Cargo.lock"]
rust = ["Cargo.toml", "src/**/*.rs", "@group:toolchain"]

[tasks.build]
run = "cargo build"
sources = ["@group:rust"]
outputs = ["target/debug/mycli"]

[tasks.test]
run = "cargo test"
sources = ["@group:rust"]
outputs = []
```

`task_config.global_inputs` adds source patterns to every task in the config scope. This is useful
for repository-wide configuration and lockfiles that should invalidate all cacheable tasks without
being repeated in each task's `sources`. Global inputs may also reference named groups.

```mise-toml
[task_config]
global_inputs = ["mise.toml", ".github/tool-versions", "@group:lockfiles"]

[task_config.input_groups]
lockfiles = ["Cargo.lock", "pnpm-lock.yaml"]
```

#### Dependency invalidation

When a task depends on another task that also has `sources` defined, and the dependency runs because
its sources changed, the dependent task will also re-run — even if the dependent's own sources haven't
changed. This is useful for monorepo workflows where downstream tasks should be invalidated by upstream
changes:

```mise-toml
[tasks."core:build"]
run = "tsc -p packages/core"
sources = ["packages/core/src/**/*.ts"]
outputs = ["packages/core/dist/**/*.js"]

[tasks."frontend:build"]
run = "tsc -p packages/frontend"
sources = ["packages/frontend/src/**/*.ts"]
outputs = ["packages/frontend/dist/**/*.js"]
depends = ["core:build"]
```

If a file in `packages/core/src/` changes, both `core:build` and `frontend:build` will run. If nothing
changes, both are skipped.

Note that dependencies **without** `sources` (which always run) do not trigger this invalidation —
otherwise `sources` on the dependent task would be effectively useless.

### `watch`

- **Type**: `{ no_vcs_ignore = bool }`
- **Default**: `{ no_vcs_ignore = false }`

Options used when the task runs through [`mise watch`](/cli/watch.html). By default, `mise watch`
respects VCS ignore files such as `.gitignore`, even when an ignored path is listed in `sources`. Set
`watch.no_vcs_ignore` for tasks that need to watch generated or intermediary files which are
intentionally excluded from version control:

```mise-toml
[tasks.generate]
run = "process generated/output.json"
sources = ["generated/output.json"]
watch = { no_vcs_ignore = true }
```

This is equivalent to passing `--no-vcs-ignore` to watchexec. Because watchexec applies ignore
options to the entire watch process, watching multiple tasks together disables VCS ignores for all
of them if any selected task enables this option. Keep `sources` narrowly scoped: disabling VCS
ignores for broad build, distribution, or dependency directories may substantially increase
filesystem scanning.

### `outputs`

- **Type**: `string | string[] | { auto = true }`
- **Default**: `{ auto = true }`

The counterpart to `sources`, these are the files or directories that the task will create/modify after
it executes.

Entries prefixed with `!` exclude matching outputs. As with `sources`, entries
are evaluated in order, a later entry can re-include a path, and `\!` escapes a
literal leading bang. Output globs also support brace alternatives such as
`dist/{client,server}/**`.

```mise-toml
[tasks.build]
run = "npm run build"
sources = ["src/**"]
outputs = ["dist", "!dist/**/*.map", "!dist/.vite/**"]
```

Excluded files do not participate in output freshness checks and are not
stored in task-cache artifacts. If excluded files already exist beneath an
output directory when a cached artifact is restored, mise preserves them.

`auto = true` is an alternative to specifying output files manually. In that case, mise will touch
an internally tracked file based on the hash of the task definition (stored in `~/.local/state/mise/task-outputs/<hash>` if you're curious).
This is useful if you want `mise run` to execute when sources change but don't want to have to manually `touch`
a file for `sources` to work.

```mise-toml
[tasks.build]
run = "cargo build"
sources = ["Cargo.toml", "src/**/*.rs"]
outputs = { auto = true } # this is the default when sources is defined
```

### `cache` <Badge type="warning" text="experimental" />

- **Type**: `{ enabled = bool, audit = bool, env = string[], command_inputs = string[] }`
- **Default**: `{ enabled = false, audit = false, env = [], command_inputs = [] }`

Stores successful task results in a content-addressed local cache and reuses them when the same task
inputs are seen again. Declared filesystem outputs are restored after deletion. Tasks with
`outputs = []` cache their successful result and logs without storing filesystem artifacts, which is
useful for checks such as linting, testing, and type checking.
Declaring `outputs = []` asserts that the task has no filesystem side effects that a cache hit needs
to reproduce.

Artifact caching requires [`experimental`](/configuration/settings.html#experimental), at least one
matching `source`, and either explicit output paths or `outputs = []`.
`outputs = { auto = true }`, absolute outputs, and output patterns (including
the body of an exclusion) that escape the task directory are not supported.

```mise-toml
[settings]
experimental = true

[tasks.build]
run = "npm run build"
sources = ["package.json", "src/**"]
outputs = ["dist"]
cache = { enabled = true, env = ["NODE_ENV"] }
```

Commands listed in `cache.command_inputs` run before cache lookup. Their command text, stdout, and
stderr are included in the cache key. Commands use the same inline shell (including a CLI `--shell`
override), resolved environment and tools, working directory, and sandbox policy as the task. This
is useful when inputs such as compiler versions or generated configuration cannot be represented by
source files alone.

```mise-toml
[tasks.build]
run = "npm run build"
sources = ["package.json", "src/**"]
outputs = ["dist"]
cache = { enabled = true, command_inputs = ["node --version", "npm config get registry"] }
```

A command input must be non-empty and exit successfully. Its output is hashed without being printed
or retained. Command inputs inherit the task timeout, or have a 30-second timeout when the task has
none, and may emit at most 16 MiB across stdout and stderr. They should be fast, deterministic, and
free of side effects because they run whenever mise computes the task's cache key. Command inputs
are not run during dry runs or when caching is disabled for raw or interactive execution.

Set `cache.audit = true` to diagnose incomplete cache declarations on Linux. When a task executes,
mise uses `strace` to report reads beneath the workspace root that do not match `sources` and writes
beneath the task directory that do not match `outputs`. The audit is advisory and does not block the
task or prevent a successful result from being cached. Access outside those roots and directory
metadata reads are ignored to keep system libraries, executables, and path traversal out of the
report.

Reported paths are always relative to the task directory, using `..` for the paths above it that a
read may legitimately touch. A reported read can be added to `sources` exactly as it was printed.

Audit mode requires `strace` on `PATH`. Mise warns and runs the task normally when tracing is not
available; other platforms are not currently supported. Cached tasks are not executed and therefore
produce no audit report, so use `mise run --force <task>` when checking an existing cache entry.

Console warnings are limited to the first 20 paths per task, which is not enough to classify a task
that reads thousands of undeclared files. Set
[`task.cache.audit_report`](/configuration/settings.html#task.cache.audit_report) to also write every
undeclared path as JSON Lines, one `{"task", "kind", "path"}` object per entry. Truncation happens
once per `mise` invocation: the first audited task in each invocation truncates the file and later
audited tasks in that invocation append to it, so one file holds that run's report for every audited
task and a later run replaces it rather than adding to it.

```shell
MISE_TASK_CACHE_AUDIT_REPORT=audit.jsonl mise run --force build
```

```mise-toml
[tasks.build]
run = "npm run build"
sources = ["package.json", "src/**"]
outputs = ["dist"]
cache = { enabled = true, audit = true }
```

#### External dependencies and lockfiles

Declare dependency manifests and lockfiles as filesystem inputs so dependency updates invalidate the
cache. They can be listed directly in a task's `sources`, shared through an input group, or applied to
every task in a config scope with `task_config.global_inputs`.

```mise-toml
[settings]
experimental = true

[task_config]
global_inputs = ["@group:node-dependencies"]

[task_config.input_groups]
node-dependencies = ["package.json", "pnpm-lock.yaml"]

[tasks.build]
run = "pnpm build"
sources = ["src/**"]
outputs = ["dist"]
cache = { enabled = true }
```

The lockfile content represents the resolved external dependency graph, so installed dependency
directories such as `node_modules` generally should not be included. Resolved mise tools already
participate in the cache key. Use `cache.command_inputs` for relevant external state that is not
captured in committed files, such as a package registry selection or a compiler wrapper version:

```mise-toml
[tasks.build]
run = "pnpm build"
sources = ["package.json", "pnpm-lock.yaml", "src/**"]
outputs = ["dist"]
cache = { enabled = true, command_inputs = ["pnpm config get registry"] }
```

Only declare deterministic external state that can affect task outputs. Secrets and credentials
should use pass-through environment variables instead so their values are not included in cache
keys.

#### Per-run cache access

Use `mise run --task-cache <mode>` or `MISE_TASK_CACHE` to control task output cache reads and writes
for one run:

- `read-write` uses cached results and publishes new results. This is the default.
- `read-only` uses cached results but does not publish misses.
- `write-only` publishes results but always executes instead of restoring.
- `off` disables task output caching and uses ordinary source/output freshness checks.
- `local-only` reads and writes only the local cache, bypassing any configured remote service.

```bash
# Prevent an untrusted pull request from publishing cache entries
mise run --task-cache read-only test

# Warm the local cache without consuming existing entries
mise run --task-cache write-only build

# Diagnose a task without reading or writing task output artifacts
mise run --task-cache off build
```

These modes only affect the experimental task output cache configured by a task's `cache` property.
The existing `--no-cache` option controls fetching remote task definitions instead.

#### Remote cache and sensitive data

Configure the experimental remote build-cache service with `task.cache.remote_url` and a non-empty
`task.cache.remote_namespace`. The namespace is an opaque repository or organization identifier;
the server must isolate entries by both namespace and cache key. It is routing metadata, not an
authentication mechanism or secret. Use a distinct namespace wherever writers should not be able to
influence one another's cache entries.

```mise-toml
[settings]
experimental = true
task.cache.remote_url = "https://cache.example.com/mise/"
task.cache.remote_namespace = "acme/widgets"
task.cache.remote_mode = "read-write"
```

Set `MISE_TASK_CACHE_REMOTE_TOKEN` in the process environment to send a bearer credential. The
equivalent `task.cache.remote_token` setting is global-only, but the environment variable is
preferred so a token does not need to be written to disk. mise redacts the token from settings trace
output and marks its HTTP header as sensitive. It requires HTTPS for non-loopback services; plain
HTTP is accepted only for local development servers. Servers should still use short-lived,
least-privilege credentials, restrict namespace access, avoid logging authorization headers, and
encrypt or otherwise protect stored cache objects according to their sensitivity and retention
requirements.

For rotating credentials, set `MISE_TASK_CACHE_REMOTE_TOKEN_FILE` to a file containing only the
bearer token. mise rereads the file before every request, which supports Kubernetes-projected
service account tokens without restarting a long-running process. The equivalent
`task.cache.remote_token_file` setting is global-only.

In GitHub Actions, mise can acquire and refresh a short-lived OIDC token itself. Grant the workflow
permission to request an identity token and set its audience explicitly:

```yaml
permissions:
  contents: read
  id-token: write

jobs:
  test:
    runs-on: ubuntu-latest
    env:
      MISE_TASK_CACHE_REMOTE_OIDC_AUDIENCE: https://cache.example.com
    steps:
      - uses: actions/checkout@v5
      - run: mise run test
```

The cache service must trust GitHub's issuer, accept the configured audience, and authorize the
workflow's identity claims for the selected namespace. mise obtains the token from GitHub's job
OIDC endpoint, keeps it only in memory, and refreshes it before expiry. The audience setting is
global-only and acquisition fails clearly when the workflow lacks `id-token: write` permission.

Credential precedence is explicit token, token file, then automatic OIDC. This lets an emergency
static credential override workload identity without changing project configuration. Other CI
providers can supply their issued OIDC token directly through `MISE_TASK_CACHE_REMOTE_TOKEN`; they
do not need a protocol-specific integration.

Task cache entries are not secret-free metadata. They contain captured stdout and stderr plus every
declared output file. mise applies its configured output redactions before storing logs, but this is
not a general secret scanner: a task can print an unknown credential or write one into an output
artifact. Do not cache such a task unless those values are safe to retain and share with every
reader of its local and remote cache. Clearing a local entry does not delete copies already uploaded
to a remote service; use the remote service's retention and deletion controls as well.

Artifact checksums detect corruption and HTTPS authenticates the configured server in transit, but
a checksum is not a signature from the original task runner. Any principal allowed to write a
namespace can publish entries that its readers will trust. Give untrusted pull-request jobs
read-only credentials or no remote credentials, use `--task-cache read-only` to prevent publishing,
and isolate less-trusted writers in a separate namespace.

#### Cache correctness and deterministic tasks

Enabling `cache` is a correctness assertion: identical cache-key material must produce equivalent
captured logs and declared outputs. Every value that can change the result must be represented by a
source or input group, a resolved mise tool, `cache.env`, `cache.command_inputs`, or a cacheable
dependency's artifact key. This includes configuration and lockfiles, locale or feature flags,
compiler wrappers, generated inputs, and relevant external service state. Operating system and
architecture are included automatically; other machine state is not.

Cache-enabled tasks should be deterministic and should not depend on undeclared files, wall-clock
time, randomness, mutable network responses, or ambient environment variables. If such an input
cannot be captured reliably, disable caching for the task. Pass-through environment variables are
intentionally absent from the key and therefore must not influence cached logs or outputs. A task
that uses credentials only to fetch content must key on a stable digest or lockfile for that content,
not on the credential itself.

Declared outputs must completely describe the filesystem state that a hit needs to reproduce. Side
effects outside those paths—database writes, deployments, notifications, and changes elsewhere in
the workspace—are not replayed. `outputs = []` is only correct when no filesystem side effect needs
to be reproduced. On Linux, `cache.audit = true` can reveal many undeclared workspace reads and
writes, but the audit is advisory and cannot prove determinism or observe every external dependency.

When correctness is uncertain, use `--task-cache off` while diagnosing, add missing key inputs, and
force an uncached execution before trusting new entries. Use separate remote namespaces when a
change to task semantics or undeclared external state could otherwise collide with entries produced
under a different trust policy.

```mise-toml
[tasks.lint]
run = "eslint ."
sources = ["package.json", "src/**"]
outputs = []
cache = { enabled = true }
```

To enable caching by default for every eligible task in a config scope, set
`task_config.cache`. Only tasks with at least one source and either explicit output paths or
`outputs = []` inherit this default; other tasks remain uncached. A task-local `cache` value
overrides the scoped default.

```mise-toml
[settings]
experimental = true

[task_config.cache]
enabled = true
env = ["NODE_ENV"]
command_inputs = ["node --version"]

[tasks.build]
run = "npm run build"
sources = ["package.json", "src/**"]
outputs = ["dist"]

[tasks.deploy]
run = "./deploy.sh"
cache = { enabled = false }
```

The cache key includes source contents, the task definition and arguments, resolved task environment,
the values (or absence) of variables named in `cache.env`, command-input output, resolved tool
versions, dependency artifact keys, and the operating system and architecture. Variables inherited
from the ambient process are ignored unless listed in `cache.env`.

Use `mise run --task-cache-explain <task>` to print a deterministic breakdown of the inputs that
produced the cache key without printing the aggregate key itself. Environment variables are
identified only by name and whether they are set, while mise variables are identified only by name,
so the explanation does not publish their contents or per-value digests. Other potentially
secret-derived inputs—including source contents, dependency keys, command output, task definitions,
and resolved tool versions—are reported only by category and count. Matched source paths, declared
output patterns, currently resolved output roots, and the target platform are listed directly.

Combine the flag with `--dry-run` to inspect the key inputs without executing, restoring, or storing
the task. Cache command inputs still run when the explanation is explicitly requested because their
output hashes are part of the key.

Use `mise run --dry-run --task-cache-explain-json <task>` for machine-readable diagnostics. The
command writes one compact JSON object per selected task to stdout, using the same redaction rules
as the human explanation. Each object includes the opaque `cache_key` so consumers can distinguish
separate invocations of the same task without exposing their arguments or dependency environment
values. This JSON Lines format remains streamable when a pattern selects multiple tasks. Cache
command inputs still run so their presence can be reported accurately, but their output and hashes
are not included.

Use `mise run --task-cache-stats <task>` to print a run summary with the number and percentage of
artifact cache hits, the uncompressed output and log bytes restored, and the execution time recorded
when each restored entry was created. Entries written before this metadata was added remain readable
and contribute zero bytes and time when restored. Freshness skips that do not perform a cache lookup
are not counted as hits or misses.

Use `mise cache task <task>` to inspect every local output-cache entry associated with a configured
task. The table shows each key, whether it is the current freshness entry, its stored and restorable
sizes, recorded execution time, last access time, and output roots. Add `--json` for structured
output as an array, including when only one task matches. Entries created before task identity
metadata was added can be inspected when they are the task's current entry; older historical entries
become discoverable after they are rewritten.

Use `mise cache clear --task <task>` to delete only that task's local output-cache entries and
freshness pointer. Declared outputs in the working directory and entries belonging to other tasks
are not removed. Legacy current entries without identity metadata are detached but retained because
their ownership cannot be verified; mise warns when this occurs, and `mise cache clear` removes them.

`task_config.global_env` adds ambient variable names to every enabled task cache in the config
scope, including tasks with a task-local `cache` value. Unlike the default values under
`task_config.cache`, these names always compose with task-local `cache.env`.

```mise-toml
[task_config]
global_env = ["CI", "NODE_ENV"]
```

For cache-enabled tasks, variables named in `cache.env` or `task_config.global_env` remain available
when environment inheritance is denied. Disabled and non-cache tasks do not inherit variables
through cache configuration. Use `pass_through_env` for variables that a task needs at runtime but
which must not affect its cache key, such as short-lived credentials. The scoped
`task_config.global_pass_through_env` equivalent applies to every task. In mise's default,
non-sandboxed environment mode, ambient variables already pass through; these options matter when
environment sandboxing is active through `allow_env`, `deny_env`, `deny_all`, or a corresponding
CLI option.

```mise-toml
[task_config]
global_pass_through_env = ["CI_JOB_TOKEN"]

[tasks.build]
pass_through_env = ["NPM_TOKEN"]
```

Pass-through variables can change task behavior without invalidating cached results. Tasks should
not use them for values that affect generated outputs. Their values are not added to the key or
persisted as cache metadata, but a task can still expose them by writing them to cached output files
or logs.

Cache entries are stored under `MISE_CACHE_DIR/task-artifacts/v2` by default. Set the experimental
[`task.cache_dir`](/configuration/settings.html#task.cache_dir) setting or
`MISE_TASK_CACHE_DIR` to choose a different parent directory; mise keeps the artifact format in its
`v2` child directory. Default and custom locations are included in `mise cache clear` and
manual and automatic cache pruning. Only successful task runs are cached. Cache read/write failures
are treated as misses and never turn a successful task run into a failure.

New cache entries include a BLAKE3 artifact checksum that is independent of the cache lookup key.
It covers the archived outputs and captured task result metadata, and mise verifies it before
extracting files or replaying output. Entries written before checksums were introduced remain
readable. `mise cache task <task> --json` includes the checksum for cache inspection tooling.

Readers, writers, inspection, and task-scoped deletion coordinate through a cross-process lock for
each cache key. Concurrent processes therefore see a complete archive and manifest pair instead of
mistaking an in-progress replacement for corruption; writers for unrelated keys remain independent.
Temporary archive and manifest files are removed when a write fails normally. On a later cache use,
mise also removes partial files abandoned by an interrupted process after acquiring the associated
cache-key lock, so it never deletes files that an active writer is still publishing.

Set [`task.cache_max_size`](/configuration/settings.html#task.cache_max_size) to bound the total
artifact cache size, or [`task.cache_max_age`](/configuration/settings.html#task.cache_max_age) to
expire entries based on their last access. Both limits are optional and apply after successful cache
writes. When a size limit is exceeded, mise removes least-recently-accessed entries first.

When a cache-enabled task executes instead of restoring a result, mise reports the reason: no
matching entry, a corrupt entry, forced execution, disabled reads, or a dependency that completed
without a stable cache key. Raw and dry-run cache bypasses retain their existing warning or preview
behavior and are not reported as cache misses.

Stdout and stderr are stored as ordered, redacted streams and replayed using the output mode selected
for the cache hit. Prefix, interleave, keep-order, timed, replacing, quiet, silent, and per-stream
silence therefore apply to replayed output just as they do to live output. Raw and interactive tasks
retain inherited terminal I/O and conservatively bypass artifact caching.

Cacheable dependencies contribute their artifact keys to dependent task keys, so a dependent can
restore the matching artifact after its dependencies execute, skip, or restore. If a dependency
executes without a stable artifact key, its dependents conservatively execute.

### `rust_cache` <Badge type="danger" text="deprecated" />

- **Type**: `boolean | table`
- **Default**: `false`

This setting no longer enables Rust compiler action caching. mise accepts it temporarily as a
deprecated no-op so existing task configurations continue to run. Enabled values print a migration
warning; disabled values are silent.

Use [mbx](https://mr-boxington.jdx.dev/getting-started) for Rust action caching instead. Install it
globally with `mise use -g mr-boxington`, or add it to the project tools, then put `mbx` in front of
the Cargo subcommand:

```mise-toml
[tools]
mr-boxington = "latest"

[tasks.build]
run = "mbx build"
```

Remove `rust_cache` after changing the command. The compatibility field is scheduled for removal in
mise 2027.8.14.

### `shell`

- **Type**: `string`
- **Default**: [`task_config.shell`](#task_config.shell) when set (config-scoped); otherwise
  [`unix_default_inline_shell_args`](/configuration/settings.html#unix_default_inline_shell_args)/[`windows_default_inline_shell_args`](/configuration/settings.html#windows_default_inline_shell_args) (global-only).
- **Note**: Only applies to toml-tasks.

The shell to use to run the task. This is useful if you want to run a task with a different shell than
the default such as `fish`, `zsh`, or `pwsh`. Generally though, it's recommended to use a [shebang](./toml-tasks#shell-shebang) instead
because that will allow IDEs with mise support to show syntax highlighting and linting for the script.

When the shell is PowerShell (`pwsh` or `powershell`), mise passes `-NoProfile` so your PowerShell
profile is not loaded, matching the non-interactive behavior of `sh -c`/`zsh -c`. This avoids profiles
that mutate `PATH` (for example a mise activation snippet) shadowing a task's own installed tools. Set
[`windows_powershell_no_profile`](/configuration/settings.html#windows_powershell_no_profile) to `false`
if your tasks depend on side effects from your profile.

```mise-toml
[tasks.hello]
run = '''
#!/usr/bin/env node
console.log('hello world')
'''
```

### `timeout`

- **Type**: `string`
- **Default**: unset

Maximum execution time for this task. The value accepts durations such as `30s`, `5m`, or `1h` and
supports Tera templates. The task fails if it does not complete within the configured duration.

```mise-toml
[tasks.integration-test]
run = "./scripts/integration-test.sh"
timeout = "10m"
```

This limits the individual task. Use [`mise run --timeout`](/cli/run.html) or the
[`task.timeout`](/configuration/settings.html#task.timeout) setting to limit the entire task run.
When both a global timeout and a per-task timeout are set, the shorter of the two wins: a per-task
timeout cannot extend beyond the global timeout. The `--timeout` CLI flag overrides the global
setting.

### `deny_all`

- **Type**: `bool`
- **Default**: `false`

Block filesystem reads, filesystem writes, network access, and environment inheritance for this
task. Specific `allow_*` properties can add exceptions.

```mise-toml
[tasks.lint]
run = "eslint ."
deny_all = true
allow_read = ["."]
allow_write = ["./node_modules/.cache"]
allow_env = ["NODE_*"]
```

Sandbox support and implicit system access vary by platform. See [Sandboxing](/sandboxing.html) for
the complete behavior and limitations.

### `deny_read`

- **Type**: `bool`
- **Default**: `false`

Block filesystem reads except for the system and mise paths required to execute the task. Use
`allow_read` to add task-specific exceptions.

### `deny_write`

- **Type**: `bool`
- **Default**: `false`

Block filesystem writes except for implicitly writable system paths such as the temporary
directory. Use `allow_write` to add task-specific exceptions.

### `deny_net`

- **Type**: `bool`
- **Default**: `false`

Block network access for this task. Use `allow_net` for host-specific exceptions on platforms that
support them.

### `deny_env`

- **Type**: `bool`
- **Default**: `false`

Block inherited environment variables except for essential variables such as `PATH`, `HOME`,
`USER`, `SHELL`, `TERM`, and `LANG`. Use `allow_env` or `pass_through_env` to preserve additional
variables.

### `allow_read`

- **Type**: `string[]`
- **Default**: `[]`

Allow reads from the listed paths and block other filesystem reads. Relative paths are resolved
from the task's effective working directory.

### `allow_write`

- **Type**: `string[]`
- **Default**: `[]`

Allow writes to the listed paths and block other filesystem writes. Allowed write paths are also
readable. Relative paths are resolved from the task's effective working directory.

### `allow_net`

- **Type**: `string[]`
- **Default**: `[]`

Allow network access to the listed hosts and block other network access. Per-host network filtering
is platform-dependent; see [Platform Support](/sandboxing.html#platform-support).

### `allow_env`

- **Type**: `string[]`
- **Default**: `[]`

Allow the listed environment variable names and block other inherited environment variables.
Entries support `*` wildcards, such as `MYAPP_*`.

### `pass_through_env` <Badge type="warning" text="experimental" />

- **Type**: `string[]`
- **Default**: `[]`

Preserve the listed ambient environment variables when environment inheritance is denied without
including their values in the task cache key. Entries support `*` wildcards. This property does not
enable environment sandboxing by itself and has no effect unless environment sandboxing is active,
including through `allow_env`, `deny_env`, `deny_all`, or an equivalent CLI or global sandbox option.

Use `pass_through_env` for values such as short-lived credentials that must not affect the cache key.
Do not use it for values that affect generated outputs or logs.
Use `cache.env` instead when changes to a variable should invalidate the task cache.

### `quiet`

- **Type**: `bool`
- **Default**: `false`

Suppress mise's output for the task such as showing the command that is run, e.g.: `[build] $ cargo build`.
When this is set, mise won't show any output other than what the script itself outputs. If you'd also
like to hide even the output that the task emits, use [`silent`](#silent).

`quiet` is a _verbosity_ setting and is independent of the [`output`](#output) _style_: it no longer
forces un-prefixed output, so `output = "prefix"` together with `quiet = true` keeps the task-name
prefixes while hiding mise's own messages.

### `silent`

- **Type**: `bool | "stdout" | "stderr"`
- **Default**: `false`

Suppress all output from the task. If set to `"stdout"` or `"stderr"`, only that stream will be suppressed.

### `output`

- **Type**: `string`
- **Default**: unset (inherits the global [`task.output`](/configuration/settings.html#task.output) setting)

Output _style_ for this task: `prefix`, `interleave`, `keep-order`, `replacing`, `timed`, `quiet`, or
`silent`. This is the per-task equivalent of the global `task.output` setting and is orthogonal to the
[`quiet`](#quiet)/[`silent`](#silent) verbosity fields, so styles and quietness combine freely
(e.g. `output = "prefix"` + `quiet = true`). The `quiet`/`silent` _values_ are kept for backwards
compatibility and bundle a style with that verbosity.

### `usage`

- **Type**: `string`

::: tip
For comprehensive information about task arguments and the usage field, see the dedicated [Task Arguments](/tasks/task-arguments) page.
:::

More advanced usage specs can be added to the task's `usage` field. This only applies to toml-tasks.

```mise-toml
[tasks.test]
usage = '''
arg "<file>" help="The file to test" default="src/main.rs"
'''
run = 'cargo test ${usage_file?}'
```

#### Environment Variable Support for Args and Flags

Both args and flags in usage specs can specify an environment variable as an alternative source for their value. This allows task arguments to be provided through environment variables when not specified on the command line.

The precedence order is:

1. CLI arguments/flags (highest priority)
2. Environment variables (middle priority)
3. Default values (lowest priority)

**For positional arguments:**

```mise-toml
[tasks.deploy]
usage = '''
arg "<environment>" env="DEPLOY_ENV" help="Target environment" default="staging"
arg "<region>" env="AWS_REGION" help="AWS region" default="us-east-1"
'''

run = '''
echo "Deploying to ${usage_environment?} in ${usage_region?}"
'''
```

Usage examples:

```bash
# Using CLI args (highest priority)
mise run deploy production us-west-2

# Using environment variables
export DEPLOY_ENV=production
export AWS_REGION=us-west-2
mise run deploy

# Using defaults (lowest priority)
mise run deploy  # deploys to staging in us-east-1

# CLI overrides environment variable
export DEPLOY_ENV=staging
mise run deploy production  # deploys to production
```

**For flags:**

```mise-toml
[tasks.build]
usage = '''
flag "-p --profile <profile>" env="BUILD_PROFILE" help="Build profile" default="dev"
flag "-v --verbose" env="VERBOSE" help="Verbose output"
'''

run = '''
echo "Building with profile: ${usage_profile?}"
echo "Verbose: ${usage_verbose:-false}"
'''
```

Usage examples:

```bash
# Using CLI flags
mise run build --profile release --verbose

# Using environment variables
export BUILD_PROFILE=release
export VERBOSE=true
mise run build

# Mixed usage - env var provides one, CLI provides another
export BUILD_PROFILE=release
mise run build --verbose
```

**File tasks** (tasks defined as executable files in `mise-tasks/` or `.mise/tasks/`) also support the `env` attribute:

```bash
#!/usr/bin/env bash
#USAGE arg "<input>" env="INPUT_FILE" help="Input file to process"
#USAGE flag "-o --output <file>" env="OUTPUT_FILE" help="Output file" default="out.txt"

echo "Processing ${usage_input?} -> ${usage_output?}"
```

**Required arguments:**

Environment variables can satisfy required argument checks. If an argument is marked as required (using angle brackets `<arg>`), providing its value through the environment variable specified in the `env` attribute fulfills that requirement:

```mise-toml
[tasks.deploy]
usage = '''
arg "<api-key>" env="API_KEY" help="API key for deployment"
'''
run = 'deploy --api-key ${usage_api_key?}'
```

```bash
# This will fail - no API_KEY provided
mise run deploy

# This succeeds - API_KEY provided via environment
export API_KEY=secret123
mise run deploy

# This also succeeds - provided via CLI
mise run deploy secret123
```

## Vars

Top-level [configuration vars](/configuration/vars) are available when rendering TOML tasks. Tasks
can also define task-local vars that override config vars for that task:

```mise-toml
[tasks.test]
vars = { e2e_args = "--headed" }
run = './scripts/test-e2e.sh {{vars.e2e_args}}'
```

## `[task_config]` options

Options available in the top-level `mise.toml` `[task_config]` section. These apply to all tasks which
are included by that config file or use the same root directory, e.g.: `~/src/myproject/mise.toml`'s `[task_config]`
applies to file tasks like `~/src/myproject/mise-tasks/mytask`. Set `cascade = true` to also apply the
section to tasks owned by descendant config roots.

### `task_config.cascade`

Cascade this config's `[task_config]` values to descendant config roots. Descendant values override
individual inherited fields. A descendant can set `cascade = false` to stop inheriting the section.

```toml
[task_config]
cascade = true
shell = "bash -c"
```

This applies to `dir`, `shell`, `cache`, `rust_cache`, `global_inputs`, `input_groups`, and
`includes`. Inherited include paths and task inputs remain relative to the config root where they
were defined.

A descendant's non-empty `global_inputs` replaces the inherited value. Descendant `input_groups`
merge with inherited groups by name; the nearest definition wins when the same name appears more
than once. This also applies to group references in inherited `global_inputs`. Each group remains
relative to the config root where it was defined.

### `task_config.dir`

Change the default directory tasks are run from.

```toml
[task_config]
dir = "{{cwd}}"
```

### `task_config.shell` {#task_config.shell}

Set the default shell for tasks in this config scope. A task's explicit `shell` setting takes
precedence, including a `shell` inherited from a task template. With `task_config.cascade = true`,
descendant config roots inherit this default and may override it with their own `task_config.shell`.

```toml
[task_config]
shell = "bash -c"
```

Unlike the global-only
[`unix_default_inline_shell_args`](/configuration/settings.html#unix_default_inline_shell_args) and
[`windows_default_inline_shell_args`](/configuration/settings.html#windows_default_inline_shell_args)
settings, this default is scoped to project tasks and cannot change the interpreter used by hooks,
tool installation, or tasks from another config root.

### `task_config.cache` <Badge type="warning" text="experimental" />

Sets the default artifact-cache configuration for tasks in this config scope. The default is only
inherited by cache-eligible tasks with sources and either explicit output paths or `outputs = []`.
Task-local and task-template cache configuration takes precedence, including
`cache = { enabled = false }`.

```toml
[task_config.cache]
enabled = true
env = ["NODE_ENV", "CI"]
command_inputs = ["node --version"]
```

### `task_config.rust_cache` <Badge type="danger" text="deprecated" />

This deprecated compatibility setting no longer enables Rust action caching. An effective enabled
value warns once while tasks continue normally. Remove it and run Rust build commands through
[mbx](https://mr-boxington.jdx.dev/getting-started) instead.

```toml
[task_config]
rust_cache = true
```

### `task_config.global_env` <Badge type="warning" text="experimental" />

Adds ambient environment variable names to the cache key of every cache-enabled task in the config
scope. These values compose with task-local `cache.env` rather than acting as defaults.

```toml
[task_config]
global_env = ["CI", "NODE_ENV"]
```

### `task_config.global_pass_through_env` <Badge type="warning" text="experimental" />

Preserves ambient environment variables when environment inheritance is denied, without adding
their values to task cache keys.

```toml
[task_config]
global_pass_through_env = ["CI_JOB_TOKEN"]
```

### `task_config.global_inputs` <Badge type="warning" text="experimental" />

Adds config-root-relative source paths and glob patterns to every task in this config scope. Entries
may reference a named input group with `@group:<name>`.

```toml
[task_config]
global_inputs = ["mise.toml", "@group:lockfiles"]
```

### `task_config.input_groups` <Badge type="warning" text="experimental" />

Defines reusable, config-root-relative source groups. Tasks reference them from `sources` with
`@group:<name>`. Groups may reference other groups.

```toml
[task_config.input_groups]
lockfiles = ["Cargo.lock", "pnpm-lock.yaml"]
rust = ["Cargo.toml", "src/**/*.rs", "@group:lockfiles"]
```

### `task_config.includes` {#task_config.includes}

Set the toml files and file-task directories mise should search when looking for tasks.

```toml
[task_config]
includes = [
    "tasks.toml", # a task toml file
    "mytasks"     # a directory containing file tasks
]
```

When `task_config.includes` is set, it replaces the default file-task directories for that config scope instead of adding to them.
Include entries are rendered as Tera templates, so they can reference values such as `config_root`,
`env`, and resolved `vars`.

The default file-task directories are:

- `mise-tasks`
- `.mise-tasks`
- `.mise/tasks`
- `.config/mise/tasks`
- `mise/tasks`

If you want to keep the defaults and add another directory, include the defaults explicitly:

```toml
[task_config]
includes = [
    "mise-tasks",
    ".mise-tasks",
    ".mise/tasks",
    ".config/mise/tasks",
    "mise/tasks",
    "mytasks",
    "tasks.toml",
]
```

For local and monorepo task discovery, mise uses the nearest config file that defines
`task_config.includes`. When the parent has `task_config.cascade = true`, its includes are inherited
until a child defines its own. A child config's `includes` replaces both the defaults and any
inherited `includes` for that directory.
User-global config files form one config scope, as do system config files. Within each scope, the
highest-precedence config that defines `task_config.includes` replaces lower-precedence includes and
the default directories. User-global and system scopes remain independent. User-global tasks replace
same-named system tasks without inheriting system task metadata, while system tasks with other names
remain available.

Entries are evaluated in order, and when more than one include defines a task with the same name the **last** entry in the list wins.
This applies uniformly to directory, toml-file, and `git::` includes, so to override a task coming from a `git::` include with a local one, list the local directory after the `git::` entry:

An inline `[tasks.<name>]` command takes precedence over a same-named task from
an included TOML file when it comes from the config that selected the include
or a higher-precedence config. An inline block without `run`, `run_windows`, or
`file` instead overlays metadata such as description, environment, and
dependencies. For executable file tasks, the script also remains the task's
command and the inline definition overlays its metadata.

The same overlay rule applies across layered inline task definitions. For
example, a metadata-only task in `mise.local.toml` overlays the nearest
lower-precedence command-bearing definition in `mise.toml`. A higher-precedence
definition with its own command still replaces the lower task. All metadata-only
definitions above the selected command-bearing base contribute in precedence
order, while definitions below it do not contribute metadata.

```toml
[task_config]
includes = [
    "git::https://github.com/myorg/shared-tasks.git//tasks", # remote task…
    ".mise/tasks",                                           # …is overridden by the local one with the same name
]
```

If using included task toml files, note that they have a different format than the `mise.toml` file. They are just a list of tasks.
The file should be the same format as the `[tasks]` section of `mise.toml` but without the `[task]` prefix:

::: code-group

```mise-toml [tasks.toml]
task1 = "echo task1"
task2 = "echo task2"
task3 = "echo task3"

[task4]
run = "echo task4"
vars = { target = "linux" }
```

:::

If you want auto-completion/validation in included toml tasks files, you can use the following JSON schema: <https://mise.jdx.dev/schema/mise-task.json>

#### Remote Git Includes <Badge type="warning" text="experimental" />

You can include directories or individual task toml files from git repositories using the `git::` URL syntax:

::: code-group

```mise-toml [ssh]
[task_config]
includes = [
    "git::ssh://git@github.com/myorg/shared-tasks.git//tasks?ref=v1.0.0",
    "git::ssh://git@github.com/myorg/shared-tasks.git//tasks/release.toml?ref=v1.0.0",
]
```

```mise-toml [https]
[task_config]
includes = [
    "git::https://github.com/myorg/shared-tasks.git//tasks?ref=main",
    "git::https://github.com/myorg/shared-tasks.git//tasks/release.toml?ref=main",
]
```

:::

URL format: `git::<protocol>://<url>//<path>?ref=<ref>`

Required fields:

- `protocol`: The git protocol (ssh or https).
- `url`: The git repository URL.
- `path`: The path to a directory or a `.toml` task file in the repository.

Optional fields:

- `ref`: The git reference (branch, tag, commit). Defaults to the repository's default branch.

When `path` points at a directory, mise loads both executable file tasks and any `.toml` task files inside that directory. When `path` points at a single `.toml` file, only that file is loaded.

Included `.toml` files use the [task toml file format](#task_config.includes) (the keys are task names — there is no `[tasks.…]` prefix). The repository will be cloned and cached in `MISE_CACHE_DIR/remote-git-tasks-cache`. Tasks from the include will be loaded as if they were local. You can disable caching with `MISE_TASK_REMOTE_NO_CACHE=true` or the `--no-cache` flag.

### `task_config.excludes` {#task_config.excludes}

Set paths or glob patterns to exclude from file-task discovery. Relative entries resolve from the
config root and may exclude a file, an entire directory, or files matched by a glob:

```toml
[task_config]
excludes = [
    ".mise/tasks/python/pyproject.toml",
    ".mise/tasks/generated",
    ".mise/tasks/**/fixtures/*.toml",
]
```

The closest config that defines `task_config.excludes` replaces inherited exclusions. Set it to an
empty array to clear exclusions inherited through `task_config.cascade = true`. Exclusions apply to
both the default task directories and paths selected by `task_config.includes`.

Task directories are searched recursively. Executable files are loaded as file tasks, and every
`.toml` file that is not a mise configuration file is loaded using the
[included task TOML format](#task_config.includes). Use `task_config.excludes` when other TOML files,
such as `pyproject.toml` or `Cargo.toml`, must live inside a task directory.

## Monorepo Support

mise supports monorepo-style task organization with target path syntax. Enable it by setting `monorepo_root = true` in your root `mise.toml`.

For complete documentation on monorepo tasks including:

- Task path syntax and wildcards
- Tool layering from parent configs
- Performance tuning
- Best practices and troubleshooting

See the dedicated [Monorepo Tasks](/tasks/monorepo) documentation.

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

You can also specify these as a glob pattern, e.g.: `redactions = ["SECRETS_*"]`.

## `[vars]` options

See [Variables](/configuration/vars).

## Task Configuration Settings

<script setup>
import Settings from '/components/settings.vue';
</script>

The following settings control task behavior. These can be set globally in `~/.config/mise/config.toml` or per-project in `mise.toml`:

<Settings :level="3" prefix="task" />
