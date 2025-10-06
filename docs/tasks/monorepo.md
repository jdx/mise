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
- **Tool and environment inheritance**: Subdirectory tasks inherit tools and environment variables from parent configs, but can also define their own in their config_root
- **Automatic trust propagation**: When the monorepo root is trusted, all descendant configs are automatically trusted

## Configuration

### Enabling Monorepo Mode

Add `experimental_monorepo_root = true` to your root `mise.toml`:

```toml
# /myproject/mise.toml
experimental_monorepo_root = true

[tools]
# Tools defined here are inherited by all subdirectories
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

## Tool and Environment Inheritance

Subdirectory tasks automatically inherit tools and environment variables from parent config files in the hierarchy. However, each subdirectory can also define its own tools and environment variables in its config_root. This allows you to:

1. Define common tools and environment at the monorepo root
2. Override tools or environment in specific subdirectories
3. Add additional tools or environment in subdirectories

### Inheritance Example

```toml
# /myproject/mise.toml
experimental_monorepo_root = true

[tools]
node = "20"      # Inherited by all subdirectories
python = "3.12"  # Inherited by all subdirectories

[env]
LOG_LEVEL = "info"  # Inherited by all subdirectories
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
# No tools or env section - inherits node 20, python 3.12, and LOG_LEVEL=info from root

[tasks.build]
run = "npm run build"  # Uses node 20 and LOG_LEVEL=info from root
```

### Inheritance Rules

1. **Base toolset and environment**: Tasks start with tools and environment from all global config files (including parent configs in the hierarchy)
2. **Subdirectory override**: Tools and environment defined in the subdirectory's config file are merged on top, allowing overrides
3. **Task-specific tools and environment**: Values defined in the task's `tools` and `env` properties take highest precedence

## Performance Tuning

For large monorepos, you can control task discovery depth with the `task.monorepo_depth` setting (default: 5):

```toml
[settings]
task.monorepo_depth = 3  # Only search 3 levels deep
```

This limits how deep mise will search for task files:

- `1` = immediate children only (`monorepo_root/projects/`)
- `2` = grandchildren (`monorepo_root/projects/frontend/`)
- `5` = default (5 levels deep)

Reduce this value if you notice slow task discovery in very large monorepos, especially if your projects are concentrated at a specific depth level.

## Discovery Behavior

### Excluded Paths

The following directories are automatically excluded from task discovery:

- Hidden directories (starting with `.`)
- `node_modules`
- `target`
- `dist`
- `build`

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
# python and go inherited from root
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

## Related

- [Task Configuration](/tasks/task-configuration) - All task configuration options
- [Running Tasks](/tasks/running-tasks) - How to execute tasks
- [Configuration](/configuration) - General mise configuration
