# `mise alias ls`

**Usage**: `mise alias ls [--no-header] [PLUGIN]`

**Source code**: [`src/cli/alias/ls.rs`](https://github.com/jdx/mise/blob/main/src/cli/alias/ls.rs)

**Aliases**: `list`

List aliases
Shows the aliases that can be specified.
These can come from user config or from plugins in `bin/list-aliases`.

For user config, aliases are defined like the following in `~/.config/mise/config.toml`:

    [alias.node]
    lts = "22.0.0"

## Arguments

### `[PLUGIN]`

Show aliases for &lt;PLUGIN>

## Flags

### `--no-header`

Don't show table header

Examples:

    $ mise aliases
    node  lts-jod      22
