---
description: "Run tasks across the projects in one repository with //path:task names, per-project tools and environment, and shared root configuration."
socialDescription: "Run tasks across a repository's projects with //path:task names and per-project tools."
---

# Monorepo tasks

Use monorepo mode when one repository holds several projects that each need
their own tools, environment variables, or tasks. Mark the root config as a
monorepo root and list the project directories. Every project's tasks then get a
path-based name such as `//projects/frontend:build`, which you can run from
anywhere in the repository with the tools and environment of the project that
defines it.

## Set up a monorepo

Declare the root and its projects in the repository's root `mise.toml`:

```toml
# mise.toml at the repository root
monorepo_root = true

[monorepo]
config_roots = ["projects/frontend", "projects/backend"]

[tools]
node = "24" # every project inherits this
```

Each listed directory has its own config:

```text
myproject/
├── mise.toml            # monorepo_root = true
└── projects/
    ├── frontend/
    │   └── mise.toml    # tasks: build, test
    └── backend/
        └── mise.toml    # tasks: build, test
```

mise names those tasks `//projects/frontend:build`, `//projects/frontend:test`,
`//projects/backend:build`, and `//projects/backend:test`.
[`mise tasks --all`](/cli/tasks/ls.html) lists them.

A config root is the directory that owns a mise config file. For both
`projects/frontend/mise.toml` and `projects/frontend/.mise/config.toml`, the
config root is `projects/frontend`.

### Config roots {#explicit-config-roots}

mise loads only the directories listed in `[monorepo].config_roots`; it does not
search the filesystem for them. Each entry is a path relative to the monorepo
root or a single-level glob:

```toml
[monorepo]
config_roots = [
  "packages/frontend",
  "packages/backend",
  "services/*",
]
```

- A glob such as `services/*` matches directories one level deep. It skips
  matches that have no mise config file, `.mise/tasks/` directory, or
  `mise-tasks/` directory.
- A listed path that has no mise config prints a warning.
- Recursive `**` globs, absolute paths, and paths containing `..` are ignored
  with a warning.

::: warning Automatic discovery is deprecated
Without `[monorepo].config_roots`, mise walks the filesystem to find task configs
and prints a deprecation warning. The
[`task.monorepo_depth`](/configuration/settings.html#task.monorepo_depth),
[`task.monorepo_exclude_dirs`](/configuration/settings.html#task.monorepo_exclude_dirs),
and [`task.monorepo_respect_gitignore`](/configuration/settings.html#task.monorepo_respect_gitignore)
settings control that walk and have no effect once `config_roots` is set.
[`mise install --monorepo`](/cli/install.html) and
[`mise ls --monorepo`](/cli/ls.html) have no fallback and fail without
`config_roots`.
:::

### Trust

In normal mode, trusting the monorepo root also trusts every config beneath it,
so review the whole repository before you run [`mise trust`](/cli/trust.html).
In [paranoid mode](/paranoid.html), trusting the root does not extend to the
configs beneath it, even if you trusted the root before you turned paranoid
mode on; trust each one. See
[Configuration trust](/security.html#configuration-trust) for the general rules.

## Run tasks by path {#task-path-syntax}

Run a monorepo task with [`mise run`](/cli/run.html) or directly as
`mise <task>`:

```sh
mise //projects/frontend:build
mise run //projects/frontend:build
```

Scripts normally use `mise run`, because a future mise subcommand could share a
task's name. mise never adds a subcommand that starts with `//` or `:`, so the
short form is safe for monorepo tasks. Quote any pattern that contains `*` so
the shell does not expand it.

### Absolute paths

A name that starts with `//` is a path from the monorepo root:

```sh
mise //projects/frontend:build
mise //projects/backend:test
```

### Tasks in the current project {#current-config-root-tasks}

A name that starts with `:` refers to the nearest config root that encloses the
current directory:

```sh
cd projects/frontend/src/components
mise :build # runs //projects/frontend:build
```

When no config root encloses the current directory, the name resolves against
the monorepo root.

### Bare task names

Inside a config root, a bare name such as `build` resolves the same way as
`:build`, on the command line and in `depends`. Existing projects can therefore
switch to monorepo mode without renaming their dependencies. Prefer `:build` in
new configuration so it is clear that the name is project-relative.

## Wildcards {#wildcard-patterns}

Task paths accept `...` in the directory part and `*` or `**` in the task-name
part:

```sh
mise //...:test                            # test in every project
mise //projects/...:build                  # build in every project under projects/
mise //projects/.../api:build              # projects/api, projects/x/api, projects/x/y/api, ...
mise //.../frontend:build                  # build in every project directory named frontend
mise '//projects/frontend:*'               # every task in projects/frontend without a : group
mise '//projects/frontend:**'              # every task in projects/frontend, including test:unit
mise '//projects/frontend:test:*'          # test:unit, but not test:unit:fast
mise '//...:**'                            # every task in every project
mise run '//...:test' ::: '//...:test:**'  # test and every test:… task in every project
```

`...` matches zero or more directories, following the Bazel and Buck2
convention. Shell-style globs such as `//projects/*:build` are not supported in
the directory part. In the task-name part, `*` matches within one `:` group and
`**` matches across groups, as with other [task wildcards](/tasks/running-tasks.html#wildcards).

Group related projects under a shared directory, such as `services/`, and give
related tasks a shared prefix, such as `test:`, so that one pattern selects them.

## Path aliases {#short-names-for-project-paths}

Give a config root a shorter name with `[monorepo.path_aliases]`:

```toml
monorepo_root = true

[monorepo]
config_roots = ["foo/bar/baz/abc/123"]

[monorepo.path_aliases]
"123" = "foo/bar/baz/abc/123"
```

`mise run //123:build` then runs `//foo/bar/baz/abc/123:build`. The alias works
for every task in that root, including patterns such as `//123:*`. The full path
remains the task's canonical name and keeps working.

An alias is a single path segment made of letters, digits, `-`, `_`, and `.`,
and cannot contain `...`. It must point to a root listed in `config_roots`,
directly or through a glob. It cannot be the first segment of a configured root,
such as `projects` when `projects/frontend` is a root.

## Depend on tasks in other projects

Name another project's task by its path in `depends`:

```mise-toml
# projects/frontend/mise.toml
[tasks.build]
depends = ["//libs/shared:build", ":lint"]
run = "npm run build"
```

`//libs/shared:build` runs with the tools and environment of `libs/shared`
before the frontend's `build`. When another build system, such as Turborepo,
Cargo, or Bazel, already orders the builds, let one mise task call it instead of
declaring the same edges again in `depends`.

### Relative dependency paths

A dependency that starts with `./` resolves relative to the project that
declares it, so one declaration works at any depth:

```toml
[tasks.test]
depends = [{ task = "./...:groups:tests:*", optional = true }]
```

Declared by `//apps/frontend:test`, this becomes
`//apps/frontend/...:groups:tests:*`. It matches the project and the projects
beneath it, but not its siblings.

## Tool, environment, and vars layering

A project's tasks use the tools and environment variables of every config above
them, and the project's own config can override or add to them. Put tools that
most projects share in the root config and override them only where a project
needs a different version.

```toml
# mise.toml at the repository root
monorepo_root = true

[monorepo]
config_roots = ["projects/frontend", "projects/backend"]

[tools]
node = "24"
python = "3.13"

[env]
LOG_LEVEL = "info"
```

```mise-toml
# projects/frontend/mise.toml
[tools]
node = "22" # overrides the root's node 24

[env]
LOG_LEVEL = "debug" # overrides the root's LOG_LEVEL
PORT = "3000"       # adds a variable

[tasks.build]
run = "npm run build" # node 22, python 3.13, LOG_LEVEL=debug
```

```mise-toml
# projects/backend/mise.toml
[tasks.build]
run = "npm run build" # node 24, python 3.13, LOG_LEVEL=info from the root
```

A task's tools and environment are merged in this order, later entries winning:

1. Every config file above the task's config root, from your global config down
   to the monorepo root and any directories in between.
2. The config file in the task's config root.
3. The task's own `tools` and `env`.

`vars` follow the same hierarchy. Templates in a project's task, such as
<span v-pre>`sources = ["{{ env.SRC_DIR }}/*"]`</span> or a
`task_config.includes` URL that uses <span v-pre>`{{ vars.central_ref }}`</span>,
render with that project's environment and vars wherever you run the task from.

Tasks also receive `MISE_MONOREPO_ROOT`, the monorepo root, and
`MISE_PROJECT_ROOT`, the task's config root. See
[Task environment](/tasks/running-tasks.html#task-environment) for the other
variables.

## List tasks {#listing-tasks}

`mise tasks` lists the tasks of the current config root and the configs above
it. `mise tasks --all` lists every task in the monorepo. Given this layout:

```text
myproject/
├── mise.toml            # task: deploy
└── projects/
    ├── frontend/
    │   └── mise.toml    # tasks: build, test
    └── backend/
        └── mise.toml    # tasks: build, serve
```

From `projects/frontend/`:

```sh
# //:deploy, //projects/frontend:build, //projects/frontend:test
mise tasks

# also //projects/backend:build and //projects/backend:serve
mise tasks --all
```

To list one project's tasks from anywhere, filter the full list:

```sh
mise tasks --all --name-only | grep '^//projects/frontend:'
```

## Install tools for every project {#tools}

`mise install --monorepo` installs the union of the tools configured in every
directory listed in `[monorepo].config_roots`, using the active `MISE_ENV`. Use
it in CI to warm a tool cache for the whole repository:

```sh
MISE_ENV=ci mise install --monorepo
```

Pass tool names to install only those tools, keeping every configured version:

```sh
mise install --monorepo node
```

Add `--include-task-tools` to also install the tools that tasks declare in their
own `tools`. `mise ls --monorepo` lists the same union, which helps when you
build CI cache keys or check which config roots contribute a tool.

## Lockfiles

Set `[monorepo] lockfile = true` to keep one set of lockfiles at the monorepo
root (`mise.lock`, `mise.ci.lock`, `mise.local.lock`) for the tools of every
project, or `lockfile = false` to keep a lockfile next to each project's config.
When you switch to `true`, the next command that reads or writes lockfiles
merges the project lockfiles into the root one and deletes them; root entries
win on conflicts. The default and its rollout schedule are described in
[Lockfile (mise.lock)](/dev-tools/mise-lock.html#monorepos).

## Nested monorepo roots

When more than one config in the hierarchy sets `monorepo_root = true`, the
nearest one wins. This happens with Git worktrees checked out inside the main
checkout:

```text
myproject/mise.toml                       # monorepo_root = true
myproject/packages/api/mise.toml
myproject/.worktrees/feature-x/mise.toml  # monorepo_root = true (same repository, other branch)
myproject/.worktrees/feature-x/packages/api/mise.toml
```

From inside `myproject/.worktrees/feature-x`, that directory is the monorepo
root: `//packages/api:build` resolves to the worktree's copy,
<code v-pre>{{ config_root }}</code> points inside the worktree, and the
worktree's own `[monorepo].config_roots` are the ones expanded.

Tasks from the enclosing monorepo are not loaded. They belong to a different
monorepo's task set, so loading them would put them outside the `//` namespace,
with `build` from the main checkout next to `//:build` from the worktree.
Configs above the enclosing root, such as your global config or
`$HOME/mise.toml`, still contribute tasks as usual.

The enclosing config is still an ancestor for tools, environment variables, and
vars, which it passes down like any parent config. To avoid that as well, keep
worktrees outside the main checkout, for example in `myproject-worktrees/feature-x`.

## Shared task definitions {#task-templates}

Define a [task template](/tasks/templates.html#monorepo-usage) in the root config
and `extends` it in each project. To give every project's task of a given name
the same defaults without touching the projects, use
[`[monorepo.task_defaults]`](/tasks/workspace-graph.html#root-task-defaults)
<Badge type="warning" text="experimental" />.

## `[monorepo]` reference

`monorepo_root = true` marks the config root. These keys go in the
`[monorepo]` section of a config at that root:

| Key             | Type                       | Default | Description                                                                                                           |
| --------------- | -------------------------- | ------- | --------------------------------------------------------------------------------------------------------------------- |
| `config_roots`  | array of strings           | none    | Project directories, as paths or single-level globs. See [Config roots](#explicit-config-roots)                       |
| `path_aliases`  | table of strings           | `{}`    | Short names for config roots. See [Path aliases](#short-names-for-project-paths)                                      |
| `lockfile`      | boolean                    | unset   | Root or per-project lockfiles. See [Lockfiles](#lockfiles)                                                            |
| `task_defaults` | table of task definitions  | `{}`    | Experimental defaults by task name. See [Root task defaults](/tasks/workspace-graph.html#root-task-defaults)          |
| `projects`      | table of project overrides | `{}`    | Experimental corrections to the project graph. See [Project overrides](/tasks/workspace-graph.html#project-overrides) |

## Next steps

- [Workspace project graph](/tasks/workspace-graph.html) <Badge type="warning" text="experimental" />:
  infer projects and their dependencies from Cargo, uv, Go, and Node.js
  workspaces, run only affected tasks, and import package scripts.
- [Task templates](/tasks/templates.html): share task definitions between
  projects.
- [Task caching](/tasks/caching.html): skip or restore tasks whose inputs have
  not changed.
