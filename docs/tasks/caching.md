# Task caching

Use ordinary `sources` and `outputs` checks to skip work that is already up to
date. Enable experimental artifact caching when you also need to reuse successful
results after switching inputs or deleting build outputs.

| Mechanism        | Compares                                             | On a hit                                             |
| ---------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| Freshness checks | Source and output modification times                 | Leaves existing outputs in place and skips the task. |
| Artifact cache   | Declared input contents and other cache-key material | Restores declared outputs and replays captured logs. |

For freshness configuration, see [`sources`](/tasks/task-configuration.html#sources)
and [`outputs`](/tasks/task-configuration.html#outputs). The remainder of this guide
covers the experimental artifact cache.

## Enable artifact caching

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

Audit mode requires `strace` on `PATH`. mise warns and runs the task normally when tracing is not
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

## External dependencies and lockfiles

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

## Per-run cache access

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
mise run --task-cache off --force build
```

These modes only affect the experimental task output cache configured by a task's `cache` property.
The existing `--no-cache` option controls fetching remote task definitions instead.

## Remote cache and sensitive data

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

The client permits remote writes only for recognized protected-branch push jobs
in GitHub Actions or GitLab. Local runs, pull requests, tags, and unrecognized CI
contexts are restricted to reads; a write-only configuration disables the remote
in those contexts. The server must enforce authorization independently. See the
[remote protocol](./remote-cache-protocol.html#transport-and-versioning).

Set `MISE_TASK_CACHE_REMOTE_TOKEN` in the process environment to send a bearer credential. The
equivalent `task.cache.remote_token` setting is global-only, but the environment variable is
preferred so a token does not need to be written to disk. mise redacts the token from settings trace
output and marks its HTTP header as sensitive. Requests carrying credentials require HTTPS outside loopback. An unauthenticated
HTTP connection is permitted with a warning and provides no transport confidentiality
or server authentication. Servers should still use short-lived,
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
      - uses: jdx/mise-action@v4
      - run: mise run test
```

The cache service must trust GitHub's issuer, accept the configured audience, and authorize the
workflow's identity claims for the selected namespace. mise obtains the token from GitHub's job
OIDC endpoint, keeps it only in memory, and refreshes it before expiry. The audience setting is
global-only, and acquisition fails with a clear error when the workflow lacks `id-token: write` permission.

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

## Cache correctness and deterministic tasks

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

## Inspect and diagnose cached results

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

## Environment variables and cache keys

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
that must not affect its cache key, such as short-lived credentials. The scoped
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

## Storage, retention, and output replay

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
