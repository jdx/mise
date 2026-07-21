# Presets

You can create your own presets by leveraging [mise tasks](../tasks/index.md) to reduce boilerplate and make it easier to set up new projects.

## Example python preset

Here is an example of how to create your python preset that creates a `mise.toml` file to work with `python` and `pdm`

```shell [~/.config/mise/tasks/preset/python]
#!/usr/bin/env bash
#MISE dir="{{cwd}}"

mise use pre-commit
mise config set env._.python.venv.path .venv
mise config set env._.python.venv.create true -t bool
mise tasks add lint -- pre-commit run -a
```

```shell [~/.config/mise/tasks/preset/pdm]
#!/usr/bin/env bash
#MISE dir="{{cwd}}"
#MISE depends=["preset:python"]
#USAGE arg "<version>"

mise use python@${usage_version?}
mise use pdm@latest
mise config set hooks.postinstall "pdm sync"
```

Then in any directory, you can run `mise preset:pdm 3.10` to scaffold a new project with `python` and `pdm`:

```shell
cd my-project
mise preset:pdm 3.10
# [preset:python] $ ~/.config/mise/tasks/preset/python
# mise WARN  No untrusted config files found.
# mise ~/my-project/mise.toml tools: pre-commit@4.0.1
# [preset:pdm] $ ~/.config/mise/tasks/preset/pdm 3.10
# mise WARN  No untrusted config files found.
# mise ~/my-project/mise.toml tools: python@3.10.15
# mise ~/my-project/mise.toml tools: pdm@2.21.0
# mise creating venv with uv at: ~/my-project/.venv
# Using CPython 3.10.15 interpreter at: /Users/simon/.local/share/mise/installs/python/3.10.15/bin/python
# Creating virtual environment at: .venv
# Activate with: source .venv/bin/activate.fish

~/my-project via 🐍 v3.10.15 (.venv)
# we are in the virtual environment ^
```

Here is the generated `mise.toml` file:

```toml [mise.toml]
[tools]
pdm = "latest"
pre-commit = "latest"
python = "3.10"

[hooks]
postinstall = "pdm sync"

[env]
[env._]
[env._.python]
[env._.python.venv]
path = ".venv"
create = true

[tasks.lint]
run = "pre-commit run -a"
```
