# Remote Tasks <Badge type="warning" text="experimental" />

mise supports loading tasks from remote sources such as git repositories and HTTP URLs. This allows
teams to share task definitions across projects without duplicating code.

## Remote task directories via `task_config.includes`

You can include entire directories of [file tasks](/tasks/file-tasks) from a git repository using the
`git::` URL syntax in `task_config.includes`. This is the recommended way to share collections of
tasks across multiple projects.

::: code-group

```toml [SSH]
[task_config]
includes = [
  "git::ssh://git@github.com/myorg/shared-tasks.git//.mise/tasks?ref=v1.0.0",
]
```

```toml [HTTPS]
[task_config]
includes = [
  "git::https://github.com/myorg/shared-tasks.git//.mise/tasks?ref=main",
]
```

:::

You can include multiple remote repositories:

```toml
[task_config]
includes = [
  "git::ssh://git@github.com/myorg/build-tasks.git//.mise/tasks?ref=v2",
  "git::ssh://git@github.com/myorg/deploy-tasks.git//.mise/tasks?ref=v1",
  "mytasks", # local directories can be mixed in
]
```

The tasks from included directories are loaded as if they were local file tasks. Task names are
derived from the file paths within the included directory, using `:` as a separator. For example,
a file at `.mise/tasks/deploy/staging` in the remote repo would become the task `deploy:staging`.

### URL format

```
git::<protocol>://<url>//<path>?ref=<ref>
```

| Field      | Required | Description                                                                      |
| ---------- | -------- | -------------------------------------------------------------------------------- |
| `protocol` | Yes      | The git protocol (`ssh` or `https`)                                              |
| `url`      | Yes      | The git repository URL                                                           |
| `path`     | Yes      | The path to the task directory in the repository                                 |
| `ref`      | No       | Git reference (branch, tag, commit). Defaults to the repository's default branch |

## Remote task files via `file`

Individual [TOML tasks](/tasks/toml-tasks) can point to a remote script using the `file` property.
This is useful when you want to define the task metadata locally but execute a remote script.

### HTTP

```toml
[tasks.build]
file = "https://example.com/build.sh"
```

::: warning
The file will be downloaded and executed. Make sure you trust the source.
:::

### Git

::: code-group

```toml [SSH]
[tasks.build]
file = "git::ssh://git@github.com/myorg/scripts.git//build.sh?ref=v1.0.0"
```

```toml [HTTPS]
[tasks.build]
file = "git::https://github.com/myorg/scripts.git//build.sh?ref=v1.0.0"
```

:::

The URL format is the same as for `task_config.includes`, but `path` points to a single file
instead of a directory.

## Caching

Remote tasks are cached in `MISE_CACHE_DIR/remote-git-tasks-cache`. Once downloaded, cached
versions are used on subsequent runs without re-fetching.

To force a fresh fetch:

- **Clear the cache**: run `mise cache clear`
- **Disable caching entirely**: set the `MISE_TASK_REMOTE_NO_CACHE=true` environment variable or
  use the `--no-cache` flag with `mise run`
- **Per-project setting**: add `task_remote_no_cache = true` to your `mise.toml` settings

::: tip
Pin your remote tasks to a specific tag or commit ref (e.g., `?ref=v1.0.0`) to ensure
reproducible builds. Using a branch name like `main` means the cached version may become stale.
:::

## Customizing remote tasks with hooks

When using remote tasks, you may want to run additional commands before or after a task without
modifying the original remote definition. Use [pre_task/post_task hooks](/hooks#pre-task-post-task-hooks)
for this:

```toml
[task_config]
includes = ["git::https://github.com/myorg/shared-tasks.git//.mise/tasks?ref=v1"]

# Run a script before the remote "deploy" task
[[hooks.pre_task]]
script = "echo 'setting up environment...'"
task = "deploy"

# Run a local task after any build task
[[hooks.post_task]]
run = "notify"
task = "build:*"
```

Hooks support both `script` (shell commands) and `run` (execute another mise task). See
[Hooks](/hooks#pre-task-post-task-hooks) for full documentation.
