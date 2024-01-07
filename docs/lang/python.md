# Python

The following are instructions for using the python mise core plugin. The core plugin will be used so long as no plugin is manually
installed named "python" using `mise plugins install python [GIT_URL]`.

The code for this is inside of the mise repository at [`./src/plugins/core/python.rs`](https://github.com/jdx/mise/blob/main/src/plugins/core/python.rs).

## Usage

The following installs the latest version of python-3.11.x and makes it the global
default:

```sh
mise use -g python@3.11
```

You can also use multiple versions of python at the same time:

```sh
$ mise use -g python@3.10 python@3.11
$ python -V
3.10.0
$ python3.11 -V
3.11.0
```

## Requirements

mise uses [python-build](https://github.com/pyenv/pyenv/tree/master/plugins/python-build) (part of pyenv) to install python runtimes, you need to ensure its [dependencies](https://github.com/pyenv/pyenv/wiki#suggested-build-environment) are installed before installing python.

## Configuration

`python-build` already has a [handful of settings](https://github.com/pyenv/pyenv/tree/master/plugins/python-build), in
additional to that python in mise has a few extra configuration variables:

- `MISE_PYENV_REPO` [string]: the default is `https://github.com/pyenv/pyenv.git`
- `MISE_PYTHON_COMPILE` [bool]: always use python-build instead of precompiled binaries.
- `MISE_PYTHON_PATCH_URL` [string]: A url to a patch file to pass to python-build.
- `MISE_PYTHON_PATCHES_DIRECTORY` [string]: A local directory containing patch files to pass to python-build.
- `MISE_PYTHON_DEFAULT_PACKAGES_FILE` [string]: location of default packages file, defaults to `$HOME/.default-python-packages`

## Default Python packages

mise can automatically install a default set of Python packages with pip right after installing a Python version. To enable this feature, provide a `$HOME/.default-python-packages` file that lists one package per line, for example:

```text
ansible
pipenv
```

You can specify a non-default location of this file by setting a `MISE_PYTHON_DEFAULT_PACKAGES_FILE` variable.

## [experimental] Automatic virtualenv creation/activation

Python comes with virtualenv support built in, use it with `.mise.toml` configuration like
one of the following:

```toml
[tools]
python = {version="3.11", virtualenv=".venv"} # relative to this file's directory
python = {version="3.11", virtualenv="/root/.venv"} # can be absolute
python = {version="3.11", virtualenv="{{env.HOME}}/.cache/venv/myproj"} # can use templates
```

## [experimental] Precompiled node binaries

In experimental mode, mise will download [precompiled binaries](https://github.com/indygreg/python-build-standalone)
for python instead of compiling them
with python-build. This makes installing python much faster. The plan is once
we have enough users using precompiled binaries successfully, we'll make
that the default behavior in mise.

In addition to being faster, it also means you don't have to install
all of the system dependencies either.

That said, there are some [quirks](https://github.com/indygreg/python-build-standalone/blob/main/docs/quirks.rst)
with the precompiled binaries to be aware of.

If you'd like to disable these binaries, set `MISE_PYTHON_COMPILE` to `1`.
