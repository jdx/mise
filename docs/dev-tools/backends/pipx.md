# pipx Backend

The `pipx` backend installs Python command-line applications in isolated virtual
environments. Each tool gets its own dependencies. Use a project environment
and pip or uv for application libraries such as NumPy and requests.

When uv is available, mise uses **`uv tool install`**. Otherwise it uses
`pipx install`. The `pipx:` prefix names the backend in both cases; the legacy
`uvx` option names do not mean that mise runs the `uvx` command.

The pipx backend supports the following sources:

- PyPI
- Git
- GitHub
- HTTP

The code for this is inside the mise repository at [`./src/backend/pipx.rs`](https://github.com/jdx/mise/blob/main/src/backend/pipx.rs).

## Dependencies

Install uv and a Python version suitable for the CLI. For example:

```sh
mise use python@3.14 uv pipx:black
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
"pipx:black" = "latest"
```

Add `-g` for global configuration. `pipx:black` installs the PyPI distribution;
`pipx:psf/black` installs from its GitHub source. Choose the source intentionally,
since its releases and installation requirements can differ.

## Choosing Python

The selected installer chooses the interpreter for each tool environment. If a
CLI must use a particular Python version, pass that requirement explicitly:

```toml
[tools]
python = "3.14"
uv = "latest"
"pipx:black" = {
  version = "latest",
  uvx_args = "--python 3.14",
  pipx_args = "--python 3.14",
}
```

Only the arguments for the selected installer apply. uv can also download an
interpreter according to its own Python discovery and download settings.

## Python upgrades

If a CLI stops working after changing Python, reinstall it under the intended
Python version. This recreates the tool environment and its dependencies:

```sh
mise install --force pipx:black
mise exec -- black --version
```

Check which Python version is active before reinstalling. Existing virtualenvs
and native extensions do not necessarily remain usable after their interpreter
is removed or changed.

### Supported Pipx Syntax

| Description                           | Usage                                                  |
| ------------------------------------- | ------------------------------------------------------ |
| PyPI shorthand latest version         | `pipx:black`                                           |
| PyPI shorthand for specific version   | `pipx:black@24.3.0`                                    |
| GitHub shorthand for latest version   | `pipx:psf/black`                                       |
| GitHub shorthand for specific version | `pipx:psf/black@24.3.0`                                |
| Git syntax for latest version         | `pipx:git+https://github.com/psf/black.git`            |
| Git syntax for a branch               | `pipx:git+https://github.com/psf/black.git@main`       |
| HTTPS with zipfile                    | `pipx:https://github.com/psf/black/archive/18.9b0.zip` |

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
<Settings child="pipx" :level="3" />

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `pipx` backend—these
go in `[tools]` in `mise.toml`.

### `registry_url`

Set the package registry URL mise uses to resolve versions for this tool. The URL must contain a
`{}` placeholder for the package name. This overrides the `pipx.registry_url` setting for this
tool only; registry arguments for installation are still configured separately through `uvx_args` or
`pipx_args`.

```toml
[tools]
"pipx:my-tool" = {
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
"pipx:black" = { version = "latest", install_env = { UV_COMPILE_BYTECODE = "1" } }
```

### `extras`

Install additional components.

```toml
[tools]
"pipx:harlequin" = { version = "latest", extras = "postgres,s3" }
# equivalent array form:
# "pipx:harlequin" = { version = "latest", extras = ["postgres", "s3"] }
# extras also work with Git sources:
# "pipx:psf/black" = { version = "latest", extras = ["jupyter"] }
```

When passing extras inline, use mise's `key=value` tool-option syntax:

```bash
mise use 'pipx:psf/black[extras=jupyter]@latest'
```

For Git repositories whose name differs from the Python distribution name, set `package_name` so
mise can build the requirement used to select extras:

```toml
[tools]
"pipx:owner/repository" = { version = "latest", package_name = "distribution", extras = ["feature"] }
```

### `pipx_args`

Additional arguments to pass to `pipx` when installing the package.

```toml
[tools]
"pipx:ansible" = { version = "latest", uvx = false, pipx_args = "--include-deps" }
```

### `uvx`

Set to `false` to always disable uv for this tool.

```toml
[tools]
"pipx:ansible" = { version = "latest", uvx = false, pipx_args = "--include-deps" }
```

### `uvx_args`

Additional arguments to pass to `uv tool install`. These apply only when uv is
selected; `pipx_args` applies only to the pipx installer.

```toml
[tools]
"pipx:ansible-core" = { version = "latest", uvx_args = "--with ansible" }
```
