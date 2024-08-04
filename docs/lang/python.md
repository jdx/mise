# Python

The following are instructions for using the python mise core plugin. The core plugin will be used
so long as no plugin is manually
installed named "python" using `mise plugins install python [GIT_URL]`.

The code for this is inside of the mise repository
at [`./src/plugins/core/python.rs`](https://github.com/jdx/mise/blob/main/src/plugins/core/python.rs).

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

See available versions with `mise ls-remote python`.

## Configuration

`python-build` already has
a [handful of settings](https://github.com/pyenv/pyenv/tree/master/plugins/python-build), in
additional to that python in mise has a few extra configuration variables.

Set these with `mise settings set [VARIABLE] [VALUE]` or by setting the environment variable.

### `python_pyenv_repo`

* Type: `string`
* Env: `MISE_PYENV_REPO`
* Default: `https://github.com/pyenv/pyenv.git`

The pyenv repo to get python-build from.

### `python_compile`

* Type: `bool`
* Env: `MISE_PYTHON_COMPILE`
* Default: [undefined]
* Values:
  * `true` - always compile with python-build instead of downloading [precompiled binaries](#precompiled-python-binaries).
  * `false` - always download precompiled binaries.
  * [undefined] - use precompiled binary if one is available for the current platform, compile otherwise.

### `python_precompiled_os`

* Type: `string`
* Env: `MISE_PYTHON_PRECOMPILED_OS`
* Default: `"apple-darwin" | "unknown-linux-gnu" | "unknown-linux-musl"`

Specify the OS to use for precompiled binaries.

### `python_precompiled_arch`

* Type: `string`
* Env: `MISE_PYTHON_PRECOMPILED_ARCH`
* Default: `"x86_64_v3" | "aarch64"`

Specify the architecture to use for precompiled binaries. If on an old CPU, you may want to set this
to
`"x86_64"` for the most compatible binaries.
See <https://gregoryszorc.com/docs/python-build-standalone/main/running.html> for more information.

### `python_patch_url`

* Type: `string`
* Env: `MISE_PYTHON_PATCH_URL`

A url to a patch file to pass to python-build.

### `python_patches_directory`

* Type: `string`
* Env: `MISE_PYTHON_PATCHES_DIRECTORY`

A local directory containing patch files to pass to python-build.

### `python_default_packages_file`

* Type: `string`
* Env: `MISE_PYTHON_DEFAULT_PACKAGES_FILE`
* Default: `$HOME/.default-python-packages`

Packages list to install with pip after installing a Python version.

## Default Python packages

mise can automatically install a default set of Python packages with pip right after installing a
Python version. To enable this feature, provide a `$HOME/.default-python-packages` file that lists
one package per line, for example:

```text
ansible
pipenv
```

You can specify a non-default location of this file by setting a `MISE_PYTHON_DEFAULT_PACKAGES_FILE`
variable.

## Precompiled python binaries

By default, mise will
download [precompiled binaries](https://github.com/indygreg/python-build-standalone)
for python instead of compiling them with python-build. This makes installing python much faster.

In addition to being faster, it also means you don't have to install all of the system dependencies
either.

That said, there are
some [quirks](https://github.com/indygreg/python-build-standalone/blob/main/docs/quirks.rst)
with the precompiled binaries to be aware of.

If you'd like to disable these binaries, set [`python_compile`](#python_compile) to `true`.

These binaries may not work on older CPUs however you may opt into binaries which
are more compatible with older CPUs by setting `MISE_PYTHON_PRECOMPILED_ARCH` with
a different version. See <https://gregoryszorc.com/docs/python-build-standalone/main/running.html> for
more information
on this option. Set it to "x86_64" for the most compatible binaries.

## python-build

Optionally, mise
uses [python-build](https://github.com/pyenv/pyenv/tree/master/plugins/python-build) (part of pyenv)
to compile python runtimes,
you need to ensure
its [dependencies](https://github.com/pyenv/pyenv/wiki#suggested-build-environment) are installed
before installing python with
python-build.

## Troubleshooting errors with Homebrew

If you normally use Homebrew and you see errors regarding OpenSSL,
your best bet might be using the following command to install Python:

```sh
CFLAGS="-I$(brew --prefix openssl)/include" \
LDFLAGS="-L$(brew --prefix openssl)/lib" \
mise install python@latest;
```

Homebrew installs its own OpenSSL version, which may collide with system-expected ones.
You could even add that to your
`.profile`,
`.bashrc`,
`.zshrc`...
to avoid setting them every time

Additionally, if you encounter issues with python-build,
you may benefit from unlinking pkg-config prior to install
([reason](https://github.com/pyenv/pyenv/issues/2823#issuecomment-1769081965)).

```sh
brew unlink pkg-config
mise install python@latest
brew link pkg-config
```

Thus the entire script would look like:

```sh
brew unlink pkg-config
CFLAGS="-I$(brew --prefix openssl)/include" \
  LDFLAGS="-L$(brew --prefix openssl)/lib" \
  mise install python@latest
brew link pkg-config
```

## Automatic virtualenv activation

Python comes with virtualenv support built in, use it with `.mise.toml` configuration like
one of the following:

```toml
[tools]
python = "3.11" # [optional] will be used for the venv

[env]
_.python.venv = ".venv" # relative to this file's directory
_.python.venv = "/root/.venv" # can be absolute
_.python.venv = "{{env.HOME}}/.cache/venv/myproj" # can use templates
_.python.venv = { path = ".venv", create = true } # create the venv if it doesn't exist
```

The venv will need to be created manually with `python -m venv /path/to/venv` unless `create=true`.
