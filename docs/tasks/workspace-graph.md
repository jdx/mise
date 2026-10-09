---
description: "Infer projects and dependencies from Cargo, uv, Go, and Node.js workspaces to run affected and upstream tasks in a monorepo."
socialDescription: "Infer monorepo projects from Cargo, uv, Go, and Node.js workspaces to run affected tasks."
---

# Workspace project graph

mise can read Cargo, uv, Go, and Node.js workspace manifests to learn which
projects a [monorepo](/tasks/monorepo.html) contains and how they depend on
each other. Use the graph to run tasks only in the projects a change affects, to
run a task in upstream projects first, and to import Node.js package scripts as
tasks.

The graph is separate from config roots. Config roots tell mise where task
configuration lives; the graph comes from ecosystem manifests, and a project
does not need its own `mise.toml` to appear in it.

## Set up the graph

Mark the repository root as a monorepo root and set `[monorepo].config_roots`:

```toml
# mise.toml at the repository root
monorepo_root = true

[monorepo]
config_roots = ["apps/*", "packages/*"]
```

Without [`config_roots`](/tasks/monorepo.html#explicit-config-roots), mise
falls back to its deprecated filesystem walk and prints a warning. Packages
appear in the graph whether or not they have a mise config. A glob such as
`packages/*` skips packages without one silently, but an explicit path to a
package without one prints a warning each time mise loads tasks.

Inspect the projects mise found:

```sh
mise tasks graph            # each project's ID, root, dependencies, and metadata
mise tasks graph --explain  # which provider and file contributed each project, edge, and task
mise tasks graph --json     # the same information as JSON
```

`--explain` also shows the provider and manifest behind each task field a
provider suggests, such as inputs or outputs. Values from
[`[monorepo.projects]`](#project-overrides) overrides are labeled
`configuration`. The JSON output has the same information in each project's
`provenance`, `dependency_provenance`, and `tasks` fields. Task suggestions
carry field-level provenance, so other tools can tell, for example, a
`turbo.json` output declaration from a root task default.

## Providers

| Provider | Detected from                                                     | Project ID                     | Dependency edges from                                                                                           |
| -------- | ----------------------------------------------------------------- | ------------------------------ | --------------------------------------------------------------------------------------------------------------- |
| Cargo    | A `[workspace]` table in the root `Cargo.toml`                    | `cargo:<package.name>`         | `path` dependencies, including `workspace = true` declarations                                                  |
| uv       | A `[tool.uv.workspace]` table in the root `pyproject.toml`        | `uv:<normalized project.name>` | `[tool.uv.sources]` entries with `workspace = true` or a `path`                                                 |
| Go       | `use` directives in the root `go.work`                            | `go:<module path>`             | None; add them with [project overrides](#project-overrides)                                                     |
| Node.js  | `pnpm-workspace.yaml`, or `workspaces` in the root `package.json` | `node:<package name>`          | `dependencies`, `devDependencies`, `optionalDependencies`, and `peerDependencies` that name a workspace package |

Project IDs come from ecosystem metadata, not from directories, so moving a
project does not change its ID. Every provider parses manifests directly:
`cargo`, `uv`, Python, `go`, and the Node.js package managers do not need to be
installed to build the graph. Running an imported Node.js script does need its
package manager.

A dependency that resolves back to the same project does not create an edge. If
the inferred edges form a cycle, [`mise tasks graph`](/cli/tasks/graph.html)
reports it, and you can break it with a project override.

### Cargo workspaces

mise expands the workspace's `members` patterns, honors `exclude`, and includes
the root package when the workspace manifest also contains `[package]`. Path
dependencies inside the workspace root are included as implicit members, as
Cargo does. A path outside the workspace stays an external dependency and is not
added to the graph. Each package must have a `[package].name`.
`mise tasks graph` reports the package root and the root `Cargo.toml` as the
workspace-definition source.

Edges come from dependencies with a local `path` in the normal, development,
build, and target-specific dependency tables. Renamed dependencies resolve by
their path, and declarations with `workspace = true` take their path from the
root `[workspace.dependencies]` table. Version-only and registry dependencies
are ignored, as are path dependencies outside the workspace or beneath an
excluded path.

### uv workspaces

mise expands the `members` globs and honors `exclude`. The root project is
included when the root `pyproject.toml` has a `[project]` table; a virtual
workspace root without one is not a project. Each project must define
`[project].name`, and mise normalizes equivalent spellings such as
`my_package`, `my.package`, and `my-package` to one ID, `uv:my-package`.

Local directory sources under the monorepo root are also projects, even when
they are excluded from the uv workspace, so path dependencies keep their edges.

mise reads dependencies from `[project].dependencies`, optional dependency
groups, `[dependency-groups]`, and uv's legacy `dev-dependencies`. It adds an
edge only when the matching `[tool.uv.sources]` entry selects a workspace member
with `workspace = true` or points to a project directory in the repository with
`path`. Source declarations in the root apply to workspace members unless a
member overrides that dependency's source. When a source is an array with
environment markers, any local alternative adds the edge, because the graph does
not depend on the platform. Registry, Git, URL, wheel, source archive, and
external workspace sources add neither projects nor edges.

### Go workspaces

mise reads both single `use` directives and `use` blocks. Each listed directory
must contain a `go.mod` with a `module` directive. Modules outside the monorepo
root are ignored, because project roots in the graph are always relative to the
repository.

mise does not infer edges from `require` or `replace`. Those directives describe
module selection, not necessarily the build relationships a task graph needs.
Add the edges that matter with an override:

```toml
[monorepo.projects."go:example.com/acme/api"]
depends_add = ["go:example.com/acme/lib"]
```

### Node.js workspaces

mise finds npm, pnpm, Yarn, and Bun workspace packages from:

- `pnpm-workspace.yaml`
- the `workspaces` array in the root `package.json`
- the single-pattern string form of `workspaces`
- the Yarn Classic object form, `workspaces.packages`

When both files exist, `pnpm-workspace.yaml` defines membership. For pnpm and
Yarn workspaces, a root `package.json` that has a `name` is also a project.
Positive and negative patterns, recursive `**` globs, and brace patterns such as
`packages/{web,api}` are supported. Discovery skips `.git` and `node_modules`
but does not read `.gitignore` or `.ignore` files.

Each package must have a `name` in its `package.json`. `mise tasks graph` also
reports the package root, the workspace-definition file, and the detected
package manager.

A dependency creates an edge when its name exactly matches another workspace
package. Version strings are opaque: `workspace:*`, `catalog:`, `*`, a version
range, or any other form creates the same edge, and mise does not resolve or
compare them. All four dependency kinds count, including development
dependencies. Use `depends`, `depends_add`, or `depends_remove` in a project
override when the build relationship should differ from the manifests.

## Run affected tasks {#affected-tasks}

[`mise run --affected <task>`](/cli/run.html) runs a task only in the projects
that changed, and in the projects that depend on them. A change is a commit
between two Git revisions, or an edit in your working tree:

```sh
# Run build in projects changed by HEAD or by uncommitted edits
mise run --affected build

# Only what you have not committed yet
mise run --affected --affected-uncommitted --affected-untracked build

# Show why each project was selected, without running anything
mise run --affected --affected-explain --dry-run build

# Print the selection as JSON without running tasks
mise run --affected --affected-json build

# Compare explicit revisions
mise run --affected --affected-base origin/main --affected-head HEAD test
```

A bare task name such as `build` means `//...:build`, the task of that name in
every project. mise combines three sources of change:

- **committed**: the changes on the head side of `<base>...<head>` since the
  merge base;
- **uncommitted**: staged and unstaged edits to tracked files, compared with
  `HEAD`;
- **untracked**: new files that your `.gitignore` does not exclude.

The working tree only counts when the head revision is the commit you have
checked out. With a different `--affected-head`, mise compares just the
committed range. In CI the working tree is normally clean, so the result is the
same as the committed range.

Name the sources you want to narrow the selection. Naming any of them leaves
out the rest, and they combine:

| Flag                     | Environment variable        | Counts                     |
| ------------------------ | --------------------------- | -------------------------- |
| `--affected-committed`   | `MISE_AFFECTED_COMMITTED`   | the committed range        |
| `--affected-uncommitted` | `MISE_AFFECTED_UNCOMMITTED` | staged and unstaged edits  |
| `--affected-untracked`   | `MISE_AFFECTED_UNTRACKED`   | untracked, unignored files |

Use `--affected-committed` for a reproducible run that ignores whatever is in
your working tree. `--affected-uncommitted` and `--affected-untracked` fail when
the head revision is not the current checkout.

mise selects:

- the project that owns each changed file (the deepest project root that
  contains it);
- every project, when a changed file belongs to no project or matches
  [`task_config.global_inputs`](/tasks/caching.html#inputs-shared-by-every-task);
- for a changed `pnpm-lock.yaml`, only the Node.js projects whose lockfile
  entries changed, comparing the merge base with your working tree when it
  counts (other lockfiles count as ordinary files);
- every project that depends on a selected project, directly or through others.

Then mise runs the matching tasks in those projects. Their dependencies run as
usual, so a selected task can still run a prerequisite in an unchanged project.

`--affected-explain` prints each selected project with the reasons it was
selected (`changed path`, `workspace-global path`, `lockfile change`, or
`depends on affected project`), then each matched task and its project:

```text
Affected projects (HEAD~1...HEAD):
  node:@acme/ui (packages/ui)
    changed path: packages/ui/index.js
  node:@acme/web (apps/web)
    depends on affected project: node:@acme/ui
Affected tasks:
  node:@acme/web#build
    affected project: node:@acme/web
  node:@acme/ui#build
    affected project: node:@acme/ui
```

`--affected-json` prints the same selection as an object with the `base` and
`head` revisions, the affected `projects` with their roots and reasons, and the
matched `tasks` with their project IDs.

mise picks the revisions in this order:

1. `--affected-base` and `--affected-head`.
2. `MISE_AFFECTED_BASE` and `MISE_AFFECTED_HEAD`.
3. CI metadata. In GitHub Actions, the base is `origin/$GITHUB_BASE_REF`, which
   is set for pull requests, and the head is `$GITHUB_SHA`. In GitLab CI, the
   base is `$CI_MERGE_REQUEST_DIFF_BASE_SHA` or
   `origin/$CI_MERGE_REQUEST_TARGET_BRANCH_NAME`, and the head is
   `$CI_COMMIT_SHA`.
4. `HEAD~1` and `HEAD`.

Both revisions must exist in the clone, so fetch enough history in CI, for
example with `fetch-depth: 0` in `actions/checkout`.

## Node.js package scripts {#node-package-scripts}

mise can import the `scripts` of every Node.js workspace package as tasks, so
packages do not need their own `mise.toml`. Turn it on in the monorepo root
config:

```toml
[settings]
task.auto_infer = ["node"]
```

An imported task is named after the project ID, `#`, and the script name. Its
monorepo path also works, so path patterns select it too:

```sh
mise run 'node:@acme/web#build'
mise run //apps/web:build
mise //...:test
```

The task runs `<package manager> run <script> --` in the package directory and
passes task arguments through. mise picks the package manager from
`pnpm-workspace.yaml`, then the root `packageManager` field, then the lockfile
(`bun.lock` or `bun.lockb`, `pnpm-lock.yaml`, `yarn.lock`, `package-lock.json`
or `npm-shrinkwrap.json`), and falls back to npm. The package manager must be on
`PATH`, for example through `[tools]`. [`mise tasks info`](/cli/tasks/info.html)
shows the package's `package.json` as the task's source.

An explicit mise task at the package's monorepo path replaces the imported
script. Both names then run the explicit task.

## Root task defaults

Use `[monorepo.task_defaults.<name>]` in the root `mise.toml` to give every
project's task of that name the same defaults:

```toml
[monorepo.task_defaults.build]
sources = ["src/**", "package.json"]
outputs = ["dist/**"]
cache = { enabled = true }

[monorepo.task_defaults.test]
env = { NODE_ENV = "test" }
```

The defaults apply to imported tasks such as `node:@acme/web#build` and to
explicit mise tasks such as `//apps/web:build`, in every project of the graph,
every config root, and every project added by an override. They fill only
fields that the task leaves unset.

### Task definition precedence

mise resolves a task definition in two stages. First, an explicit project task
replaces an imported task with the same project and task name, and the imported
task's project-ID name becomes an alias for it.

Then mise fills unset fields in this order, from highest to lowest precedence:

1. The task's own fields, from project configuration or from the provider.
2. A [task template](/tasks/templates.html) named by `extends`, for an explicit
   task that uses one.
3. The matching `[monorepo.task_defaults.<name>]` entry.

Map fields such as `env`, `vars`, and `tools` merge across these layers, and the
higher layer wins for each entry. Collection fields such as `depends`,
`sources`, and `outputs` take the whole value from the highest layer that
defines them. These follow the
[task template merge rules](/tasks/templates.html#merge-semantics), with one
exception: a root task default supplies `usage` only to a task that has none,
while a template named by `extends` adds its usage spec ahead of the task's.

For example, an imported script keeps its own command when the root default
also defines `run`, but it inherits cache inputs and environment entries the
provider did not set. If a project later defines that task explicitly, the
explicit command replaces the script, and a named template fills its missing
fields before the root default does.

## Provider task suggestions

A provider can add task configuration when the ecosystem metadata states it
unambiguously:

- input patterns relative to the project, which become task `sources`;
- output patterns relative to the project, including an explicit declaration
  that a task writes no files;
- whether the [artifact cache](/tasks/caching.html#enable-artifact-caching) is
  enabled;
- task dependencies within the project and `^task` dependencies.

Suggestions are part of the imported task, so they have the same precedence as
the provider's command. A matching explicit task replaces them; otherwise task
templates and root task defaults fill only the fields the provider left unset.
Providers leave a field unset when their metadata is not authoritative, and mise
never guesses outputs or cacheability from a command string.

The Node.js provider reads `inputs`, `outputs`, `cache`, and `dependsOn` from
the `tasks` in the root `turbo.json`. A `<package>#<task>` entry takes precedence
over a plain `<task>` entry. Patterns that mise cannot reproduce exactly, such
as those containing `$TURBO_ROOT$`, are left unset so a task template or root
task default can supply them.

## Upstream task dependencies

Prefix a dependency with `^` to run that task in upstream projects first. A root
task default usually applies this across the workspace:

```toml
[monorepo.task_defaults.build]
depends = ["^build"]
```

Running `node:@acme/web#build` now runs `build` in every project that
`@acme/web` depends on before it builds `@acme/web`. mise follows the whole
project graph, including through projects that have no `build` task, and skips
projects that lack the task. For a config root that is not in the project graph,
`^build` does nothing.

`^` works only in `depends`. mise rejects it in `depends_post` and `wait_for`,
which do not describe prerequisite work. Upstream dependencies work with
imported and explicit tasks and use the same scheduler as other `depends`
entries, with cycle detection, deduplication, parallel execution, and
propagation of [cache keys](/tasks/caching.html#what-goes-into-the-cache-key).

## Project overrides

Use `[monorepo.projects]` in the root `mise.toml` to correct or extend what the
providers inferred. Quote project IDs, because they contain `:`:

```toml
[monorepo.projects."node:@acme/web"]
root = "apps/web"
depends_add = ["custom:docs"]
depends_remove = ["node:@acme/legacy"]

[monorepo.projects."custom:docs"]
root = "docs"
metadata = { kind = "documentation" }
```

| Field            | Effect                                                                                             |
| ---------------- | -------------------------------------------------------------------------------------------------- |
| `remove`         | `true` removes the project and every edge connected to it; it cannot be combined with other fields |
| `root`           | Replaces the inferred root, or adds a project when the ID is new                                   |
| `metadata`       | Replaces the inferred metadata                                                                     |
| `depends`        | Replaces the whole inferred dependency set                                                         |
| `depends_add`    | Adds edges, after any `depends` replacement                                                        |
| `depends_remove` | Removes edges, after any `depends` replacement                                                     |

A new project needs a namespaced ID, such as `custom:docs`, and a `root`. The
same dependency cannot be both added and removed. The final graph must reference
only existing project IDs and must not contain cycles; diagnostics name the
projects involved and the override fields that can repair the graph.
