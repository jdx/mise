# pipx Backend

You may install python packages directly from:

- PyPI
- Git
- GitHub
- Http

The code for this is inside of the mise repository at [`./src/backend/pipx.rs`](https://github.com/jdx/mise/blob/main/src/backend/pipx.rs).

## Dependencies

This relies on having `pipx` installed. You can install it with or without mise.
Here is how to install `pipx` with mise:

```sh
mise use -g python
pip install --user pipx
```

Other installation instructions can be found [here](https://pipx.pypa.io/latest/installation/)

## Usage

The following installs the latest version of [black](https://github.com/psf/black)
and sets it as the active version on PATH:

```sh
$ mise use -g pipx:psf/black
$ black --version
black, 24.3.0
```

The version will be set in `~/.config/mise/config.toml` with the following format:

```toml
[tools]
"pipx:psf/black" = "latest"
```

## Python upgrades

If the python version used by a pipx package changes, (by mise or system python), you may need to
reinstall the package. This can be done with:

```sh
mise install -f pipx:psf/black
```

Or you can reinstall all pipx packages with:

```sh
mise install -f "pipx:*"
```

mise _should_ do this automatically when using `mise up python`.

### Supported Pipx Syntax

| Description                           | Usage                                                  |
| ------------------------------------- | ------------------------------------------------------ |
| PyPI shorthand latest version         | `pipx:black`                                           |
| PyPI shorthand for specific version   | `pipx:black@24.3.0`                                    |
| GitHub shorthand for latest version   | `pipx:psf/black`                                       |
| GitHub shorthand for specific version | `pipx:psf/black@24.3.0`                                |
| Git syntax for latest version         | `pipx:git+https://github.com/psf/black`                |
| Git syntax for a branch               | `pipx:git+https://github.com/psf/black.git@main`       |
| Https with zipfile                    | `pipx:https://github.com/psf/black/archive/18.9b0.zip` |

Other syntax may work but is unsupported and untested.

## Settings

Set these with `mise settings set [VARIABLE] [VALUE]` or by setting the environment variable listed.

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="pipx" :level="3" />

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `pipx` backend—these
go in `[tools]` in `mise.toml`.

### `extras`

Install additional components.

```toml
[tools]
"pipx:harlequin" = { extras = "postgres,s3" }
```

### `pipx_args`

Additional arguments to pass to `pipx` when installing the package.

```toml
[tools]
"pipx:black" = { pipx_args = "--preinstall" }
```

### `uvx_args`

Additional arguments to pass to `uvx` when installing the package.

```toml
[tools]
"pipx:ansible-core" = { uvx_args = "--with ansible" }
```
