# pipx Backend <Badge type="warning" text="experimental" />

You may install python packages directly from:
- PyPI
- Git
- Github
- Http

The code for this is inside of the mise repository at [`./src/forge/pipx.rs`](https://github.com/jdx/mise/blob/main/src/forge/pipx.rs).

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

### Supported Pipx Syntax

| Description                           | Usage                                                  |
| ------------------------------------- | ------------------------------------------------------ |
| PyPI shorthand latest version         | `pipx:black`                                           |
| PyPI shorthand for specific version   | `pipx:black@24.3.0`                                    |
| Github shorthand for latest version   | `pipx:psf/black`                                       |
| Github shorthand for specific version | `pipx:psf/black@24.3.0`                                |
| Git syntax for latest version         | `pipx:git+https://github.com/psf/black`                |
| Git syntax for a branch               | `pipx:git+https://github.com/psf/black.git@main`       |
| Https with zipfile                    | `pipx:https://github.com/psf/black/archive/18.9b0.zip` |

Other syntax may work but is unsupported and untested.
