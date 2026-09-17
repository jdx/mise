---
description: "Task templates let you define reusable task definitions that multiple tasks can extend."
---

# Task Templates

Define a task template when several tasks share commands, tools, environment variables, or
dependencies. Each task selects a template with `extends` and supplies the settings that differ.
Run the task by its name; declaring a template alone does not create a runnable task.

The Python examples below assume a uv project whose development dependencies
include `pytest` and, for coverage, `pytest-cov`. Declaring Python as a tool does
not install those project packages.

## Defining Templates

Add a named template under `[task_templates.<name>]` in `mise.toml`:

```toml
[task_templates."python:build"]
description = "Build a Python project"
run = "uv build"
tools = { python = "3.12", uv = "latest" }
env = { PYTHONPATH = "src" }

[task_templates."python:test"]
description = "Run Python tests"
run = "uv run pytest"
tools = { python = "3.12", uv = "latest" }
depends = ["build"]
```

## Extending Templates

Set `extends` to the template name. Fields omitted from the task inherit the template
values; fields set on the task follow the [merge rules](#merge-semantics):

```toml
[tasks.build]
extends = "python:build"

[tasks.test]
extends = "python:test"
run = "uv run pytest --cov"  # Override run while keeping tools, depends
```

[File tasks](/tasks/file-tasks) extend a template from their `#MISE` header:

```bash [mise-tasks/build]
#!/usr/bin/env bash
#MISE extends="python:build"
uv build
```

## Parameterizing a Template with Vars

Put a shared command in the template's `run` field and use `vars` for the values that differ
between tasks. Template fields are merged into each task before rendering, so the inherited
command sees that task's vars.

```toml
[task_templates.e2e]
vars = { mode = "headless" }
run = "echo --mode={{ vars.mode }}"

[tasks.test]
extends = "e2e"
vars = { mode = "headed" }

[tasks."test:ci"]
extends = "e2e"
```

`mise run test` prints `--mode=headed`. `mise run test:ci` inherits the template's default
and prints `--mode=headless`. Replace `echo` with your test command to use the same pattern
for a test suite.

Template vars and task-local vars are combined, with task-local values taking precedence for
the same name. You can also use a Tera fallback in `run`, such as
<span v-pre>`{{ vars.mode | default(value='headless') }}`</span>, when the var is optional.

Keep expressions that depend on task-local vars in the task or its template. A top-level
`[vars]` entry is resolved during config loading; task-local overrides do not recalculate
that stored value. See [when vars are resolved](/configuration/vars.html#when-vars-are-resolved).

## Template Naming

Template names are arbitrary strings. Use colon (`:`) separators to group related templates,
just as you would for task names:

- `python:build`
- `python:test`
- `rust:cargo:build`
- `node:npm:test`

## Merge Semantics

The task inherits omitted fields. When both the template and task set a field, the following
rules apply. “Local” means the value defined on the task.

| Field                                             | Behavior                                                          |
| ------------------------------------------------- | ----------------------------------------------------------------- |
| `run`, `run_windows`                              | Local overrides completely; ignored when the task has a `file`    |
| `tools`                                           | Deep merge (local tools add to or override the template's values) |
| `env`                                             | Deep merge (local env adds to or overrides the template's values) |
| `vars`                                            | Deep merge (local vars add to or override the template's values)  |
| `depends`, `depends_post`, `wait_for`             | Local overrides completely (not merged)                           |
| `dir`                                             | Local overrides; defaults to config_root if not in template       |
| `sources`, `outputs`, `cache`                     | Local overrides completely                                        |
| `usage`                                           | Concatenated: the template's spec, then the task's own            |
| `output`                                          | Local overrides template (if set)                                 |
| Sandbox deny fields                               | Compose with task-local settings                                  |
| Sandbox allow fields                              | Template and task-local values are combined                       |
| `description`, `shell`, `timeout`, etc.           | Local overrides template (if set)                                 |
| `quiet`, `hide`, `raw`, `interactive`, `raw_args` | Not supported on templates (set explicitly on each task)          |

### Empty lists and file tasks

For `run`, `run_windows`, `depends`, `depends_post`, `wait_for`, and `sources`, an
empty local list currently inherits the template's value. In particular,
`depends = []` does not clear template dependencies. Use a separate template when
a task must omit those prerequisites. `outputs = []` is an explicit no-files
output declaration; `cache = { enabled = false }` explicitly disables inherited caching.

A task that sets `file` — including every file task — runs that script rather than
any `run` script, so it never inherits `run` or `run_windows` from a template.

### Example: Deep Merge for Tools

```toml
[task_templates."fullstack:build"]
tools = { python = "3.12", node = "18" }

[tasks.build]
extends = "fullstack:build"
tools = { node = "20" }  # Override node, keep python from template
# Result: tools = { python = "3.12", node = "20" }
```

### Example: Deep Merge for Env

```toml
[task_templates."python:build"]
env = { PYTHONPATH = "src", DEBUG = "0" }

[tasks.build]
extends = "python:build"
env = { DEBUG = "1" }  # Override DEBUG, keep PYTHONPATH from template
# Result: env = { PYTHONPATH = "src", DEBUG = "1" }
```

### Example: Shared Arguments

A template's `usage` spec holds the flags and arguments its tasks have in common.
A task that extends it adds its own; it does not have to repeat the shared ones:

```toml
[task_templates.deploy]
usage = """
flag "--env <env>" help="Target environment"
flag "--dry-run" help="Print what would happen"
"""

[tasks.deploy-api]
extends = "deploy"
usage = 'flag "--replicas <n>" help="How many to run"'
run = 'echo "env=$usage_env replicas=$usage_replicas"'
```

`mise run deploy-api --help` lists `--env`, `--dry-run`, and `--replicas`, in that
order. Declare each flag in one place: a flag written in both the template and the
task is two declarations and appears twice in `--help`.

### Example: Complete Override for Depends

```toml
[task_templates."python:test"]
depends = ["lint", "typecheck"]

[tasks.test]
extends = "python:test"
depends = ["build"]  # Completely replaces template depends
# Result: depends = ["build"] (lint and typecheck NOT included)
```

## Tera Templating

Task templates reuse task definitions; [Tera templates](/templates) evaluate expressions inside
those definitions. Tera expressions use the context of the project that uses the task template,
even when the template is defined in a parent or global config:

```toml
[task_templates."python:build"]
description = "Build Python project"
dir = "{{ config_root }}"  # Resolves to the PROJECT's directory
run = "uv build"
env = { PROJECT = "{{ config_root | basename }}" }
```

Available variables (same as regular tasks):

- <code v-pre>{{ config_root }}</code> - The project using the template (NOT where the template is defined)
- <code v-pre>{{ env.VAR }}</code> - Environment variables
- <code v-pre>{{ cwd }}</code> - Current working directory
- <code v-pre>{{ vars.* }}</code> - Config vars, with template and task-local overrides

## Monorepo Usage

Define shared templates in the monorepo root, then extend them in each package. In this example,
the API package adds coverage to its test command while the worker uses the inherited command:

```toml
# Root mise.toml
monorepo_root = true

[monorepo]
config_roots = ["packages/api", "packages/worker"]

[task_templates."python:build"]
run = "uv build"
tools = { python = "3.12", uv = "latest" }

[task_templates."python:test"]
run = "uv run pytest"
tools = { python = "3.12", uv = "latest" }
depends = ["build"]

[task_templates."python:lint"]
run = "ruff check ."
tools = { python = "3.12", ruff = "latest" }
```

```toml
# packages/api/mise.toml
[tasks.build]
extends = "python:build"

[tasks.test]
extends = "python:test"
run = "uv run pytest --cov"  # Add coverage

[tasks.lint]
extends = "python:lint"
```

```toml
# packages/worker/mise.toml
[tasks.build]
extends = "python:build"

[tasks.test]
extends = "python:test"

[tasks.lint]
extends = "python:lint"
```

## Template scope

Templates come from the active configuration hierarchy, including global and
parent configurations. A task selects one by name with `extends`; declaring a
template does not create a runnable task. Use `mise tasks info <task>` to inspect
the resulting task after inheritance.

Prefer repository-owned templates for behavior teammates and CI need to share.
Global templates are useful for personal tasks, but another machine will need
the same template definition to resolve `extends`.
