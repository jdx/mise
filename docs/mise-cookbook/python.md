# Mise + Python Cookbook

Here are some tips on managing [Python](/lang/python.html) projects with mise.

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

## mise + uv

If you are using a `uv` project initialized with `uv init .`, here is how you can use it with mise.

Here is how the `uv` project will look like:

```shell [uv-project]
.
├── .gitignore
├── .python-version
├── main.py
├── pyproject.toml
└── README.md

cat .python-version
# 3.12
```

If you run `uv run main.py` in the `uv` project, `uv` will automatically create a virtual environment for you using the python version specified in the `.python-version` file. This will also create a `uv.lock` file.

`mise` will detect the python version in `.python-version`, however, it won't use the virtual env created by `uv` by default. So, using `which python` will show a global python installation from `mise`.

```shell
mise i
which python
# ~/.local/share/mise/installs/python/3.12.4/bin/python
```

If you want `mise` to use the virtual environment created by `uv`, you can set the [`python.uv_venv_auto`](/lang/python.html#python.uv_venv_auto) setting to `true` in your `mise.toml` file.

```toml [mise.toml]
[settings]
python.uv_venv_auto = true
```

Using `which python` will now show the python version from the virtual environment created by `uv`.

```shell
which python
# ./uv-project/.venv/bin/python
```

Another option is to use `_.python.venv` in your `mise.toml` file to specify the path to the virtual environment created by `uv`.

```toml [mise.toml]
[env]
_.python.venv = { path = ".venv" }
```

### Syncing python versions installed by mise and uv

You can use [mise sync python --uv](/cli/sync/python.html#uv) to sync the python version installed by `mise` with the python version specified in the `.python-version` file in the `uv` project.

### uv scripts

You can take advantage of `uv run` in [`shebang`](/tasks/toml-tasks.html#shell-shebang) in toml or file tasks.
Note that using `--script` is required if the filename does not end in `.py`.

Here is an example toml task:

```toml [mise.toml]
[tools]
uv = 'latest'

[tasks.print_peps]
run = """
#!/usr/bin/env -S uv run --script
# /// script
# dependencies = ["requests<3", "rich"]
# ///

import requests
from rich.pretty import pprint

resp = requests.get("https://peps.python.org/api/peps.json")
data = resp.json()
pprint([(k, v["title"]) for k, v in data.items()][:10])
"""
```

Or as a file task:

```python [mise-tasks/print_peps.py]
#!/usr/bin/env -S uv run --script
# /// script
# dependencies = ["requests<3", "rich"]
# ///

import requests
from rich.pretty import pprint

resp = requests.get("https://peps.python.org/api/peps.json")
data = resp.json()
pprint([(k, v["title"]) for k, v in data.items()][:10])
```

You can then run it with `mise run print_peps`:

```shell
❯ mise run print_peps
[print_peps] $ ~/uv-project/mise-tasks/print_peps.py
Installed 9 packages in 8ms
[
│   ('1', 'PEP Purpose and Guidelines'),
│   ('2', 'Procedure for Adding New Modules'),
    #...
]
```
