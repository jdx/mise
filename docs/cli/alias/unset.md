# `mise alias unset`

- **Usage**: `mise alias unset <PLUGIN> <ALIAS>`
- **Aliases**: `rm`, `remove`, `delete`, `del`
- **Source code**: [`src/cli/alias/unset.rs`](https://github.com/jdx/mise/blob/main/src/cli/alias/unset.rs)

Clears an alias for a plugin

This modifies the contents of ~/.config/mise/config.toml

## Arguments

### `<PLUGIN>`

The plugin to remove the alias from

### `<ALIAS>`

The alias to remove

Examples:

```
mise alias unset node lts-jod
```
