---
description: "Share commands, tools, environment variables, and dependencies across tasks by extending a named task template."
socialDescription: "Share commands, tools, and dependencies across tasks with task templates."
---

# Task templates

Define a task template when several tasks share commands, tools, environment
variables, or dependencies. Each task names its template with `extends` and
sets only what differs. A template is not a task: declaring one does not create
anything you can run.

Task templates are different from [Tera templates](/templates.html). A task
template supplies whole task properties that a task inherits; Tera renders
expressions inside a value, including values that come from a task template.

The Python examples assume a uv project with `pytest` and `pytest-cov` in its
dev dependencies.

## Defining templates

Add a named template under `[task_templates.<name>]` in `mise.toml`. A template
accepts the same properties as a task, with the exceptions listed under
[merge semantics](#merge-semantics):

```mise-toml
[task_templates."python:build"]
description = "Build a Python project"
run = "uv build"
tools = { python = "3.14", uv = "latest" }
env = { PYTHONPATH = "src" }

[task_templates."python:test"]
description = "Run Python tests"
run = "uv run pytest"
tools = { python = "3.14", uv = "latest" }
depends = ["build"]
```

Template names are free-form. Group them with `:` the way you group task names,
for example `python:build` or `rust:cargo:build`.

## Extending templates

Set `extends` to the template name. Properties the task omits come from the
template; properties set on both follow the [merge rules](#merge-semantics):

```mise-toml
[tasks.build]
extends = "python:build"

[tasks.test]
extends = "python:test"
run = "uv run pytest --cov" # replaces run, keeps tools and depends
```

[File tasks](/tasks/file-tasks.html) extend a template from their `#MISE`
header:

```bash [mise-tasks/build]
#!/usr/bin/env bash
#MISE extends="python:build"
uv build
```

Run [`mise tasks info <task>`](/cli/tasks/info.html) to see the task that
results from the merge.

## Template scope

mise collects templates from every config file it loads, including global and
parent configs, into one set of names. When several config files define a
template with the same name, the one from the highest-precedence config, the
one nearest the current directory, wins. Every task that names it uses that
definition, including tasks defined in a parent or global config.

Keep templates that teammates and CI need in the repository. A template in your
global config works for personal tasks, but another machine needs the same
definition to resolve `extends`.

## Parameterizing a template with vars

Put a shared command in the template's `run` and use `vars` for the values that
differ between tasks. mise merges the template into each task before it renders
the task, so the inherited command sees that task's vars.

```mise-toml
[task_templates.e2e]
vars = { mode = "headless" }
run = "echo --mode={{ vars.mode }}"

[tasks.test]
extends = "e2e"
vars = { mode = "headed" }

[tasks."test:ci"]
extends = "e2e"
```

`mise run test` prints `--mode=headed`. `mise run test:ci` inherits the
template's default and prints `--mode=headless`.

Template vars and task-local vars are combined, and the task's value wins for
the same name. Use a Tera fallback in `run`, such as
<code v-pre>{{ vars.mode | default(value='headless') }}</code>, when the var is
optional.

Keep expressions that depend on task-local vars in the task or its template. A
top-level `[vars]` entry is resolved during config loading, and task-local
overrides do not recalculate that stored value. See
[when vars are resolved](/configuration/vars.html#when-vars-are-resolved).

## Merge semantics

The task inherits every property it omits. When both the template and the task
set a property, these rules apply:

| Property                                                                                      | Result                                                                                                                          |
| --------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| `run`, `run_windows`                                                                          | The task's value replaces the template's. Ignored when the task or the template sets `file`.                                    |
| `file`                                                                                        | The task's value, else the template's.                                                                                          |
| `tools`, `env`, `vars`                                                                        | Merged by key. The task's entries add to the template's and win for the same key.                                               |
| `depends`, `depends_post`, `wait_for`                                                         | The task's list replaces the template's.                                                                                        |
| `alias`, `sources`, `outputs`                                                                 | The task's value replaces the template's.                                                                                       |
| `dir`                                                                                         | The task's value, else the template's, else `task_config.dir`, else the task's config root.                                     |
| `usage`                                                                                       | Both specs, the template's first.                                                                                               |
| `deny_all`, `deny_read`, `deny_write`, `deny_net`, `deny_env`                                 | On when either sets it. A task cannot turn off a template's restriction.                                                        |
| `allow_read`, `allow_write`, `allow_net`, `allow_env`, `pass_through_env`                     | The template's entries followed by the task's.                                                                                  |
| `description`, `shell`, `timeout`, `output`, `silent`, `confirm`, `cache`, `watch`, `daemons` | The task's value if set, else the template's.                                                                                   |
| `quiet`, `hide`, `raw`, `interactive`, `raw_args`, `extends`                                  | Not accepted in templates; mise warns about an unknown field. Set them on each task. A template cannot extend another template. |
| `secrets`                                                                                     | Not allowed in templates; mise reports a config error.                                                                          |

### Empty lists {#empty-lists-and-file-tasks}

An empty list on the task inherits the template's value for `run`,
`run_windows`, `depends`, `depends_post`, `wait_for`, and `sources`, so
`depends = []` does not clear the template's dependencies. Use a separate
template for tasks that need none. `outputs = []` does declare that the task
writes no files, and `cache = { enabled = false }` turns off inherited caching.

### Example: deep merge for tools

```toml
[task_templates."fullstack:build"]
tools = { python = "3.14", node = "22" }

[tasks.build]
extends = "fullstack:build"
tools = { node = "24" } # overrides node, keeps python
# Result: tools = { python = "3.14", node = "24" }
```

### Example: deep merge for env

```toml
[task_templates."python:build"]
env = { PYTHONPATH = "src", DEBUG = "0" }

[tasks.build]
extends = "python:build"
env = { DEBUG = "1" } # overrides DEBUG, keeps PYTHONPATH
# Result: env = { PYTHONPATH = "src", DEBUG = "1" }
```

### Example: shared arguments

A template's `usage` spec holds the flags and arguments its tasks have in
common. A task that extends it adds its own without repeating the shared ones:

```mise-toml
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

`mise run deploy-api --help` lists `--env`, `--dry-run`, and `--replicas`, in
that order. Declare each flag in one place: a flag written in both the template
and the task is two declarations and appears twice in `--help`.

### Example: complete override for depends

```toml
[task_templates."python:test"]
depends = ["lint", "typecheck"]

[tasks.test]
extends = "python:test"
depends = ["build"] # replaces the template's depends
# Result: depends = ["build"]; lint and typecheck do not run
```

## Tera templating

Tera expressions in a template render in the context of the task that extends
it, even when the template is defined in a parent or global config.
<code v-pre>{{ config_root }}</code> is the config root of that task, not of the
file that defines the template:

```mise-toml
[task_templates."python:build"]
run = "uv build --out-dir {{ config_root }}/dist"
env = { PROJECT = "{{ config_root | basename }}" }
```

Templates use the same variables as regular tasks:

- <code v-pre>{{ config_root }}</code>: the config root of the task that uses
  the template
- <code v-pre>{{ env.VAR }}</code>: environment variables
- <code v-pre>{{ cwd }}</code>: the current working directory
- <code v-pre>{{ vars.NAME }}</code>: config vars, with template and task-local
  overrides

A task template supplies whole properties; it cannot insert a parameterized
snippet into the middle of a task's `run` string. For repeated or nested
snippets inside one script, use
[Tera components](/templates.html#parameterized-snippets-with-components).

## Monorepo usage

Define shared templates in the monorepo root, then extend them in each project.
In this example, the API project adds coverage to its test command while the
worker uses the inherited command:

```mise-toml
# Root mise.toml
monorepo_root = true

[monorepo]
config_roots = ["packages/api", "packages/worker"]

[task_templates."python:build"]
run = "uv build"
tools = { python = "3.14", uv = "latest" }

[task_templates."python:test"]
run = "uv run pytest"
tools = { python = "3.14", uv = "latest" }
depends = ["build"]

[task_templates."python:lint"]
run = "ruff check ."
tools = { python = "3.14", ruff = "latest" }
```

```mise-toml
# packages/api/mise.toml
[tasks.build]
extends = "python:build"

[tasks.test]
extends = "python:test"
run = "uv run pytest --cov" # adds coverage

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

To give every project's `build` task the same defaults without adding
`extends` to each one, use `[monorepo.task_defaults.build]` from the
experimental [workspace project graph](/tasks/workspace-graph.html#root-task-defaults) instead. A
template named by `extends` takes precedence over a root default.
