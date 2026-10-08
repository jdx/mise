---
description: "Control where mise looks for tasks, how included and remote task files load, and which definition wins when names collide."
socialDescription: "Control where mise finds tasks and which definition wins when names collide."
---

# Task discovery and precedence

mise collects tasks from config files, from task files you include, and from
executable scripts in task directories, in the current directory and each of
its parents. Use [`task_config.includes`](#include-task-files-and-directories)
to change where it looks, and read [Which definition wins](#which-definition-wins)
when two sources define the same task name.

To see where a task came from, run [`mise tasks info <task>`](/cli/tasks/info.html).

## Task sources

mise loads three kinds of task source:

- `[tasks.<name>]` tables in config files such as `mise.toml`
- [TOML task files](#included-toml-files) that a config includes or that sit in
  a task directory
- executable [file tasks](/tasks/file-tasks.html) in task directories

It reads these sources for the current directory, for every parent directory,
and for the [global config](#global-tasks). Tasks from a parent directory are
available in its subdirectories:

```text
project/
├── mise.toml          # defines lint, test, build
└── frontend/
    └── mise.toml      # defines test and bundle
```

In `frontend/`, `mise run lint` and `mise run build` run the parent's tasks,
`mise run test` runs the `frontend` definition, and `mise run bundle` is only
available there.

### Default task directories

Unless a config sets [`task_config.includes`](#include-task-files-and-directories),
mise looks for file tasks in these directories, in this order:

1. `mise-tasks`
2. `.mise-tasks`
3. `.mise/tasks`
4. `.config/mise/tasks`
5. `mise/tasks`

mise searches each directory recursively. It loads every executable file as a
file task and every `.toml` file that is not a mise config file as a
[TOML task file](#included-toml-files). It skips files and subdirectories
inside them whose names start with `.`. Subdirectories become part of the task
name, as described in [file task grouping](/tasks/file-tasks.html#task-grouping).
When two of these directories define the same task, the one later in the list
wins.

On Windows there is no execute permission; see
[Windows](/tasks/file-tasks.html#windows) for how mise decides that a file is a
task there.

### Global tasks

For the global config, mise resolves the same directories from your home
directory, so file tasks in `~/.config/mise/tasks` are available in every
project. mise loads global tasks only when a global config file, such as
`~/.config/mise/config.toml`, exists. Inline tasks in that file are global too.

### Trust

Loading a task file can run code when the file contains Tera template syntax,
because mise renders templates while it loads tasks. In a directory with no
trusted config file, such a file, or one that lists `secrets`, must be trusted
before mise loads it, the same way an untrusted config file must be. In
[paranoid mode](/paranoid.html), every task file in such a directory needs
trust. See [configuration trust](/security.html#configuration-trust).

## Which definition wins

When more than one source defines a task with the same name, these rules pick
the definition:

- A task from a nearer directory replaces a same-named task from a parent
  directory, and a project task replaces a global task.
- Within one directory, the task from the higher-precedence config file wins,
  for example `mise.local.toml` over `mise.toml`. See
  [config file precedence](/configuration.html#mise-toml). A block without a
  command adds metadata instead; see
  [layered task definitions](#layered-task-definitions).
- An inline `[tasks.<name>]` block with a command replaces a task of the same
  name from an included TOML file or a task directory, as long as the block
  comes from the config that selected that file or a higher-precedence one. See
  [configuring file tasks from TOML](#configuring-file-tasks-from-toml).
- Among the entries of one `includes` list, the last entry that defines a name
  wins.
- User-global config files form one scope and system config files another. A
  user-global task replaces a system task of the same name entirely, without
  inheriting its metadata. System tasks with other names stay available.
- Each [conf.d folder](/configuration.html#conf-d-folders) is its own root. Its
  `includes` resolve inside the folder and replace only the folder's own
  default directories.

## Include task files and directories

Set `task_config.includes` to list the TOML task files and task directories
mise reads for a config root:

```toml
[task_config]
includes = [
  "tasks.toml", # a TOML task file
  "mytasks",    # a directory of file tasks and TOML task files
]
```

The list replaces the [default task directories](#default-task-directories)
for that config root. To keep them and add another, list them explicitly:

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

Each entry can be:

- a path relative to the config root, an absolute path, or a path that starts
  with `~`
- a glob pattern such as `tasks/*.toml`
- a [`git::` URL](#remote-git-includes) or an [`oci::` reference](#remote-oci-includes)

mise renders entries as Tera templates, so they can use
<code v-pre>{{ config_root }}</code>, <code v-pre>{{ env.HOME }}</code>, and
resolved `vars`:

```toml
[task_config]
includes = ["{{ config_root }}/tasks/*.toml", "~/shared-tasks"]
```

### Which includes list applies

For each directory, mise uses the `includes` list of the highest-precedence
config file in that directory that sets one. Lists from other config files in
the same directory are not combined with it. Without a list, mise uses the
default task directories of that directory.

A parent's list applies to a child directory only when the parent sets
[`task_config.cascade = true`](/tasks/task-configuration.html#task_config.cascade).
The inherited entries still resolve from the parent's config root, and a child
that sets its own `includes` replaces the inherited list.

### Override an included task

When more than one entry defines a task with the same name, the last entry in
the list wins. This applies to directories, TOML task files, and remote
includes alike, so to override a task from a shared repository with a local
one, list the local directory after the `git::` entry:

```toml
[task_config]
includes = [
  "git::https://github.com/myorg/shared-tasks.git//tasks", # shared tasks
  ".mise/tasks", # a local task with the same name overrides the shared one
]
```

## Included TOML files

A TOML task file contains only tasks. Write each task as you would under
`[tasks]` in `mise.toml`, without the `tasks.` prefix:

```mise-toml [tasks.toml]
task1 = "echo task1"
task2 = "echo task2"

[task3]
run = "echo task3"
vars = { target = "linux" }
```

For completion and validation in an editor, use the JSON schema at
<https://mise.jdx.dev/schema/mise-task.json>.

## Configuring file tasks from TOML

Use a `[tasks.<name>]` block to configure an executable file task. A block
without `run`, `run_windows`, or `file` adds metadata and keeps the script as
its command. Adding one of those fields replaces the script's command, subject
to [file task config precedence](#file-task-config-precedence).

Use a block to set the properties that `#MISE` header lines do not accept:
`vars`, `timeout`, and the `deny_*` and `allow_*`
[sandbox properties](/tasks/task-configuration.html#sandbox).

### Add metadata and dependencies

For `mise-tasks/hello.sh`, use either `[tasks.hello]` or `[tasks."hello.sh"]`:

```toml [mise.toml]
[tasks.hello]
description = "Say hello after linting"
env = { GREETING = "hi" }
depends = ["lint"]
```

`mise run hello` runs `lint` and then the script with `GREETING=hi`.
`mise tasks ls` shows the description. The script's full task name is
`hello.sh`; `mise run` also accepts `hello` without the extension.

The full name selects one script. The name without the extension selects all
scripts with that name, unless a task already has that exact name. For example,
if both `hello.sh` and `hello.js` exist, `[tasks.hello]` configures both, while
`[tasks."hello.sh"]` configures only `hello.sh`.

### Replace a script's command

Set `run`, `run_windows`, or `file` to replace a matching file task:

```mise-toml [mise.toml]
[tasks.hello]
run = "echo hi"
```

`mise run hello` now runs `echo hi`. The discovered `hello.sh` no longer exists
as a separate task, so `mise run hello.sh` is no longer available. If `hello.js`
also exists, this block replaces both scripts with one task named `hello`.

To replace only `hello.sh`, use its full name:

```mise-toml [mise.toml]
[tasks."hello.sh"]
run = "echo hi"
```

Here, `mise run hello.sh` runs `echo hi`, and `hello.js` remains a separate
task. To keep both the original script and a new command available, give the
command a different task name.

### File task config precedence

A command replaces a script only when its block comes from the config whose
[`task_config.includes`](#include-task-files-and-directories) selected the
script's directory, or from a higher-precedence config. A lower-precedence
block can add metadata, but its command is ignored and the script still runs.
This applies to both full names and names without extensions.

When no config sets `task_config.includes`, mise discovers scripts in the
default directories. In that case, a command from any config in the chain can
replace a matching script.

While a script remains the task's command, only the highest-precedence TOML
block that matches it supplies metadata. Lower-precedence blocks add nothing,
including `env` and `alias`. For example, `[tasks.hello]` in `mise.local.toml`
takes precedence over `[tasks."hello.sh"]` in `mise.toml`; their metadata is
not combined.

Once a TOML command replaces the script,
[layered task definitions](#layered-task-definitions) apply. Higher-precedence
metadata blocks can configure the replacement using either name. For example,
`[tasks."hello.sh"]` in `mise.local.toml` can add a description to
`[tasks.hello] run = "echo hi"` in `mise.toml`. Blocks below the selected
command contribute nothing. If both names declare commands, the
higher-precedence command wins.

### Windows script pairs

On Windows, mise selects the [Windows-native sibling](/tasks/file-tasks.html#windows)
from a pair such as `build.sh` and `build.ps1`, and names the task `build`.
Use `[tasks.build]` to configure or replace that task. A block named
`[tasks."build.ps1"]` defines a separate task.

## Layered task definitions

An inline `[tasks.<name>]` block without `run`, `run_windows`, or `file` adds
metadata to a task of the same name from a lower-precedence config. It can add
a description, environment variables, or dependencies without repeating the
command.

::: code-group

```toml [mise.toml]
[tasks.check]
depends = ["lint", "test"]
```

```toml [mise.local.toml]
[tasks.check]
description = "Run the project checks"
```

:::

Here, `mise run check` still runs `lint` and `test`. A dependency group can
receive metadata even when it has no command of its own.

When a definition with `run`, `run_windows`, or `file` exists, it provides the
command. Blocks above the highest-precedence command definition add metadata in
precedence order, including any `depends` they declare. Definitions below that
command do not contribute. When no definition has a command, the
highest-precedence dependency group provides the base instead.

For a task from an [included TOML file](#included-toml-files), an inline
command replaces the included task, while an inline block without a command
adds metadata. The inline block must come from the config that selected the
include or a higher-precedence config. This is also required when
[replacing a file task's command](#file-task-config-precedence).

## Remote git includes

Include a directory or a single TOML task file from a git repository with a
`git::` URL:

::: code-group

```toml [ssh]
[task_config]
includes = [
  "git::ssh://git@github.com/myorg/shared-tasks.git//tasks?ref=v1.0.0",
  "git::ssh://git@github.com/myorg/shared-tasks.git//tasks/release.toml?ref=v1.0.0",
]
```

```toml [https]
[task_config]
includes = [
  "git::https://github.com/myorg/shared-tasks.git//tasks?ref=v1.0.0",
  "git::https://github.com/myorg/shared-tasks.git//tasks/release.toml?ref=v1.0.0",
]
```

:::

When the path names a directory, mise loads the executable file tasks and the
TOML task files inside it, as it does for a local task directory. When the path
names a `.toml` file, mise loads only that file, in the
[TOML task file format](#included-toml-files). Tasks from the include behave
like local tasks.

mise clones the repository into `MISE_CACHE_DIR/remote-git-tasks-cache` and
reuses that clone on later runs, so a branch such as `main` does not pick up
new commits until you clear the cache with [`mise cache clear`](/cli/cache/clear.html).
Run `mise run --no-cache`, or set
[`task.remote_no_cache`](/configuration/settings.html#task.remote_no_cache), to
clone on every run. Pin a tag or a commit to keep runs reproducible.

### Git URL syntax

`git::` URLs in `task_config.includes` and in a task's
[`file`](/tasks/task-configuration.html#file) use this form:

```text
git::<protocol>://<repository>.git//<path>?ref=<ref>
```

- `protocol`: `ssh` or `https` (`http` also works). For SSH, put the user in
  the URL, as in `ssh://git@github.com/...`.
- `repository`: the host and path of the repository, before the `.git` suffix,
  such as `github.com/myorg/shared-tasks`. Azure DevOps `_git` URLs and
  `git@ssh.dev.azure.com:v3/<org>/<project>/<repo>` are also accepted without
  `.git`.
- `path`: the file or directory inside the repository, after a double slash. It
  must be relative and cannot contain `.` or `..` segments.
- `ref` (optional): a branch, tag, or commit. Without it, mise uses the
  repository's default branch.

## Remote OCI includes

Include a task catalog published as an OCI artifact by prefixing an image
reference with `oci::`:

```toml
[task_config]
includes = [
  "oci::ghcr.io/myorg/shared-tasks:1.0.0",
  "oci::registry.example.com/platform/tasks@sha256:0f1e2d3c...",
]
```

The reference uses the same syntax as `docker pull`: `<registry>/<repository>`
followed by `:<tag>` or `@sha256:<digest>`. Pin a version tag or a digest.
Tags such as `latest` can move, and mise reuses a cached pull for the same
reference.

Publish a catalog with [`oras push`](https://oras.land/docs/commands/oras_push/).
Each pushed file becomes a file in the task directory, and a pushed directory
is extracted under its name:

```sh
oras push ghcr.io/myorg/shared-tasks:1.0.0 build.toml scripts/deploy
```

mise unpacks the artifact into a directory and loads it like a local task
directory: executable file tasks and `.toml` [task files](#included-toml-files)
are both picked up. Artifacts do not carry file modes, so mise makes every
file that starts with a `#!` line executable.

::: details How mise reads artifact layers

- A layer with an `org.opencontainers.image.title` annotation becomes a file at
  that relative path. This is what `oras push` and
  [`podman artifact add`](https://docs.podman.io/en/stable/markdown/podman-artifact-add.1.html)
  produce.
- A tar layer with that annotation and `io.deis.oras.content.unpack=true` (an
  `oras push` of a directory) is extracted into the directory of that name.
- A tar, tar+gzip, or tar+zstd layer without a title is extracted at the root,
  with whiteouts applied, so an artifact made of tar layers (for example with
  `crane append`) works too. This is not a general container-image reader: an
  image whose layers contain symlinks or device files is rejected.

:::

mise verifies every blob against the digest in the manifest and rejects
symlinks and other special files. Credentials come from the same places as
[`mise oci push`](/cli/oci/push.html): `docker login` or `podman login`
configuration, with anonymous access when none is found. mise contacts
registries on loopback addresses over plain HTTP; add other plain-HTTP
registries to [`oci.insecure_registries`](/configuration/settings.html#oci.insecure_registries).

Pulls are cached per reference in `MISE_CACHE_DIR/remote-oci-tasks-cache`, so
mise does not notice when a tag moves to a new digest. To refresh, reference a
new tag or digest, delete that directory, or set
[`task.remote_no_cache`](/configuration/settings.html#task.remote_no_cache) to
pull on every run.

## Exclude paths from discovery

Set `task_config.excludes` to skip files, directories, or glob matches inside
task directories. Relative entries resolve from the config root:

```toml
[task_config]
excludes = [
  ".mise/tasks/python/pyproject.toml",
  ".mise/tasks/generated",
  ".mise/tasks/**/fixtures/*.toml",
]
```

Exclusions apply to the default task directories and to paths selected by
`task_config.includes`, including a TOML task file listed there directly. Use
them when other TOML files, such as `pyproject.toml` or `Cargo.toml`, must live
inside a task directory, since mise would otherwise try to load them as task
files.

The closest config that sets `task_config.excludes` replaces inherited
exclusions. Set it to an empty array to clear exclusions inherited through
`task_config.cascade = true`. The
[`task.disable_paths`](/configuration/settings.html#task.disable_paths) setting
also keeps mise from looking for tasks in the listed paths.
