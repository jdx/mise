# uv Backend

> Formerly called the **pipx** backend. The `pipx:` prefix still works but is deprecated; use `uv:` instead.

`uv` is a tool for running Python CLIs in isolated virtualenvs via `uv tool`. This is necessary for Python CLIs
because it prevents conflicting dependencies between CLIs or between a CLI and Python projects. In essence,
this backend lets you add Python CLIs to mise.

To be clear, this backend is **not** for managing Python dependencies generally. mise is a tool manager, not a
dependency manager like pip, uv, or poetry. You can, however, use mise to install those package managers. You'd
use the `uv` backend to install a CLI like "black", not a library like "NumPy" or "requests".

The uv backend supports the following sources:

- PyPI
- Git
- GitHub
- Http

The code for this is inside of the mise repository at
[`./src/backend/uv_tool.rs`](https://github.com/jdx/mise/blob/main/src/backend/uv_tool.rs).

## Dependencies

This backend prefers `uv` (recommended) and can fall back to `pipx` if configured.

Install `uv` with mise:

```sh
mise use -g uv
```

If you need to force pipx for a specific tool (e.g., compatibility issues), first install `pipx`.
You can install it with or without mise. Here is how to install `pipx` with mise:

```sh
mise use -g python
pip install --user pipx
```

[Other pipx installation instructions](https://pipx.pypa.io/latest/installation/)

Then set `pipx = true` for the tool:

```sh
mise use "uv:psf/black[pipx=true]"
```

## Usage

The following installs the latest version of [black](https://github.com/psf/black)
from GitHub and sets it as the active version on PATH:

```sh
$ mise use -g uv:psf/black
$ black --version
black, 24.3.0
```

The version will be set in `~/.config/mise/config.toml` with the following format:

```toml
[tools]
"uv:psf/black" = "latest"
```

## Python upgrades

If the Python version used by a uv tool changes (by mise or system Python), you may need to reinstall the
package. This can be done with:

```sh
mise install -f uv:psf/black
```

Or you can reinstall all uv tools with:

```sh
mise install -f "uv:*"
```

mise _should_ do this automatically when using `mise up python`.

### Supported uv Syntax

| Description                           | Usage                                                |
| ------------------------------------- | ---------------------------------------------------- |
| PyPI shorthand latest version         | `uv:black`                                           |
| PyPI shorthand for specific version   | `uv:black@24.3.0`                                    |
| GitHub shorthand for latest version   | `uv:psf/black`                                       |
| GitHub shorthand for specific version | `uv:psf/black@24.3.0`                                |
| Git syntax for latest version         | `uv:git+https://github.com/psf/black.git`            |
| Git syntax for a branch               | `uv:git+https://github.com/psf/black.git@main`       |
| Https with zipfile                    | `uv:https://github.com/psf/black/archive/18.9b0.zip` |

Other syntax may work but is unsupported and untested.

## Settings

Set these with `mise settings set [VARIABLE] [VALUE]` or by setting the environment variable listed.

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="uv" :level="3" />

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `uv` backend--these
go in `[tools]` in `mise.toml`.

### `extras`

Install additional components.

```toml
[tools]
"uv:harlequin" = { version = "latest", extras = "postgres,s3" }
```

### `pipx`

Set to `true` to always disable `uv tool` for this tool and use pipx instead.

```toml
[tools]
"uv:ansible" = { version = "latest", pipx = "true" }
```

### `pipx_args`

Additional arguments to pass to `pipx` when installing the package (only used when `pipx = true`).

```toml
[tools]
"uv:black" = { version = "latest", pipx = "true" , pipx_args = "--preinstall" }
```

### `uv_tool_args`

Additional arguments to pass to `uv tool` when installing the package.

```toml
[tools]
"uv:ansible-core" = { version = "latest", uv_tool_args = "--with ansible" }
```

> Legacy options `uvx` and `uvx_args` are still supported but deprecated.
