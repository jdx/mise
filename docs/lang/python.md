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

## Settings

`python-build` already has
a [handful of settings](https://github.com/pyenv/pyenv/tree/master/plugins/python-build), in
additional to that python in mise has a few extra configuration variables.

Set these with `mise settings set [VARIABLE] [VALUE]` or by setting the environment variable.

<script setup>
import { data } from '/settings.data.ts';
import Setting from '/components/setting.vue';

const settings = data.find(s => s.key === 'python').settings;
</script>

<Setting v-for="setting in settings" :setting="setting" :key="setting.key" :level="3" />

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

If you'd like to disable these binaries, set `mise settings set python.compile 1`.

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

## Installing free-threaded python

Free-threaded python can be installed via python-build by running the following:

```bash
MISE_PYTHON_COMPILE=1 PYTHON_BUILD_FREE_THREADING=1 mise install
```

Currently, they are not supported with precompiled binaries.
