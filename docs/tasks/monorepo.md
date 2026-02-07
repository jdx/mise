# Monorepo Tasks <Badge type="warning" text="experimental" />

mise supports monorepo-style task organization with target path syntax. This feature allows you to manage tasks across multiple projects in a single repository, where each project can have its own `mise.toml` configuration with tools, environment variables, and tasks that may be different from where the task is called from.

## Overview

When `experimental_monorepo_root` is enabled in your root `mise.toml`, mise will automatically discover tasks in subdirectories and prefix them with their relative path from the monorepo root. This creates a unified task namespace across your entire repository.

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

Add `experimental_monorepo_root = true` to your root `mise.toml`:

```toml
# /myproject/mise.toml
experimental_monorepo_root = true

[tools]
# Tools defined here apply to all subdirectories
node = "20"
```

::: warning
This feature requires `MISE_EXPERIMENTAL=1` environment variable.
:::

### Example Structure

```
myproject/
├── mise.toml (with experimental_monorepo_root = true)
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

## Tool and Environment Layering

Subdirectory tasks automatically use tools and environment variables from parent config files in the hierarchy. However, each subdirectory can also define its own tools and environment variables in its config_root. This allows you to:

1. Define common tools and environment at the monorepo root
2. Override tools or environment in specific subdirectories
3. Add additional tools or environment in subdirectories

### Layering Example

```toml
# /myproject/mise.toml
experimental_monorepo_root = true

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

## Config Roots

You must explicitly list your config roots using the `[monorepo]` section:

```toml
# /myproject/mise.toml
experimental_monorepo_root = true

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
experimental_monorepo_root = true

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
experimental = true
experimental_monorepo_root = true

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
