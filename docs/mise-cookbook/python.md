---
description: "Set up Python projects with mise: a requirements.txt virtualenv, a uv project, and uv scripts with inline dependencies."
socialDescription: "Set up a requirements.txt virtualenv, a uv project, or uv scripts with mise."
---

# Python

Set up a Python project with mise: a `requirements.txt` project with a
virtualenv, a uv project, or scripts with inline dependencies. For installing
Python and the virtualenv settings, see [Python](/lang/python.html).

## Use a `requirements.txt` project with a virtualenv {#a-python-project-with-virtualenv}

This recipe expects `requirements.txt`, `app.py` and a `tests/` directory, with
`pytest` in the requirements. mise creates `.venv` and activates it; the install
task fills it with the project's dependencies.

```toml [mise.toml]
[tools]
python = "3.14"
uv = "latest"
ruff = "latest"

[env]
_.python.venv = { path = ".venv", create = true }

[tasks.install]
description = "Install dependencies"
alias = "i"
run = "uv pip install -r requirements.txt"

[tasks.app]
description = "Run the application"
run = "python app.py"

[tasks.test]
description = "Run tests"
run = "python -m pytest tests/"

[tasks.lint]
description = "Lint the code"
run = "ruff check ."
```

Run `mise run install`, then `mise run test`, `mise run lint` or
`mise run app`. Add `.venv/` to `.gitignore`. See
[`_.python.venv`](/lang/python.html#python-venv) for its other options.

## Use a uv project {#mise-uv}

This recipe expects a project created by `uv init`, which writes
`pyproject.toml` and `.python-version`. mise reads the Python version from
`.python-version` and activates the virtualenv that uv manages:

```toml [mise.toml]
[tools]
uv = "latest"

[settings]
idiomatic_version_file_enable_tools = ["python"]
python.uv_venv_auto = "create|source"
```

Run `mise install`, then `mise exec -- uv sync` to create `uv.lock` and
`.venv`. mise finds the uv project through `uv.lock`, so
[`python.uv_venv_auto`](/lang/python.html#uv-projects) has no effect until that
file exists. Use `"source"` instead if only uv should create `.venv`.

In a shell with mise activated, `python` resolves to `.venv/bin/python` at the
next prompt. To check without activation:

```sh
mise exec -- python -c 'import sys; print(sys.executable)'
# /path/to/project/.venv/bin/python
```

To have mise run `uv sync` when `pyproject.toml` or `uv.lock` changes, use the
experimental [`mise deps`](/dev-tools/deps.html): keep `python.uv_venv_auto` at
`"source"`, set `experimental = true` under `[settings]`, add `[deps.uv]`, and
run `mise deps`.

### Share Python installs with uv {#syncing-python-versions-installed-by-mise-and-uv}

[`mise sync python --uv`](/cli/sync/python.html) makes the Python versions
installed by mise and by uv available to both. It shares installed
interpreters only: it does not change `.python-version`, select the project's
version, or sync packages. Use `uv sync` for project dependencies.

## Run scripts with inline dependencies {#uv-scripts}

A file task can run with `uv run` in its shebang and declare its dependencies
in a [PEP 723](https://peps.python.org/pep-0723/) block. uv installs them into a
cached environment the first time the script runs:

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

`--script` is required when the file name does not end in `.py`. Declare uv in
`[tools]` as in the uv project recipe, make the file executable with
`chmod +x mise-tasks/print_peps.py`, and run it:

```sh
mise run print_peps
```

```text
[print_peps] $ ~/uv-project/mise-tasks/print_peps.py
Installed 9 packages in 8ms
[
│   ('1', 'PEP Purpose and Guidelines'),
│   ('2', 'Procedure for Adding New Modules'),
    #...
]
```

The same script also works inline as a `run` value in `mise.toml`; see
[TOML tasks](/tasks/toml-tasks.html#other-languages).
