# `mise plugins ls-remote`

**Usage**: `mise plugins ls-remote [-u --urls] [--only-names]`

**Source code**: [`src/cli/plugins/ls-remote.rs`](https://github.com/jdx/mise/blob/main/src/cli/plugins/ls-remote.rs)

**Aliases**: `list-remote`, `list-all`

List all available remote plugins

The full list is here: <https://github.com/jdx/mise/blob/main/registry.toml>

Examples:

    mise plugins ls-remote

## Flags

### `-u --urls`

Show the git url for each plugin e.g.: <https://github.com/mise-plugins/mise-poetry.git>

### `--only-names`

Only show the name of each plugin by default it will show a "*" next to installed plugins
