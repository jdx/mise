# `mise sync python`

- **Usage**: `mise sync python <--pyenv>`
- **Source code**: [`src/cli/sync/python.rs`](https://github.com/jdx/mise/blob/main/src/cli/sync/python.rs)

Symlinks all tool versions from an external tool into mise

For example, use this to import all pyenv installs into mise

## Flags

### `--pyenv`

Get tool versions from pyenv

Examples:

    pyenv install 3.11.0
    mise sync python --pyenv
    mise use -g python@3.11.0 - uses pyenv-provided python
