# `mise outdated`

**Usage**: `mise outdated [FLAGS] [TOOL@VERSION]...`

**Source code**: [`src/cli/outdated.rs`](https://github.com/jdx/mise/blob/main/src/cli/outdated.rs)

Shows outdated tool versions

See `mise upgrade` to upgrade these versions.

## Arguments

### `[TOOL@VERSION]...`

Tool(s) to show outdated versions for
e.g.: node@20 python@3.10
If not specified, all tools in global and local configs will be shown

## Flags

### `-l --bump`

Compares against the latest versions available, not what matches the current config

For example, if you have `node = "20"` in your config by default `mise outdated` will only
show other 20.x versions, not 21.x or 22.x versions.

Using this flag, if there are 21.x or newer versions it will display those instead of 20.x.

### `-J --json`

Output in JSON format

### `--no-header`

Don't show table header

Examples:

    $ mise outdated
    Plugin  Requested  Current  Latest
    python  3.11       3.11.0   3.11.1
    node    20         20.0.0   20.1.0

    $ mise outdated node
    Plugin  Requested  Current  Latest
    node    20         20.0.0   20.1.0

    $ mise outdated --json
    {"python": {"requested": "3.11", "current": "3.11.0", "latest": "3.11.1"}, ...}
