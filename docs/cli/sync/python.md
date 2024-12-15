# `mise sync python`

- **Usage**: `mise sync python [--pyenv] [--uv]`
- **Source code**: [`src/cli/sync/python.rs`](https://github.com/jdx/mise/blob/main/src/cli/sync/python.rs)

Symlinks all tool versions from an external tool into mise

For example, use this to import all pyenv installs into mise

This won't overwrite any existing installs but will overwrite any existing symlinks

## Flags

### `--pyenv`

Get tool versions from pyenv

### `--uv`

Sync tool versions with uv (2-way sync)

Examples:

```
pyenv install 3.11.0
mise sync python --pyenv
mise use -g python@3.11.0 - uses pyenv-provided python

uv python install 3.11.0
mise install python@3.10.0
mise sync python --uv
mise x python@3.11.0 -- python -V - uses uv-provided python
uv run -p 3.10.0 -- python -V - uses mise-provided python
```
