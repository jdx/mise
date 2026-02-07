# Task Templates

::: warning
This feature is experimental and requires `experimental = true` in your settings.
:::

Task templates allow you to define reusable task definitions that can be extended by multiple tasks. This is particularly useful in monorepos or projects with similar task patterns across different components.

## Defining Templates

Templates are defined in the `[task_templates.*]` section of your `mise.toml`:

```toml
[settings]
experimental = true

[task_templates."python:build"]
description = "Build a Python project"
run = "uv build"
tools = { python = "3.12", uv = "latest" }
env = { PYTHONPATH = "src" }

[task_templates."python:test"]
description = "Run Python tests"
run = "pytest"
tools = { python = "3.12" }
depends = ["build"]
```

## Extending Templates

Tasks can extend templates using the `extends` field:

```toml
[tasks.build]
extends = "python:build"

[tasks.test]
extends = "python:test"
run = "pytest --cov"  # Override run while keeping tools, depends
```

## Template Naming

Templates use colon (`:`) separators for namespacing, similar to task naming conventions in monorepos:

- `python:build`
- `python:test`
- `rust:cargo:build`
- `node:npm:test`

## Merge Semantics

When a task extends a template, fields are merged according to these rules:

| Field                                   | Behavior                                                    |
| --------------------------------------- | ----------------------------------------------------------- |
| `run`, `run_windows`                    | Local overrides completely                                  |
| `tools`                                 | Deep merge (local tools added/override template)            |
| `env`                                   | Deep merge (local env added/override template)              |
| `depends`, `depends_post`, `wait_for`   | Local overrides completely (not merged)                     |
| `dir`                                   | Local overrides; defaults to config_root if not in template |
| `sources`, `outputs`                    | Local overrides completely                                  |
| `description`, `shell`, `timeout`, etc. | Local overrides template (if set)                           |
| `quiet`, `hide`, `raw`                  | Not carried over (must be set explicitly in task)           |

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

Templates support Tera templating, rendered with the **using project's context**:

```toml
[task_templates."python:build"]
description = "Build Python project"
dir = "{{ config_root }}"  # Resolves to the PROJECT's directory
run = "uv build"
env = { PROJECT = "{{ config_root | basename }}" }
```

Available variables (same as regular tasks):

- <code v-pre>{{ config_root }}</code> - The project using the template (NOT where template is defined)
- <code v-pre>{{ env.VAR }}</code> - Environment variables
- <code v-pre>{{ cwd }}</code> - Current working directory
- <code v-pre>{{ vars.* }}</code> - User-defined variables from config

## Monorepo Usage

Task templates are especially useful in monorepos where multiple packages share similar build patterns:

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
run = "pytest --cov"  # Add coverage

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

## Future Enhancements

The following features are planned for future releases:

- **Global templates**: Define templates in `~/.config/mise/config.toml` for use across all projects
- **Template packages**: Import templates from external sources
- **Pattern-matching rules**: Auto-apply templates based on file detection (e.g., auto-apply `python:*` templates when `pyproject.toml` exists)
- **File task templates**: Define templates as standalone script files, similar to [file tasks](/tasks/file-tasks)
