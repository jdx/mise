---
description: "Install Python command-line applications in isolated virtual environments."
---

# PyPI Backend

The `pypi` backend installs Python command-line applications in isolated virtual
environments. Each tool gets its own dependencies. Use a project environment
and pip or uv for application libraries such as NumPy and requests.

With dependency graphs, mise uses **`uv sync --frozen`**. Version-only installs
use `uv tool install` when uv is available, otherwise `pipx install`. The `pypi:`
prefix names the backend in all cases; `pipx:` remains a supported alias with
the same installations and lock entries. Explicit `pipx:` names are preserved in
output and lockfiles, including older lockfile revisions. The legacy `uvx` option names do not
mean that mise runs the `uvx` command.

The PyPI backend supports the following sources:

- PyPI
- Git
- GitHub
- HTTP

The code for this is inside the mise repository at [`./src/backend/pipx.rs`](https://github.com/jdx/mise/blob/main/src/backend/pipx.rs).

## Dependencies

Install uv and a Python version suitable for the CLI. For example:

```sh
mise use python@3.14 uv pypi:black
mise exec -- black --version
```

If you need the pipx installer instead, install `python` and `pipx` with
`mise use python@3.14 pipx`, then set the tool's [`uvx`](#uvx) option to `false`.
No separately installed pipx is needed for the uv path.

mise forwards [`minimum_release_age`](/configuration/settings.html#minimum_release_age)
to transitive Python dependency resolution during install. The uv install path uses uv's
`--exclude-newer` flag and requires `uv >= 0.2.22`. The `pipx` fallback passes pip's
`--uploaded-prior-to` flag.

## Usage

The command above writes a project configuration like this:

```toml
[tools]
python = "3.14"
uv = "latest"
"pypi:black" = "latest"
```

Add `-g` for global configuration. `pypi:black` installs the PyPI distribution;
`pypi:psf/black` installs from its GitHub source. Choose the source intentionally,
since its releases and installation requirements can differ.

## Dependency locking

With uv 0.12.10 or newer, new `mise.lock` files record the complete portable
Python dependency graph, including wheel hashes and Python/platform markers.
Existing lockfiles retain their behavior until explicitly upgraded:

```sh
mise lock --upgrade
mise install --locked
mise lock --bump pypi:black
```

The last command refreshes transitive dependencies even when Black's version
has not changed. Ordinary `mise lock` reuses the recorded graph. Different graphs
and configured Python interpreter identities have separate installations; `mise ls` shows
the normal package version.

Graph-locked installations require published wheels for the current platform and
Python version. They do not build source distributions. Git sources and standalone
pipx installs retain version-only locking. Free-form `uvx_args` and `pipx_args`
are unsupported with dependency graphs; an existing uv graph cannot be replayed
through standalone pipx. Missing graphs in revision-2 locked uv installs are errors.

The graph covers the package's supported Python range (Python 3.8 or newer).
It retains all published wheel targets for portability, so large dependency graphs
can substantially increase the lockfile size. Frozen installs reuse uv's artifact
cache. Lock generation needs an installed interpreter discoverable by uv, although
it need not be the Python version configured for the tool.

Ordinary resolution does not launch Python. Configured Python identities use the
resolved mise version and installation path. For system Python, installation records
its implementation, major/minor version, ABI and platform; later commands discover
that environment without requiring system Python to remain on PATH.
The selected mise Python interpreter determines which locked marker branches are
installed. Configure Python under `[tools]`; graph installs do not silently download
a replacement interpreter. Simple-only indexes must publish consistent
`data-requires-python` metadata on the selected release's wheel links.

Registry credentials stay in the installer environment or credential provider,
not `mise.lock`. URLs containing credentials or query strings cannot be recorded.
Release-age filtering applies when resolving a graph, not when replaying it.

## Choosing Python

For graph-locked tools, configure the interpreter through mise:

```toml
[tools]
python = "3.14"
uv = "0.12.10"
"pypi:black" = "latest"
```

For legacy version-only installs, the selected installer chooses the interpreter;
`uvx_args` and `pipx_args` can pass installer-specific Python options.

## Python upgrades

If a CLI stops working after changing Python, reinstall it under the intended
Python version. This recreates the tool environment and its dependencies:

```sh
mise install --force pypi:black
mise exec -- black --version
```

Check which Python version is active before reinstalling. Existing virtualenvs
and native extensions do not necessarily remain usable after their interpreter
is removed or changed.

### Supported Pipx Syntax

| Description                           | Usage                                                  |
| ------------------------------------- | ------------------------------------------------------ |
| PyPI shorthand latest version         | `pypi:black`                                           |
| PyPI shorthand for specific version   | `pypi:black@24.3.0`                                    |
| GitHub shorthand for latest version   | `pypi:psf/black`                                       |
| GitHub shorthand for specific version | `pypi:psf/black@24.3.0`                                |
| Git syntax for latest version         | `pypi:git+https://github.com/psf/black.git`            |
| Git syntax for a branch               | `pypi:git+https://github.com/psf/black.git@main`       |
| HTTPS with zipfile                    | `pypi:https://github.com/psf/black/archive/18.9b0.zip` |

For GitHub URLs, `latest` resolves to the latest published GitHub Release and falls
back to default-branch HEAD when there are no releases. For other Git URLs, `latest`
tracks default-branch HEAD and resolves it to a concrete commit before installation.
Remote tags are available for explicit version requests.

Other syntax may work but is unsupported and untested.

## Settings

Set these with `mise settings set [VARIABLE]=[VALUE]` or by setting the environment variable listed.

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="pypi" :level="3" />

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `pypi` backend—these
go in `[tools]` in `mise.toml`.

### `registry_url`

Set the package registry URL mise uses to resolve versions for this tool. The URL must contain a
`{}` placeholder for the package name. This overrides the `pypi.registry_url` setting for this
tool only; registry arguments for installation are still configured separately through `uvx_args` or
`pipx_args`.

```toml
[tools]
"pypi:my-tool" = {
  version = "latest",
  registry_url = "https://packages.example.com/pypi/{}/json",
  uvx_args = "--index-url https://packages.example.com/pypi/simple",
  pipx_args = "--pip-args='--index-url https://packages.example.com/pypi/simple'"
}
```

### `install_env`

Set environment variables for `uv tool install` or `pipx install`. mise still
sets the tool directory, bin directory, and configured Python package index
variables after applying `install_env`. For the uv installer, for example:

```toml
[tools]
"pypi:black" = { version = "latest", install_env = { UV_COMPILE_BYTECODE = "1" } }
```

### `extras`

Install additional components.

```toml
[tools]
"pypi:harlequin" = { version = "latest", extras = "postgres,s3" }
# equivalent array form:
# "pypi:harlequin" = { version = "latest", extras = ["postgres", "s3"] }
# extras also work with Git sources:
# "pypi:psf/black" = { version = "latest", extras = ["jupyter"] }
```

When passing extras inline, use mise's `key=value` tool-option syntax:

```bash
mise use 'pypi:psf/black[extras=jupyter]@latest'
```

For Git repositories whose name differs from the Python distribution name, set `package_name` so
mise can build the requirement used to select extras:

```toml
[tools]
"pypi:owner/repository" = { version = "latest", package_name = "distribution", extras = ["feature"] }
```

### `pipx_args`

Additional arguments to pass to `pipx` when installing the package.

```toml
[tools]
"pypi:ansible" = { version = "latest", uvx = false, pipx_args = "--include-deps" }
```

### `uvx`

Set to `false` to always disable uv for this tool.

```toml
[tools]
"pypi:ansible" = { version = "latest", uvx = false, pipx_args = "--include-deps" }
```

### `uvx_args`

Additional arguments to pass to `uv tool install`. These apply only when uv is
selected; `pipx_args` applies only to the pipx installer.

```toml
[tools]
"pypi:ansible-core" = { version = "latest", uvx_args = "--with ansible" }
```
