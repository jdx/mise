# `mise plugins uninstall`

- **Usage**: `mise plugins uninstall [-p --purge] [-a --all] [PLUGIN]...`
- **Aliases**: `remove`, `rm`
- **Source code**: [`src/cli/plugins/uninstall.rs`](https://github.com/jdx/mise/blob/main/src/cli/plugins/uninstall.rs)

Removes a plugin

## Arguments

### `[PLUGIN]...`

Plugin(s) to remove

## Flags

### `-p --purge`

Also remove the plugin's installs, downloads, and cache

### `-a --all`

Remove all plugins

Examples:

```
mise uninstall node
```
