# `mise sync ruby`

- **Usage**: `mise sync ruby [--brew]`
- **Source code**: [`src/cli/sync/ruby.rs`](https://github.com/jdx/mise/blob/main/src/cli/sync/ruby.rs)

Symlinks all ruby tool versions from an external tool into mise

## Flags

### `--brew`

Get tool versions from Homebrew

Examples:

```
brew install ruby
mise sync ruby --brew
mise use -g ruby - Use the latest version of Ruby installed by Homebrew
```
