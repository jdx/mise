# rtx-python

[![Build Status](https://travis-ci.org/danhper/rtx-python.svg?branch=master)](https://travis-ci.org/danhper/rtx-python)

Python plugin for [rtx](https://github.com/jdxcode/rtx) version manager

## Install

```
rtx plugin add python
```

### Install with `--patch`

> Enable to fix macOS 11 issues

You can use environment variable `RTX_PYTHON_PATCH_URL` to install with `--patch` like that:

```
export RTX_PYTHON_PATCH_URL="https://github.com/python/cpython/commit/8ea6353.patch?full_index=1"
rtx install python 3.6.12
```

or use environment variable `RTX_PYTHON_PATCHES_DIRECTORY`.

## Use

Check [rtx](https://github.com/jdxcode/rtx) readme for instructions on how to install & manage versions of Python.
Please make sure you have the required [system dependencies](https://github.com/pyenv/pyenv/wiki#suggested-build-environment) installed before trying to install Python.

Under the hood, rtx-python uses [python-build](https://github.com/yyuu/pyenv/tree/master/plugins/python-build)
to build and install Python, check its [README](https://github.com/yyuu/pyenv/tree/master/plugins/python-build)
for more information about build options and the [common build problems](https://github.com/pyenv/pyenv/wiki/Common-build-problems) wiki page for any issues encountered during installation of python versions.

## Using multiple versions of Python

A common request for Python is being able to use the `python2` and `python3` commands without needing to switch version.
This can be achieved by setting multiple versions of Python, for example with

```
rtx global python 3.6.2 2.7.13
```

Executables in the first version will take priority over the executables in the next one. Note that you can use an arbitrary number over versions, if needed.
With the above example, `python` will therefore use the `python` executable found in version 3.6.2.
However, as the `python2` does not exist in Python 3.6.2, `python2` will use the `python2` executable found in version 2.7.13.

```
python -V
Python 3.6.2

python3 -V
Python 3.6.2

python2 -V
Python 2.7.13
```

## Pip installed modules and binaries

If you use pip to install a module like ipython that has binaries. You will need to run `rtx reshim python` for the binary to be in your path.

## Default Python packages

rtx-python can automatically install a default set of Python packages with pip right after installing a Python version. To enable this feature, provide a `$HOME/.default-python-packages` file that lists one package per line, for example:

```
ansible
pipenv
```

You can specify a non-default location of this file by setting a `RTX_PYTHON_DEFAULT_PACKAGES_FILE` variable.

## [experimental] Virtualenv support

Automatically create and activate a virtualenv when entering a directory with the
`virtualenv` option. This requires using `.rtx.toml` since `.tool-versions` could
not support this syntax:

```toml
[tools]
python = { version='3.11', virtualenv='.venv' }
```

## [experimental] Pipenv support

Automatically create and activate a pipenv virtualenv when entering a directory with the
`pipenv` option. This requires using `.rtx.toml`:

```toml
[tools]
python = { version='3.11', pipenv='Pipfile' }
```

## [experimental] Poetry support

Automatically create and activate a poetry virtualenv when entering a directory with the
`poetry` option. This requires using `.rtx.toml`:

```toml
[tools]
python = { version='3.11', poetry='pyproject.toml' }
```
