# `mise sync node`

- **Usage**: `mise sync node [FLAGS]`
- **Source code**: [`src/cli/sync/node.rs`](https://github.com/jdx/mise/blob/main/src/cli/sync/node.rs)

Symlinks all tool versions from an external tool into mise

For example, use this to import all Homebrew node installs into mise

This won't overwrite any existing installs but will overwrite any existing symlinks

## Flags

### `--brew`

Get tool versions from Homebrew

### `--nvm`

Get tool versions from nvm

### `--nodenv`

Get tool versions from nodenv

Examples:

```
brew install node@18 node@20
mise sync node --brew
mise use -g node@18 - uses Homebrew-provided node
```
