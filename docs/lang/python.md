---
description: "Install Python with mise, select it per project, and create or activate a virtualenv automatically."
---

# Python

mise installs Python from precompiled
[python-build-standalone](https://github.com/astral-sh/python-build-standalone)
builds, or with python-build, and selects a version per project. It can also
create and activate a project virtualenv, with or without uv.

## Quick start

Select Python for the current project and check the interpreter:

```sh
mise use python@3.14
mise exec -- python --version
```

`mise use` writes `python = "3.14"` to `mise.toml`. Use `mise use -g python@3.14`
for a personal default. Installing Python gives you the interpreter; install a
project's dependencies into a [virtual environment](#automatic-virtualenv-activation).

See the [Python cookbook](/mise-cookbook/python.html) for project recipes,
including uv projects.

## Choosing a version

| Request             | Selects                                            |
| ------------------- | -------------------------------------------------- |
| `python@3.14`       | The newest 3.14.x release                          |
| `python@3.14.8`     | That release                                       |
| `python@latest`     | The newest stable CPython release                  |
| `python@pypy3.11`   | The newest PyPy for Python 3.11                    |
| `python@anaconda3`  | The newest Anaconda distribution (`anaconda3-*`)   |
| `python@miniconda3` | The newest Miniconda distribution (`miniconda3-*`) |

Implementations other than CPython use the names of python-build's
definitions; list them with `mise ls-remote python`. Prereleases such as
`3.15.0rc3` are listed but not selected by a prefix like `3.15` or by `latest`.

You can select several versions at once. The first one provides `python`, and
each provides its versioned executable:

```sh
mise use python@3.13 python@3.14
mise exec -- python --version     # the first configured version, 3.13.x
mise exec -- python3.14 --version # the versioned executable, 3.14.x
```

## Version files

mise can read `.python-version` and `.python-versions`. Enable them for Python:

```sh
mise settings add idiomatic_version_file_enable_tools python
```

This changes your global config. Add `--local` to enable it in the project's
`mise.toml` instead, so teammates get the same behavior. A Python version in
`mise.toml` takes precedence over these files, so keep one source of truth. See
[idiomatic version files](/dev-tools/versions.html#idiomatic-version-files).

## Virtual environments {#automatic-virtualenv-activation}

mise can create a project virtualenv and activate it, in one of two ways:

| Mechanism                             | Use for                           | Configured in |
| ------------------------------------- | --------------------------------- | ------------- |
| [`_.python.venv`](#python-venv)       | Projects that do not use uv       | `[env]`       |
| [`python.uv_venv_auto`](#uv-projects) | uv projects that have a `uv.lock` | `[settings]`  |

Both set `VIRTUAL_ENV` and put the virtualenv's `bin` directory first on
`PATH` (`Scripts` on Windows). That takes effect in shells where mise is
activated, under [`mise exec`](/cli/exec.html) and in tasks. With
[shims](/dev-tools/shims.html) alone the virtualenv is not on `PATH`, and
`which python` points to the shim.

### `_.python.venv` {#python-venv}

Add `_.python.venv` to the `[env]` section of `mise.toml`:

```toml [mise.toml]
[tools]
python = "3.14"

[env]
_.python.venv = { path = ".venv", create = true }
```

Run `mise exec -- python -c 'import sys; print(sys.executable)'` to check that
Python comes from `.venv`, and add `.venv/` to `.gitignore`.

A string such as `_.python.venv = ".venv"` activates an existing environment
and warns, with the command to create it, when it is missing. The table form
accepts these options:

| Option               | Purpose                                                                            |
| -------------------- | ---------------------------------------------------------------------------------- |
| `path`               | Environment directory, relative to the config root or an absolute or template path |
| `create`             | Create the environment when it is missing                                          |
| `python`             | Python version to create it with; defaults to the first configured Python          |
| `python_create_args` | Arguments for `python -m venv`, such as `["--without-pip"]`                        |
| `uv_create_args`     | Arguments for `uv venv`, such as `["--seed"]` or `["--system-site-packages"]`      |

mise creates the environment with uv when uv is installed (for example with
`mise use -g uv`), and otherwise with `python -m venv`. Set
[`python.venv_stdlib`](/lang/python.html#python.venv_stdlib) to always use `venv`. Without
per-directive arguments, mise uses [`python.uv_venv_create_args`](/lang/python.html#python.uv_venv_create_args)
and [`python.venv_create_args`](/lang/python.html#python.venv_create_args).

A virtualenv created by uv has no `pip` (uv provides `uv pip` instead). To get
`pip`, seed it:

```toml [mise.toml]
[env]
_.python.venv = {
  path = ".venv",
  create = true,
  uv_create_args = ["--seed"],
}
```

To pass arguments to Python's `venv` module instead:

```toml [mise.toml]
[env]
_.python.venv = {
  path = ".venv",
  create = true,
  python_create_args = ["--without-pip"],
}
```

### uv projects (`python.uv_venv_auto`) {#uv-projects}

For a project managed by uv, set [`python.uv_venv_auto`](/lang/python.html#python.uv_venv_auto)
and let uv own the environment:

```toml [mise.toml]
[settings]
python.uv_venv_auto = "source"
```

With `"source"`, mise activates the virtualenv uv created. With
`"create|source"`, mise also creates it with uv when it is missing, using the
Python mise selected. mise finds the project by walking up from the current
directory to the nearest `uv.lock`. Without a `uv.lock` the setting does
nothing, so run `uv lock` or `uv sync` first. With `"source"` and no
virtualenv yet, mise warns and asks you to run `uv sync` or `uv venv`, or to
enable [`[deps.uv]`](/dev-tools/deps.html).

The environment is `.venv` next to `uv.lock`, or the path in uv's
`UV_PROJECT_ENVIRONMENT` variable. A relative path is resolved from the
directory that contains `uv.lock`; an absolute path is used as is.

```toml [mise.toml]
[env]
UV_PROJECT_ENVIRONMENT = "my.venv"

[settings]
python.uv_venv_auto = "create|source"
```

The `uv_create_args` and `python_create_args` options of `_.python.venv` do not
apply here. To pass arguments when mise creates this environment, set
`python.uv_venv_create_args`.

uv can still choose a Python it downloaded itself over the one mise installed.
To make `uv sync` and `uv run` use mise's interpreter, set `UV_PYTHON` to its
path:

```toml [mise.toml]
[tools]
python = "3.14"

[env]
UV_PYTHON = { value = "{{ tools.python.path }}", tools = true }
```

See the [uv recipes in the Python cookbook](/mise-cookbook/python.html#mise-uv)
for complete project setups.

### Deprecated forms

The `virtualenv` tool option, such as
`python = { version = "3.14", virtualenv = ".venv" }`, warns since mise 2026.7.0
and is removed in 2027.7.0. Use `_.python.venv` instead.

`python.uv_venv_auto = true` warns since mise 2026.7.0 and is removed in
2027.7.0. Use `"source"` or `"create|source"`. Besides creating and activating
the environment, `true` exported `UV_PYTHON` set to only the Python version
number; use the `UV_PYTHON` recipe above to point uv at mise's interpreter.

## How mise installs Python {#precompiled-python-binaries}

By default mise downloads a precompiled CPython build from
python-build-standalone, which needs no compiler or system libraries, and
verifies its GitHub artifact attestation
([`python.github_attestations`](/lang/python.html#python.github_attestations)). These builds
have some [known differences](https://github.com/astral-sh/python-build-standalone/blob/main/docs/quirks.rst)
from a Python compiled on your machine. mise sets no Python environment
variables of its own; the version's `bin` directory goes on `PATH`.

Versions without a precompiled build, and implementations such as PyPy or
Anaconda, install through
[python-build](https://github.com/pyenv/pyenv/tree/master/plugins/python-build),
the build tool from pyenv. To compile every CPython with python-build, install
its [build dependencies](https://github.com/pyenv/pyenv/wiki#suggested-build-environment)
and set [`python.compile`](/lang/python.html#python.compile):

```sh
mise settings python.compile=true
```

Set `python.compile` to `false` to never use python-build. mise then lists only
precompiled CPython builds and PyPy releases, and installs PyPy from PyPy's own
downloads. python-build reads its own variables, such as
`PYTHON_CONFIGURE_OPTS`, from the environment or from `install_env`.

On Alpine and NixOS, mise currently compiles from source by default, because
[`all_compile`](/configuration/settings.html#all_compile) defaults to `true`
there. This default is deprecated, and mise 2027.8.0 switches to precompiled
binaries. To use precompiled binaries now, set `python.compile = false` or
`all_compile = false`; on NixOS, enable [nix-ld](https://github.com/Mic92/nix-ld)
first. To keep compiling, set `all_compile = true` explicitly.

Official mise releases download the baseline `x86_64` build on x86-64 Linux.
For a build optimized for newer CPUs, such as `x86_64_v3` on a CPU with AVX2,
set [`python.precompiled_arch`](/lang/python.html#python.precompiled_arch).

### Free-threaded Python

To install the free-threaded build from python-build-standalone, choose its
flavor:

```toml [mise.toml]
[settings]
python.precompiled_flavor = "freethreaded-install_only_stripped"
```

```sh
mise install python
```

`python` in that install runs the free-threaded interpreter (`python3.Xt`). The
setting applies to every Python that config installs. The build installs into
the same `installs/python/<version>` directory as the default build, so if that
version is already installed, reinstall it with `mise install -f python`.

To compile a free-threaded Python with python-build instead:

```sh
MISE_PYTHON_COMPILE=true PYTHON_BUILD_FREE_THREADING=1 mise install python
```

### Windows

mise installs the same precompiled python-build-standalone builds on Windows;
compiling with python-build is not supported there. mise adjusts two of the
upstream [quirks](https://github.com/astral-sh/python-build-standalone/blob/main/docs/quirks.rst):

- The archives ship only `python.exe`, so mise adds a `python3.exe` alias next
  to it.
- The archives ship no `pip.exe`, so mise adds `pip.cmd` and `pip3.cmd`
  wrappers that run `python -m pip`. They keep working after pip upgrades
  itself.

The install's `Scripts` directory is on `PATH`, so console scripts from
`pip install`, such as `black`, run directly. If you use shims instead of
`mise activate`, run `mise reshim` after `pip install` to create shims for new
executables.

## Migrating from pyenv or uv

[`mise sync python`](/cli/sync/python.html) makes Pythons installed by other
tools available to mise without reinstalling them:

```sh
mise sync python --pyenv  # link versions from $PYENV_ROOT/versions
mise sync python --uv     # share installs both ways with uv
```

Neither command selects a version for a project. Enable `.python-version` as
described in [Version files](#version-files) to keep using the project's
existing file.

## Troubleshooting

An installed plugin named `python` takes precedence over the built-in
installer. If mise behaves differently from this page, check
[`mise plugins ls`](/cli/plugins/ls.html) and see
[selecting another implementation](/core-tools.html#selecting-another-implementation).

### OpenSSL errors when compiling on macOS {#troubleshooting-errors-with-homebrew}

If a source build cannot find OpenSSL, point the compiler at Homebrew's OpenSSL
for that install only:

```sh
CFLAGS="-I$(brew --prefix openssl)/include" \
LDFLAGS="-L$(brew --prefix openssl)/lib" \
MISE_PYTHON_COMPILE=true mise install python@3.14
```

Keeping the flags on the command avoids affecting unrelated builds. If the
build still fails, read python-build's build log and the
[build environment guide](https://github.com/pyenv/pyenv/wiki#suggested-build-environment).
Precompiled builds are not affected.

### A tool misbehaves with the precompiled build

Some tools trip over the precompiled builds'
[known differences](https://github.com/astral-sh/python-build-standalone/blob/main/docs/quirks.rst).
Set `python.compile = true` to use a Python compiled by python-build instead.

## Tool options

### `patch_sysconfig`

When it installs a precompiled Python on Unix, mise rewrites the build-time
paths in Python's `sysconfig` data so they point at the install directory. If
that patching breaks the install of a particular build, turn it off:

```toml [mise.toml]
[tools]
python = { version = "3.14", patch_sysconfig = false }
```

Without the patch, the installed Python keeps stale build-time paths in its
`sysconfig` data, so use this only as a workaround.

### Generic options

`install_env` reaches python-build, the `python --version` check, default
package installs and `postinstall` commands. For example, to compile with
optimizations when mise builds Python from source (`python.compile = true`, or a
version without a precompiled build):

```toml [mise.toml]
[tools]
python = { version = "3.14", install_env = { PYTHON_CONFIGURE_OPTS = "--enable-optimizations" } }
```

Other generic options are described in [tool options](/dev-tools/#tool-options).

## Default packages file <Badge type="danger" text="deprecated" /> {#default-python-packages}

mise installs the packages listed in `~/.default-python-packages`
([`python.default_packages_file`](/lang/python.html#python.default_packages_file)), one per line,
with `pip install` into each new Python version. mise warns about this file from
2026.11.0 and stops reading it in 2027.11.0. Install Python CLIs with the
[PyPI backend](/dev-tools/backends/pypi.html) instead, for example
`"pypi:black" = "latest"`, or use a
[`postinstall`](/dev-tools/#tool-postinstall-commands) command for packages
every Python version needs.

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="python" :level="3" />
