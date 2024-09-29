# `mise sync node`

**Usage**: `mise sync node [FLAGS]`

Symlinks all tool versions from an external tool into mise

For example, use this to import all Homebrew node installs into mise

## Flags

### `--brew`

Get tool versions from Homebrew

### `--nvm`

Get tool versions from nvm

### `--nodenv`

Get tool versions from nodenv

Examples:

    brew install node@18 node@20
    mise sync node --brew
    mise use -g node@18 - uses Homebrew-provided node
