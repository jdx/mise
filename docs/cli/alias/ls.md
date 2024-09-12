## `mise alias ls [OPTIONS] [PLUGIN]`

**Aliases:** `list`

```text
List aliases
Shows the aliases that can be specified.
These can come from user config or from plugins in `bin/list-aliases`.

For user config, aliases are defined like the following in `~/.config/mise/config.toml`:

  [alias.node]
  lts = "20.0.0"

Usage: alias ls [OPTIONS] [PLUGIN]

Arguments:
  [PLUGIN]
          Show aliases for <PLUGIN>

Options:
      --no-header
          Don't show table header

Examples:

    $ mise aliases
    node    lts-hydrogen   20.0.0
```
