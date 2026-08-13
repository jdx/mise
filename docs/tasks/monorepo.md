# Monorepo Tasks

mise supports monorepo-style task organization with target path syntax. This feature allows you to manage tasks across multiple projects in a single repository, where each project can have its own `mise.toml` configuration with tools, environment variables, and tasks that may be different from where the task is called from.

## Overview

When `monorepo_root` is enabled in your root `mise.toml`, mise will automatically discover tasks in subdirectories and prefix them with their relative path from the monorepo root. This creates a unified task namespace across your entire repository.

::: tip
The directory containing a `mise.toml` file is called the **config_root**. In monorepo mode, each project can have its own config_root with its own configuration, separate from the monorepo root. Note that if you use one of the alternate paths in a subdirectory like `./projects/frontend/.mise/config.toml`, the config_root will be `./projects/frontend`–not `./projects/frontend/.mise`.
:::

### Benefits

- **Consistent execution**: Run tasks from any location in the monorepo using the mise config that would be set if called from the task's directory
- **Clear task namespacing**: Tasks are prefixed with their location from the monorepo root
- **Pattern-based execution**: Use wildcards to run tasks across multiple projects
- **Tool and environment layering**: Subdirectory tasks use tools and environment variables from parent configs, but can also define their own in their config_root
- **Automatic trust propagation**: When the monorepo root is trusted, all descendant configs are automatically trusted

## Configuration

### Enabling Monorepo Mode

Add `monorepo_root = true` to your root `mise.toml`:

```toml
# /myproject/mise.toml
monorepo_root = true

[tools]
# Tools defined here apply to all subdirectories
node = "20"
```

### Example Structure

```
myproject/
├── mise.toml (with monorepo_root = true)
├── projects/
│   ├── frontend/
│   │   └── mise.toml (with tasks: build, test)
│   └── backend/
│       └── mise.toml (with tasks: build, test)
```

With this structure, tasks will be automatically namespaced:

- `//projects/frontend:build`
- `//projects/frontend:test`
- `//projects/backend:build`
- `//projects/backend:test`

## Task Path Syntax

Monorepo tasks use special path syntax with `//` and `:` prefixes. You can run these tasks directly with `mise` or with `mise run`. With non-monorepo tasks, the guidance is to avoid using the direct syntax for scripts because it could conflict with future core mise commands. However, mise will never define commands with a `//` or `:` prefix, so this guidance does not apply to monorepo tasks.

```bash
# Direct syntax (preferred for monorepo tasks)
mise //projects/frontend:build

# Also works with 'run'
mise run //projects/frontend:build

# Need quotes for wildcards
mise '//projects/frontend:*'
```

### Absolute Paths

Use `//` prefix to specify the absolute path from the monorepo root:

```bash
# Run build task in frontend project
mise //projects/frontend:build

# Run test task in backend project
mise //projects/backend:test
```

### Current config_root Tasks

Use `:` prefix to run tasks in the current config_root:

```bash
cd projects/frontend
mise :build  # Runs the build task from frontend's config_root
```

This works from any directory below a config_root, not just the config_root itself. The
task name resolves to the nearest enclosing config_root, so `cd projects/frontend/src/components
&& mise :build` also runs frontend's `build`. If no config_root encloses the current directory,
the name resolves against the monorepo root.

::: tip Optional Colon Syntax
The leading `:` is optional when running tasks from subdirectories or defining task dependencies. While both syntaxes work, **we encourage using the `:` prefix to be explicit** about monorepo task references.

**Running from subdirectory:**

```bash
cd projects/frontend
mise :build      # Recommended: Explicit monorepo task reference
mise build       # Also works (for migration compatibility)
```

**Task dependencies:**

```toml
# projects/frontend/mise.toml
[tasks.lint]
run = "eslint ."

[tasks.build]
depends = [":lint"]  # Recommended: Explicit and clear
# OR
depends = ["lint"]   # Also works (for migration compatibility)
run = "webpack build"
```

Dependency paths beginning with `./` are resolved relative to the task that
declares them. This makes it possible to reuse the same dependency declaration
at different levels of a monorepo:

```toml
[tasks.test]
depends = [{ task = "./...:groups:tests:*", optional = true }]
```

For example, when declared by `//apps/frontend:test`, this pattern resolves to
`//apps/frontend/...:groups:tests:*` and matches the current project and its
descendants without matching sibling projects.

The bare name syntax (without `:`) is supported primarily to ease migration from non-monorepo to monorepo configurations. When migrating, you won't need to update all your task dependencies immediately - they'll continue to work. However, using the `:` prefix makes it clear you're referencing a task in the current config_root.
:::

### Wildcard Patterns

mise supports two types of wildcards for flexible task execution:

#### Path Wildcards (`...`)

Use ellipsis (`...`) to match any directory depth:

```bash
# Run 'test' task in ALL projects (any depth)
mise //...:test

# Run 'build' in all subdirs under projects/
mise //projects/...:build

# Match paths with wildcards in the middle
mise //projects/.../api:build  # Matches projects/*/api and projects/*/*/api
```

::: info
Additional glob patterns may be added in a future version so `mise //projects/*:build`
and `mise '//projects/**:build'` will likely be supported. We're using `...` because it matches
how bazel and buck2 do it.
:::

#### Task Name Wildcards (`*`)

Use asterisk (`*`) to match task names:

```bash
# Run ALL tasks in frontend project
mise '//projects/frontend:*'

# Run all tasks starting with 'test:'
mise '//projects/frontend:test:*'

# Run 'lint' task across all projects
mise //...:lint
```

### Combining Wildcards

You can combine both types of wildcards for powerful patterns:

```bash
# Run all tasks in all projects (idk why you'd ever want to do this, but you can)
mise '//...:*'

# Run all test tasks in all projects
mise '//...:test*'

# Run build tasks in all frontend-related projects
mise //.../frontend:build
```

## Tool, Environment, and Vars Layering

Subdirectory tasks automatically use tools and environment variables from parent config files in the hierarchy. However, each subdirectory can also define its own tools and environment variables in its config_root. This allows you to:

1. Define common tools and environment at the monorepo root
2. Override tools or environment in specific subdirectories
3. Add additional tools or environment in subdirectories

`vars` follow the same hierarchy for task templating, so child config vars are available when
running subdirectory tasks from the monorepo root.

Task templates like <span v-pre>`sources = ["{{env.SRC_DIR}}/*"]`</span> are rendered with env from the
task's own config hierarchy, so a subproject's `[env]` section applies no matter where the task
is invoked from.

Child `task_config.includes` templates can also reference inherited vars, which is useful for
centralized task includes like <span v-pre>`git::https://example.com/tasks.git//go.toml?ref={{vars.central_ref}}`</span>.

### Layering Example

```toml
# /myproject/mise.toml
monorepo_root = true

[tools]
node = "20"      # Available to all subdirectories
python = "3.12"  # Available to all subdirectories

[env]
LOG_LEVEL = "info"  # Available to all subdirectories
```

```toml
# /myproject/projects/frontend/mise.toml
[tools]
node = "18"  # Overrides the root's node 20

[env]
LOG_LEVEL = "debug"  # Overrides the root's LOG_LEVEL
PORT = "3000"        # Adds new environment variable

[tasks.build]
run = "npm run build"  # Uses node 18 and LOG_LEVEL=debug
```

```toml
# /myproject/projects/backend/mise.toml
# No tools or env section - uses node 20, python 3.12, and LOG_LEVEL=info from root

[tasks.build]
run = "npm run build"  # Uses node 20 and LOG_LEVEL=info from root
```

### Layering Rules

1. **Base toolset and environment**: Tasks start with tools and environment from all global config files (including parent configs in the hierarchy)
2. **Subdirectory override**: Tools and environment defined in the subdirectory's config file are merged on top, allowing overrides
3. **Task-specific tools and environment**: Values defined in the task's `tools` and `env` properties take highest precedence

## Tools

Use `mise install --monorepo` to install the union of tools from every directory listed in `[monorepo].config_roots`. This is useful in CI when you want to warm a cache for all projects in the repository:

```bash
MISE_ENV=ci mise install --monorepo
```

Passing a tool name filters the union while preserving multiple configured versions:

```bash
mise install --monorepo node
```

`mise ls --monorepo` lists the same union and can be used to inspect cache keys or debug which config roots are contributing tools. Both commands require `monorepo_root = true` and explicit `[monorepo].config_roots`.

## Lockfiles

Monorepos can use one lockfile at the monorepo root. Tools from `packages/api/mise.toml` write to `<monorepo_root>/mise.lock`, while environment and local variants write to root files such as `mise.ci.lock` and `mise.local.lock`.

This is rolling out as a tri-state setting. During the rollout window, unset keeps today's per-subproject lockfile behavior. Set `lockfile = true` to opt into root lockfiles now:

```toml
[monorepo]
lockfile = true
```

If mise finds old subproject lockfiles, it migrates them into the root lockfile the next time a lock-aware command runs. Root entries win on conflicts, unique subproject entries are preserved, and migrated subproject lockfiles are removed.

To keep lockfiles next to each subproject config after the default changes, pin the old behavior in the monorepo root:

```toml
[monorepo]
lockfile = false
```

Unset monorepos that use `mise*.lock` files will start warning in mise `2026.12.0` and will default to root lockfiles in mise `2027.6.0`. Older mise versions do not understand unified monorepo lockfiles for subproject-owned tools. Teams that need mixed-version compatibility should use `lockfile = false` until everyone has upgraded.

## Config Roots

You must explicitly list your config roots using the `[monorepo]` section:

```toml
# /myproject/mise.toml
monorepo_root = true

[monorepo]
config_roots = [
    "packages/frontend",
    "packages/backend",
    "services/*",          # Single-level glob pattern
]
```

This tells mise exactly which directories contain project configurations. Benefits:

- **Fast discovery**: No filesystem walking needed
- **Explicit control**: Only the projects you list are included
- **Glob support**: Use `*` for single-level patterns (e.g., `services/*` matches `services/api`, `services/worker`)

::: tip
Single-level globs (`*`) are supported, but recursive globs (`**`) are not. This ensures predictable performance while still allowing flexible patterns.
:::

::: warning Automatic Discovery Deprecated
Automatic filesystem walking to discover monorepo subdirectories is deprecated. If you don't define `[monorepo].config_roots`, mise will still walk the filesystem but will emit a deprecation warning. Please migrate to explicit config roots.
:::

### Nested Monorepo Roots

When more than one config in the hierarchy sets `monorepo_root = true`, the **nearest** one wins. This comes up with git worktrees checked out inside the main checkout:

```
myproject/mise.toml                       # monorepo_root = true
myproject/packages/api/mise.toml
myproject/.worktrees/feature-x/mise.toml  # monorepo_root = true (same repo, other branch)
myproject/.worktrees/feature-x/packages/api/mise.toml
```

From inside `myproject/.worktrees/feature-x`, that directory is the monorepo root: `//packages/api:build` resolves to the worktree's copy, `{{config_root}}` points inside the worktree, and the worktree's own `[monorepo].config_roots` are the ones expanded.

Tasks from the **enclosing** monorepo are not loaded. They belong to a different monorepo's task set rather than to a parent namespace of the selected root, so loading them would place them outside the `//` namespace — you'd see `build` from the main checkout sitting next to `//:build` from the worktree. Everything above the enclosing root (your global config, a `$HOME/mise.toml`) is unaffected and still contributes tasks as usual.

The enclosing config is still an ancestor config for **tools, environment variables, and vars**, which inherit the same way any parent config's would. If you don't want that either, keep worktrees outside the main checkout (e.g. `myproject-worktrees/feature-x`).

## Workspace Project Graph (Experimental)

mise can infer a provider-neutral project graph from ecosystem workspace metadata. This graph is separate from config-root task discovery: a project does not need its own `mise.toml` to appear in the graph.

Enable experimental features and mark the repository root:

```toml
# /myproject/mise.toml
experimental = true
monorepo_root = true
```

Inspect the inferred projects with:

```bash
mise tasks graph
mise tasks graph --explain
mise tasks graph --json
```

Use `--explain` to see which workspace provider inferred each project, dependency edge, and task.
When a provider suggests task inputs, outputs, cacheability, or dependencies, the explanation also
shows the provider and ecosystem metadata file for each suggested field. Values introduced by
`[monorepo.projects]` overrides are labeled `configuration` instead of being attributed to a
provider.

The JSON output includes the same information in each project's `provenance`,
`dependency_provenance`, and `tasks` fields. Task suggestions contain field-level provenance so
other tooling can distinguish, for example, a `turbo.json` output declaration from a root mise task
default.

### Affected Tasks

Use `mise run --affected <task-pattern>` to run a task only in projects changed between two Git
revisions. mise selects projects that own changed paths, then follows reverse project dependencies
so downstream projects are included. Workspace-global paths and `task_config.global_inputs` select
the whole workspace. Providers may narrow lockfile changes to the projects whose external
dependencies changed.

```bash
# Compare HEAD to its first parent and run affected build tasks
mise run --affected build

# Inspect the selection while preserving normal dry-run behavior
mise run --affected --affected-explain --dry-run build

# Emit the selection as JSON without running tasks
mise run --affected --affected-json build

# Compare explicit revisions
mise run --affected --affected-base origin/main --affected-head HEAD test
```

`--affected-explain` lists each selected project and its cause: an owned changed path, a
workspace-global path, a provider-attributed lockfile, or a dependency on another affected project.
It also lists the task-pattern matches associated with those projects. Normal task dependencies are
expanded afterward, so a selected task can still run a required prerequisite from an unchanged
project.

`--affected-json` emits the same pre-expansion selection without running tasks. Its stable JSON
object contains the base and head revisions, affected projects with their roots and reasons, and
task-pattern matches with their associated project IDs.

The revision defaults are `HEAD~1` and `HEAD` locally. `MISE_AFFECTED_BASE` and
`MISE_AFFECTED_HEAD` override them. GitHub Actions and GitLab merge-request metadata provide CI
defaults when those variables are not set; explicit CLI options take highest precedence.

### Cargo Workspace Discovery

The Cargo provider discovers packages when the root `Cargo.toml` contains a `[workspace]` table.
It expands the workspace's `members` patterns, honors `exclude`, and includes the root package when
the workspace manifest also contains `[package]`. Path dependencies inside the workspace root are
included as implicit members, matching Cargo's workspace membership behavior. A path outside the
workspace remains an external dependency and is not added to the graph.

Each discovered package must have a `[package].name`. mise uses that stable ecosystem identity to
create an ID such as `cargo:my-crate`; moving the crate to another directory does not change its ID.
`mise tasks graph` also reports the package root and `Cargo.toml` as the workspace-definition source.
Discovery parses manifests directly and does not require the `cargo` executable to be installed.

### Cargo Dependency Inference

For every discovered Cargo package, mise infers internal edges from dependencies with a local
`path`. Normal, development, build, and target-specific dependency tables all participate. Renamed
dependencies are resolved by their path, and declarations with `workspace = true` inherit paths
from the root `[workspace.dependencies]` table.

Version-only and registry dependencies are ignored, as are path dependencies outside the workspace
or beneath an excluded path. Declarations that resolve back to the same package do not create a
self-edge. If the inferred internal edges produce a cycle, `mise tasks graph` reports the cycle;
project overrides can replace or adjust the inferred dependencies when needed.

### uv Workspace Discovery

The uv provider discovers Python projects when the root `pyproject.toml` contains a
`[tool.uv.workspace]` table. The root project is always included, and mise expands `members` globs
and honors `exclude` for the remaining workspace members. Each project must define
`[project].name`; mise normalizes equivalent Python package spellings such as `my_package`,
`my.package`, and `my-package` to a stable ID such as `uv:my-package`.

Local directory sources under the configured monorepo root are also represented as projects, even
when they are excluded from uv workspace membership. This preserves dependency edges for uv's
path-dependency alternative to workspaces. Local metadata is parsed directly, so neither `uv` nor
Python needs to be installed for graph discovery.

### uv Dependency Inference

mise reads dependencies from `[project].dependencies`, optional dependency groups,
`[dependency-groups]`, and uv's legacy `dev-dependencies`. An internal edge is added only when the
corresponding `[tool.uv.sources]` entry selects a workspace member with `workspace = true` or points
to an in-repository project directory with `path`. Root source declarations apply to workspace
members unless a member overrides that dependency's source.

Source arrays with environment markers are treated conservatively: any local alternative adds the
edge because the graph is platform-independent. Registry, Git, URL, wheel, source archive, and
external-workspace sources do not add projects or edges. Self-dependencies are ignored, while
cycles among projects are reported by `mise tasks graph` and can be corrected with project
overrides.

### Go Workspace Discovery

The Go provider discovers modules listed by `use` directives in the root `go.work`. Both individual
directives and `use` blocks are supported. Each listed directory must contain a `go.mod` with a
`module` directive; mise uses that stable module path to create an ID such as
`go:example.com/acme/api`. Modules listed outside the configured monorepo root are ignored because
project roots in the mise graph are always repository-relative.

Discovery parses `go.work` and `go.mod` directly and does not require the `go` executable. It does
not infer dependency edges from `require` or `replace`: those directives describe module selection,
not necessarily the source-level relationship needed by a task graph. Add the edges that matter to
your build with project overrides:

```toml
[monorepo.projects."go:example.com/acme/api"]
depends_add = ["go:example.com/acme/lib"]
```

Use `depends` to replace the complete dependency set, or `depends_add` and `depends_remove` for
targeted adjustments. The graph explanation attributes these configured edges to `configuration`.

### Node Workspace Discovery

The Node provider discovers npm, pnpm, Yarn, and Bun workspace packages from:

- `pnpm-workspace.yaml`
- the `workspaces` array in the root `package.json`
- the single-pattern string form of `workspaces`
- the Yarn Classic object form, `workspaces.packages`

When both files exist, `pnpm-workspace.yaml` defines membership. For pnpm and detected Yarn workspaces, a valid root `package.json` with a `name` is implicitly included. Positive and negative patterns, recursive `**` globs, and brace patterns such as `packages/{web,api}` are supported for Node workspace discovery. Discovery skips `.git` and `node_modules`, but does not apply Git ignore files or `.ignore` files.

Each discovered package must have a `name` in its `package.json`. mise uses that stable ecosystem identity to create an ID such as `node:@acme/web`; moving the package to another directory does not change its ID. `mise tasks graph` also reports the package root, workspace-definition source, and detected package manager.

### Node Dependency Inference

For every discovered Node package, mise checks these `package.json` fields:

- `dependencies`
- `devDependencies`
- `optionalDependencies`
- `peerDependencies`

When a declared dependency name exactly matches another discovered workspace package, mise adds an edge to that package's stable `node:` project ID. External package names and declarations that refer back to the same project are ignored.

Dependency version strings are treated as opaque. A matching internal name creates the same edge whether its value uses `workspace:*`, `catalog:`, `*`, a normal version range, or another package-manager-specific form. mise does not resolve or compare those values when constructing the project graph.

All four dependency kinds participate in the same project graph, including development dependencies. If the declarations produce a cycle, `mise tasks graph` reports the cycle instead of silently dropping an edge. Use `depends`, `depends_add`, or `depends_remove` in a project override when the inferred build relationship needs to differ from the package manifests.

### Node Package Scripts

When task inference and experimental features are enabled, mise imports scripts from each
discovered Node workspace package as tasks. Packages do not need their own `mise.toml`.

An imported task uses the stable project ID followed by `#` and the package script name:

```bash
mise run 'node:@acme/web#build'
```

The equivalent monorepo path is available as an alias, so existing path patterns also work:

```bash
mise run //apps/web:build
mise //...:test
```

The task runs in the package directory through the workspace package manager (`npm`, `pnpm`,
`yarn`, or `bun`) and passes task arguments through to it. mise uses the root `packageManager`
declaration or lockfile to select the manager and falls back to npm when neither identifies one.
`mise task info` reports the package's `package.json` as the task source.

An explicit mise task at the package's monorepo path takes precedence over the imported script.
Both names continue to resolve to that explicit task.

This inference is opt-in, currently experimental, and only runs for a configured monorepo root:

```toml
[settings]
experimental = true
task.auto_infer = ["node"]
```

### Root Task Defaults

Use `[monorepo.task_defaults.<name>]` in the root `mise.toml` to define shared defaults for
tasks with the same name in every workspace project:

```toml
[monorepo.task_defaults.build]
sources = ["src/**", "package.json"]
outputs = ["dist/**"]
cache = { enabled = true }

[monorepo.task_defaults.test]
env = { NODE_ENV = "test" }
```

These defaults apply to both provider-inferred tasks such as `node:@acme/web#build` and explicit
mise tasks such as `//apps/web:build`. Task-local configuration takes precedence. When an explicit
task uses `extends`, its template also takes precedence over the root default.

Root task defaults are experimental and are ignored unless experimental features are enabled.

### Task Definition Precedence

Task definitions are resolved in two stages. First, an explicit project task replaces a
provider-inferred task with the same project and task name. The provider task's project-ID name is
kept as an alias for the explicit task, so either name runs the explicit definition.

After selecting the task, mise fills unset fields in this order, from highest to lowest precedence:

1. The selected task's own fields, whether they came from project-local configuration or provider
   inference
2. A task template named by `extends`, for explicit tasks that use one
3. A matching `[monorepo.task_defaults.<name>]` definition from the workspace root

Map fields such as `env`, `vars`, and `tools` merge across these layers, with entries from the
higher-precedence layer winning. Collection fields such as `depends`, `sources`, and `outputs` use
the complete value from the highest-precedence layer that defines them rather than concatenating
values from multiple layers. These are the same merge rules used by [task templates](/tasks/templates).

For example, an inferred package script keeps its provider command when the root default also
defines `run`, while still inheriting cache inputs or environment entries that the provider did not
specify. If a project later defines that task explicitly, the explicit command replaces the package
script; a named template fills its missing fields before the root default does.

### Provider Task Suggestions

Workspace providers can attach task configuration when ecosystem metadata describes it
unambiguously. A provider can suggest:

- project-relative input patterns, which become task `sources`
- project-relative output patterns, including an explicit declaration that a task has no file
  outputs
- whether task output caching is enabled or disabled
- project-relative task dependencies and `^task` dependencies

Suggestions are part of the inferred task definition, so they have the same precedence as the
provider command. A matching explicit project task replaces them. Otherwise, task templates and
root task defaults fill only fields the provider did not suggest. Providers leave fields unset when
their ecosystem metadata is not authoritative; mise does not guess outputs or cacheability from a
command string.

The Node workspace provider reads `inputs`, `outputs`, `cache`, and `dependsOn` from matching
`turbo.json` task definitions. Turbo-specific patterns that mise cannot preserve exactly, such as
`$TURBO_ROOT$`, are left unset so a task template or root task default can supply them instead.

### Upstream Task Dependencies

Prefix a task dependency with `^` to run that task in upstream workspace projects first. A root
task default is the usual way to apply this relationship across the workspace:

```toml
[monorepo.task_defaults.build]
depends = ["^build"]
```

The `^` prefix is supported only in `depends`. It is rejected in `depends_post` and `wait_for`
because those fields do not describe prerequisite work.

Running `node:@acme/web#build` now runs `build` in each project that `@acme/web` depends on before
building `@acme/web`. The relationship follows the complete project dependency graph, including
through intermediate projects that do not define `build`. Missing upstream tasks are skipped.
For a configured task root that is not represented in the detected project graph, the dependency
is a no-op because that task has no upstream project relationship.

Upstream dependencies work with both provider-inferred tasks and explicit mise tasks. They use the
same task scheduler as ordinary `depends`, including cycle detection, deduplication, parallel
execution, and dependency cache-key propagation. This syntax is available only for configured
monorepo workspaces while experimental features are enabled.

### Project Overrides

Use `[monorepo.projects]` in the root `mise.toml` to correct or extend provider inference. Project IDs containing `:` or scoped package names must be quoted:

```toml
[monorepo.projects."node:@acme/web"]
root = "apps/web"
depends_add = ["custom:docs"]
depends_remove = ["node:@acme/legacy"]

[monorepo.projects."custom:docs"]
root = "docs"
metadata = { kind = "documentation" }
```

An override can:

- set `remove = true` to remove an inferred project and its connected edges
- set `root` or `metadata` to replace inferred values
- set `depends` to replace the complete inferred dependency set
- use `depends_add` and `depends_remove` to adjust individual edges
- add a provider-independent project by giving a new namespaced ID an explicit `root`

The final graph must reference existing project IDs and must not contain dependency cycles. Diagnostics identify the affected projects and the override fields that can repair the graph.

## Listing Tasks

The difference between `mise tasks` and `mise tasks --all`:

- **`mise tasks`**: Lists tasks from the current config_root hierarchy (current config_root and its parents)
- **`mise tasks --all`**: Lists tasks from the entire monorepo, including sibling and descendant directories

### Listing Example

Given this structure:

```
myproject/
├── mise.toml (task: deploy)
├── projects/
│   ├── frontend/
│   │   └── mise.toml (tasks: build, test)
│   └── backend/
│       └── mise.toml (tasks: build, serve)
```

When in `projects/frontend/`:

```bash
# Lists: //:deploy, //projects/frontend:build, //projects/frontend:test
mise tasks

# Lists: //:deploy, //projects/frontend:build, //projects/frontend:test,
#        //projects/backend:build, //projects/backend:serve
mise tasks --all
```

### View Specific Project Tasks

```bash
# List all tasks in frontend project
mise tasks '//projects/frontend:*'
```

## Best Practices

### 1. Define Shared Tools and Environment at Root

Place commonly-used tools and environment in the root `mise.toml` to avoid repetition:

```toml
# /myproject/mise.toml
monorepo_root = true

[tools]
node = "20"
python = "3.12"
go = "1.21"

[env]
NODE_ENV = "development"
```

### 2. Override Only When Necessary

Only override tools in subdirectories when they genuinely need different versions:

```toml
# /myproject/legacy-app/mise.toml
[tools]
node = "14"  # Override only for legacy app
# python and go from root
```

### 3. Use Descriptive Task Names

Prefix related tasks with common names to enable pattern matching:

```toml
[tasks.test]
run = "npm test"

[tasks."test:unit"]
run = "npm run test:unit"

[tasks."test:e2e"]
run = "npm run test:e2e"
```

Then run all test tasks: `mise '//...:test*'`

### 4. Group Related Projects

Organize projects in subdirectories to enable targeted execution:

```
myproject/
├── services/
│   ├── api/
│   ├── worker/
│   └── scheduler/
└── apps/
    ├── web/
    └── mobile/
```

Then run tasks by group:

```bash
mise //services/...:build  # Build all services
mise //apps/...:test       # Test all apps
```

## Comparison to Other Tools

The monorepo ecosystem offers many excellent tools, each with different strengths. Here's how mise's Monorepo Tasks compares:

### Simple Task Runners

**Taskfile** and **Just** are fantastic for single-project task automation. They're lightweight and easy to set up, but they weren't designed with monorepos in mind. While you can have multiple Taskfiles/Justfiles in a repo, they don't provide unified task discovery, cross-project wildcards, or automatic tool/environment layering across projects.

**mise's advantage:** Automatic task discovery across the entire monorepo with a unified namespace and powerful wildcard patterns.

### JavaScript-Focused Tools

**Nx**, **Turborepo**, and **Lerna** are powerful tools specifically designed for JavaScript/TypeScript monorepos.

- **Nx** offers incredible features like dependency graph visualization, affected project detection, code generation, and computation caching. It has a massive plugin ecosystem and excels at frontend monorepos.
- **Turborepo** focuses on blazing-fast task caching and parallel execution with minimal configuration.
- **Lerna** pioneered JavaScript monorepo management with package versioning and publishing workflows.

**mise's advantage:** Language-agnostic support. While these tools excel in JS/TS ecosystems, mise works equally well with Rust, Go, Python, Ruby, or any mix of languages. You also get unified tool version management (not just tasks) and environment variables across your entire stack.

### Large-Scale Build Systems

**Bazel** (Google) and **Buck2** (Meta) are industrial-strength build systems designed for massive, multi-language monorepos at companies with thousands of engineers.

- **Bazel** offers incredible features like distributed caching, remote execution, and hermetic builds with fine-grained dependency tracking.
- **Buck2** is a modern rewrite with a clean architecture and impressive performance optimizations.

Both are extremely powerful but come with significant complexity:

- Hermetic builds require strict isolation and complete dependency control
- Steep learning curve with specialized DSLs (Starlark, etc.)
- Complex configuration requiring dedicated build engineers
- Heavy investment in infrastructure for remote caching
- Stricter constraints on how you structure your code

**mise's advantage:** Simplicity through non-hermetic builds. mise doesn't try to control your entire build environment in isolation - instead, it manages tools and tasks in a flexible, practical way. This "non-hermetic" approach means you can use mise without restructuring your entire codebase or learning a new language. You get powerful monorepo task management with simple TOML configuration - enough power for most teams without the enterprise-level complexity that hermetic builds require.

### Other Notable Tools

**Rush** (Microsoft) offers strict dependency management and build orchestration for JavaScript monorepos, with a focus on safety and convention adherence.

**Moon** is a newer Rust-based build system that aims to be developer-friendly while supporting multiple languages.

### The mise Sweet Spot

mise's Monorepo Tasks aims to hit the sweet spot between simplicity and power:

| Feature                 | Simple Runners | JS-Focused | Build Systems | mise |
| ----------------------- | -------------- | ---------- | ------------- | ---- |
| Multi-language support  | ✅             | ❌         | ✅            | ✅   |
| Easy to learn           | ✅             | ⚠️         | ❌            | ✅   |
| Unified task discovery  | ❌             | ✅         | ✅            | ✅   |
| Wildcard patterns       | ❌             | ⚠️         | ✅            | ✅   |
| Tool version management | ❌             | ❌         | ⚠️            | ✅   |
| Environment layering    | ❌             | ⚠️         | ❌            | ✅   |
| Minimal setup           | ✅             | ⚠️         | ❌            | ✅   |
| Task caching            | ❌             | ✅         | ✅            | ❌   |

**When to choose mise:**

- ✅ Polyglot monorepos (multiple languages)
- ✅ You want unified tool + task management
- ✅ You prefer simplicity over maximum performance
- ✅ You're already using mise for tool management

**When to consider alternatives:**

- You're exclusively JavaScript/TypeScript → Nx or Turborepo might offer more JS-specific features
- You're at Google/Meta scale with thousands of engineers → Bazel or Buck2 offer distributed build infrastructure
- You need advanced task caching → Nx, Turborepo, or Bazel offer sophisticated caching systems

The best tool is the one that fits your team's needs. mise's Monorepo Tasks is designed for teams who want powerful monorepo management without the complexity overhead, especially when working across multiple languages.

## Task Templates

For monorepos with similar task patterns across projects, [task templates](/tasks/templates) allow you to define reusable task definitions at the monorepo root:

```toml
# Root mise.toml
[settings]
monorepo_root = true

[task_templates."python:build"]
run = "uv build"
tools = { python = "3.12", uv = "latest" }

[task_templates."python:test"]
run = "pytest"
tools = { python = "3.12" }
depends = ["build"]
```

Projects can then extend these templates:

```toml
# packages/api/mise.toml
[tasks.build]
extends = "python:build"

[tasks.test]
extends = "python:test"
run = "pytest --cov"  # Override with coverage
```

See [Task Templates](/tasks/templates) for complete documentation.

## Related

- [Task Templates](/tasks/templates) - Reusable task definitions
- [Task Configuration](/tasks/task-configuration) - All task configuration options
- [Running Tasks](/tasks/running-tasks) - How to execute tasks
- [Configuration](/configuration) - General mise configuration
