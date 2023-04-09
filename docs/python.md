# Python in rtx

The following are instructions for using the python rtx core plugin. This is used when
the "experimental" setting is "true" and there isn't a git plugin installed named "python"

If you want to use asdf-python or rtx-python then use `rtx plugins install python URL`.

The code for this is inside of the rtx repository at [`./src/plugins/core/python.rs`](https://github.com/jdxcode/rtx/blob/main/src/plugins/core/python.rs).

## Usage

The following installs the latest version of python-3.11.x and makes it the global
default:

```sh-session
$ rtx install python@3.11
$ rtx global python@3.11
```

You can also use multiple versions of python at the same time:

```sh-session
$ rtx global python@3.10 python@3.11
$ python -V
3.10.0
$ python3.11 -V
3.11.0
```

## Default Python packages

rtx-python can automatically install a default set of Python packages with pip right after installing a Python version. To enable this feature, provide a `$HOME/.default-python-packages` file that lists one package per line, for example:

```
ansible
pipenv
```

You can specify a non-default location of this file by setting a `RTX_PYTHON_DEFAULT_PACKAGES_FILE` variable.

## [experimental] Automatic virtualenv creation/activation

Python comes with virtualenv support built in, use it with `.rtx.toml` configuration like
the following:

```toml
[tools]
python = {version="3.11", virtualenv=".venv"} # relative to this file's directory
python = {version="3.11", virtualenv="/root/.venv"} # can be absolute
python = {version="3.11", virtualenv="{{env.HOME}}/.cache/venv/myproj"} # can use templates
```
