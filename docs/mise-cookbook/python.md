# Python Cookbook

Choose a recipe for an existing requirements-based project, a uv project, or a
standalone Python script. See [Python configuration](/lang/python.html) for runtime
installation and virtualenv settings.

## A Python Project with virtualenv

This recipe expects `requirements.txt`, `app.py`, and a `tests/` directory. Include
`pytest` in the requirements used for development. mise creates `.venv`; the
install task populates it with project dependencies.

```toml [mise.toml]
min_version = "2024.9.5"

[env]
# Use the project name derived from the current directory
PROJECT_NAME = "{{ config_root | basename }}"

# Automatic virtualenv activation
_.python.venv = { path = ".venv", create = true }

[tools]
python = "3.12"
uv = "latest"
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
run = "python -m pytest tests/"

[tasks.lint]
description = "Lint the code"
run = "ruff check ."

[tasks.info]
description = "Print project information"
run = '''
echo "Project: $PROJECT_NAME"
echo "Virtual Environment: $VIRTUAL_ENV"
'''
```

Run `mise run install`, then `mise run test`, `mise run lint`, or `mise run run`.
Add `.venv/` to `.gitignore`.

## mise + uv

If you are using a `uv` project initialized with `uv init .`, here is how you can use it with mise.

Here is what the `uv` project looks like:

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

If you run `uv run main.py` in the `uv` project, `uv` automatically creates a virtual environment for you using the Python version specified in the `.python-version` file. It also creates a `uv.lock` file.

Enable `.python-version` discovery if you want mise to select the same Python
version. Declare uv as a tool as well:

```toml [mise.toml]
[tools]
uv = "latest"

[settings]
idiomatic_version_file_enable_tools = ["python"]
```

Run `mise install`, then `mise exec -- uv sync` to create the lockfile, virtualenv,
and project dependencies. By default, mise still selects its managed Python when
you run `mise exec -- python`; `uv run` selects uv's project environment.

To make `mise` use the virtual environment created by `uv`, set the [`python.uv_venv_auto`](/lang/python.html#python.uv_venv_auto) setting in your `mise.toml` file.
Use `"source"` to source only an existing `.venv`, or `"create|source"` to create it if missing and then source it.
If you prefer `mise deps` to create the venv, keep it at `"source"`, enable `[deps.uv]`, and run `mise deps`.

::: tip
`mise` locates the uv project by walking up the directory tree for a `uv.lock` file — that lockfile is how `mise` knows the project uses uv. A `uv.lock` must therefore be present: if none is found (for example, in a fresh project that hasn't been `uv sync`'d yet), the setting does nothing. Run `uv sync` (or `uv lock`) to generate one.
:::

```toml [mise.toml]
[settings]
python.uv_venv_auto = "source"
# or, to create if missing
# python.uv_venv_auto = "create|source"
```

After activation refreshes, `python` resolves to the virtualenv. You can also
check through mise directly:

```shell
mise exec -- python -c 'import sys; print(sys.executable)'
# /path/to/uv-project/.venv/bin/python
```

Another option is to use `_.python.venv` in your `mise.toml` file to specify the path to the virtual environment created by `uv`.

```toml [mise.toml]
[env]
_.python.venv = { path = ".venv" }
```

### Syncing python versions installed by mise and uv

Use [`mise sync python --uv`](/cli/sync/python.html) to make existing Python
installations available across mise and uv. This shares installed runtimes; it
does not update `.python-version`, select the project version, or sync packages.
Use `uv sync` for project dependencies.

### uv scripts

You can use `uv run` in a [`shebang`](/tasks/toml-tasks.html#shell-shebang) in toml or file tasks.
The `--script` flag is required if the filename does not end in `.py`.

Here is an example toml task:

```toml [mise.toml]
[tools]
uv = 'latest'

[tasks.print_peps]
run = '''
#!/usr/bin/env -S uv run --script
# /// script
# dependencies = ["requests<3", "rich"]
# ///

import requests
from rich.pretty import pprint

resp = requests.get("https://peps.python.org/api/peps.json", timeout=30)
resp.raise_for_status()
data = resp.json()
pprint([(k, v["title"]) for k, v in data.items()][:10])
'''
```

Or as a file task:

```python [mise-tasks/print_peps.py]
#!/usr/bin/env -S uv run --script
# /// script
# dependencies = ["requests<3", "rich"]
# ///

import requests
from rich.pretty import pprint

resp = requests.get("https://peps.python.org/api/peps.json", timeout=30)
resp.raise_for_status()
data = resp.json()
pprint([(k, v["title"]) for k, v in data.items()][:10])
```

For the file task, make it executable on Unix with
`chmod +x mise-tasks/print_peps.py`. Declare uv in the project as in the TOML
example. Either form can then run with `mise run print_peps`:

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
