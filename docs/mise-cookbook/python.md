# Mise + Python Cookbook

Here are some tips on managing Python projects with mise.

## A Python Project with virtualenv

Here is an example python project with a `requirements.txt` file.

```toml [mise.toml]
min_version = "2024.9.5"

[env]
# Use the project name derived from the current directory
PROJECT_NAME = "{{ config_root | basename }}"

# Automatic virtualenv activation
_.python.venv = { path = ".venv", create = true }

[tools]
python = "{{ get_env(name='PYTHON_VERSION', default='3.11') }}"
ruff = "latest"

[tasks.install]
description = "Install dependencies"
alias = "i"
run = "uv pip install -r requirements.txt"

[tasks.run]
description = "Run the application"
run = "python app.py"

[tasks.test]
description = "Run tests"
run = "pytest tests/"

[tasks.lint]
description = "Lint the code"
run = "ruff src/"

[tasks.info]
description = "Print project information"
run = '''
echo "Project: $PROJECT_NAME"
echo "Virtual Environment: $VIRTUAL_ENV"
'''
```

## Using tasks to create mise.toml for individual projects

This is intended to be used on each project separately. Via the defined tasks you create a `mise.toml` with different content, depending on the type of python project. Here are examples for "legacy" projects with `requirements.txt`, `uv` and `poetry`.

It will create a `.venv` directory via the `python` preset and additional configuration depending on the other tooling.

### Configuration files

Recommended configuration

```tomo [~/.config/mise/config.toml]
[tools]
python = ["3.12", "3.11", "3.13"]
"pipx:black" = "latest"
"pipx:ruff" = "latest"
watchexec = "latest"

[tools.uv]
version = "latest"
# This is specific to zsh, make sure that you use that directory for completions
postinstall = "uv --generate-shell-completion zsh > $HOME/.local/share/zsh/completions/_uv && autoload -Uz compinit && compinit -u"

# This auto-updates tools and plugins
[hooks]
enter = "mise i -q && mise plugins update"

[settings]
# dont run in parallel in case things collide
jobs = 1
experimental = true

[settings.pipx]
# when using python tools use uvx instead of pipx
uvx = true

[settings.python]
uv_venv_auto = true
```

#### Base for all projects

```sh [~/.config/mise/tasks/preset/python]
!/usr/bin/env bash
#MISE dir="{{cwd}}"
#USAGE arg "<version>"

# Run this to add it to a project:
# mise tasks run preset:python 3.12


touch mise.toml
mise trust
# somehow does not work here: mise use {{arg(name="pyversion", var=true)}}
mise use "python@$usage_version" python@3.13 python@3.12 python@3.11 # change this to fit which python versions you want to develop for

mise use -g watchexec@latest # if you use `mise watch`

mise config set env._.python.venv.path .venv
mise config set env._.python.venv.create true -t bool

mise config set tasks.lint.description "Lint with ruff check"
mise config set tasks.lint.alias "ruff"
mise config set tasks.lint.run "ruff check ."

mise config set tasks.test.description "test with pytest"
mise config set tasks.test.alias "pytest"
mise config set tasks.test.run "pytest tests/"

mise settings set python.uv_venv_auto true
```

#### Projects with requirements.txt

```sh [~/.config/mise/tasks/preset/python-requirementstxt]
#!/usr/bin/env bash
#MISE dir="{{cwd}}"

# does not work with args
#XXMISEXX depends=["preset:python"]

# Run this to add it to a new project:
# mise tasks run preset:python-requirementstxt

touch mise.toml
mise trust

# install latest versions from {tests/,}requirements.txt when entering the project directory and when the requirements.txts change
mise config set hooks.enter 'if [ -e tests/requirements.txt ] ; then extra_file="tests/requirements.txt" ; fi ; uv pip sync requirements.txt $extra_file'

# this is more or less redundant after the above config, but if you want to run it manually you still can
mise config set tasks.install_deps.description "Install dependencies"
mise config set tasks.install_deps.alias "id"
mise config set tasks.install_deps.run 'if [ -e tests/requirements.txt ] ; then extra_file="tests/requirements.txt" ; fi ; uv pip sync requirements.txt $extra_file'
```

After applying this you can use e.g. `mise watch --watch requirements.txt --watch tests/requirements.txt --on-busy-update queue install_deps` to have updated packages in your venv if something changes from your `requirements.txt`.

#### Projects that use uv

```sh [~/.config/mise/tasks/preset/python-pyprojecttoml-uv]
#!/usr/bin/env bash
#MISE dir="{{cwd}}"

# does not work with args
#XXMISEXX depends=["preset:python"]

# Run this to add it to a project:
# mise tasks run preset:python-pyprojecttoml-uv

touch mise.toml
mise trust

# install latest versions from {tests/,}requirements.txt when entering the project directory and when the requirements.txts change
#mise config set hooks.enter "uv pip install -r pyproject.toml  --all-extras"
mise config set hooks.enter "uv pip sync pyproject.toml"

# this is more or less redundant after the above config, but if you want to run it manually you still can
mise config set tasks.install_deps.description "Install dependencies"
mise config set tasks.install_deps.alias "id"
#mise config set tasks.install_deps.run "uv pip install -r pyproject.toml  --all-extras"
mise config set tasks.install_deps.run "uv pip sync pyproject.toml"
```

Optionally run `mise watch --watch pyproject.toml --on-busy-update queue install_deps` as described above.

#### Projects that use poetry

```sh [~/.config/mise/tasks/preset/python-pyprojecttoml-poetry]
#!/usr/bin/env bash
#MISE dir="{{cwd}}"

# does not work with args
#XXMISEXX depends=["preset:python"]

# Run this to add it to a project:
# mise tasks run preset:python-pyprojecttoml-poetry

touch mise.toml
mise trust

mise use poetry[pyproject='pyproject.toml']
# install latest versions from {tests/,}requirements.txt when entering the project directory and when the requirements.txts change
#mise config set hooks.enter "uv pip install -r pyproject.toml  --all-extras"
mise config set hooks.enter "poetry install --sync --all-extras"

# this is more or less redundant after the above config, but if you want to run it manually you still can
mise config set tasks.install_deps.description "Install dependencies"
mise config set tasks.install_deps.alias "id"
#mise config set tasks.install_deps.run "uv pip install -r pyproject.toml  --all-extras"
mise config set tasks.install_deps.run "poetry install --sync --all-extras"
```

Optionally run `mise watch --watch pyproject.toml --on-busy-update queue install_deps` as described above.

### Actual application in a project directory

```sh
# for every project
mise tasks run -vvv preset:python 3.13

# when using requirements.txt
mise tasks run -vvv preset:python-requirementstx
# when using uv
mise tasks run -vvv preset:python-pyprojecttoml-uv
# when using poetry
mise tasks run -vvv preset:python-pyprojecttoml-poetry
```
