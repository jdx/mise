# Presets

A preset is a task you write to create a project's starting configuration. Store
it as a [global file task](/tasks/file-tasks.html) so it is available in new
repositories. For reuse within an existing project, [task templates](/tasks/templates.html)
may be a better fit: they share task definitions without generating files.

## Example python preset

This Bash task writes a Python and uv config into the directory where you invoke
it. It refuses to replace an existing `mise.toml` and leaves project initialization
and dependency installation as explicit next steps.

Create the task directory:

```sh
mkdir -p ~/.config/mise/tasks/preset
```

Then save the following as `~/.config/mise/tasks/preset/python`:

```bash [~/.config/mise/tasks/preset/python]
#!/usr/bin/env bash
#MISE description="Create a Python and uv project config"
#MISE dir="{{cwd}}"
set -euo pipefail

if [[ -e mise.toml ]]; then
  echo "mise.toml already exists; merge the preset manually" >&2
  exit 1
fi

cat > mise.toml <<'TOML'
[tools]
python = "3.12"
uv = "latest"

[tasks.sync]
description = "Sync the project's locked dependencies"
run = "uv sync --locked"

[tasks.test]
description = "Run tests from the project environment"
run = "uv run --locked pytest"
TOML

echo "Created mise.toml"
```

Make the task executable on Unix:

```sh
chmod +x ~/.config/mise/tasks/preset/python
```

Then run it from an empty project directory:

```sh
mkdir my-project
cd my-project
mise run preset:python
mise exec -- uv init --bare
mise exec -- uv add --dev pytest
```

The preset creates `mise.toml`; uv creates the project manifest and lockfile.
Add your application and tests, then run `mise run test`. Teammates can run
`mise run sync` after cloning to install the locked dependencies. Commit
`mise.toml`, `pyproject.toml`, and `uv.lock`, and ignore `.venv/`.

`#MISE dir="{{cwd}}"` matters for a global task: it makes the task write into the
invocation directory instead of the global task's config root. Adapt the script
before using it in repositories with existing configuration or another package
manager.
