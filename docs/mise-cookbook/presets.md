---
description: "Write a global mise task that adds tools and tasks to a new project, such as a Python and uv setup."
---

# Project scaffolding tasks

A scaffolding task is a [file task](/tasks/file-tasks.html) in your global
config that sets up a project with `mise use` and `mise tasks add`. To share
task definitions between existing projects without generating files, use
[task templates](/tasks/templates.html).

## Scaffold a Python project {#example-python-preset}

Create the task directory, and an empty global config file if you do not have
one yet:

```sh
mkdir -p ~/.config/mise/tasks/scaffold
touch ~/.config/mise/config.toml
```

mise loads [global tasks](/tasks/task-discovery.html#global-tasks) only when a
global config file such as `~/.config/mise/config.toml` exists. Save this script
as `~/.config/mise/tasks/scaffold/python`:

```bash [~/.config/mise/tasks/scaffold/python]
#!/usr/bin/env bash
#MISE description="Add Python, uv and test tasks to the current project"
#MISE dir="{{cwd}}"
#USAGE arg "[version]" default="3.14" help="Python version"
set -euo pipefail

mise use "python@${usage_version}" uv@latest
mise tasks add sync --description "Sync locked dependencies" -- uv sync --locked
mise tasks add test --description "Run tests" -- uv run --locked pytest
```

<span v-pre>`#MISE dir="{{cwd}}"`</span> makes the task run in the directory where you
invoke it. Without it, a global task runs in your home directory, the global
config root. The directory name becomes the task's namespace, so this file is
the task `scaffold:python`, and the `#USAGE` line gives it an optional
`version` argument that the script reads as `$usage_version`.

Make the script executable:

```sh
chmod +x ~/.config/mise/tasks/scaffold/python
```

## Use it in a new project

```sh
mkdir my-project
cd my-project
mise run scaffold:python
mise exec -- uv init --bare
mise exec -- uv add --dev pytest
```

`mise run scaffold:python 3.13` selects another Python version. The task
installs Python and uv and writes this `mise.toml`:

```toml [mise.toml]
[tools]
python = "3.14"
uv = "latest"

[tasks.sync]
description = "Sync locked dependencies"
run = "uv sync --locked"

[tasks.test]
description = "Run tests"
run = "uv run --locked pytest"
```

uv then writes `pyproject.toml`, `uv.lock` and `.venv`. Add your code and tests,
then run `mise run test`. After cloning, teammates run `mise run sync` to install
the locked dependencies. Commit `mise.toml`, `pyproject.toml` and `uv.lock`, and
add `.venv/` to `.gitignore`.

In a project that already has a `mise.toml`, the task adds to it, but
`mise tasks add` replaces a task with the same name and `mise use` replaces the
existing `python` and `uv` versions. Run `mise tasks ls` first to check whether
the project already defines `sync` or `test`.
