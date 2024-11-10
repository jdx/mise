# `mise alias`

- **Usage**: `mise alias [-p --plugin <PLUGIN>] [--no-header] <SUBCOMMAND>`
- **Aliases**: `a`
- **Source code**: [`src/cli/alias.rs`](https://github.com/jdx/mise/blob/main/src/cli/alias.rs)

Manage aliases

## Flags

### `-p --plugin <PLUGIN>`

filter aliases by plugin

### `--no-header`

Don't show table header

## Subcommands

- [`mise alias get <PLUGIN> <ALIAS>`](/cli/alias/get.md)
- [`mise alias ls [--no-header] [TOOL]`](/cli/alias/ls.md)
- [`mise alias set <ARGS>…`](/cli/alias/set.md)
- [`mise alias unset <PLUGIN> <ALIAS>`](/cli/alias/unset.md)
