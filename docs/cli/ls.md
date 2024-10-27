# `mise ls`

**Usage**: `mise ls [FLAGS] [PLUGIN]...`

**Source code**: [`src/cli/ls.rs`](https://github.com/jdx/mise/blob/main/src/cli/ls.rs)

**Aliases**: `list`

List installed and active tool versions

This command lists tools that mise "knows about".
These may be tools that are currently installed, or those
that are in a config file (active) but may or may not be installed.

It's a useful command to get the current state of your tools.

## Arguments

### `[PLUGIN]...`

Only show tool versions from [PLUGIN]

## Flags

### `-c --current`

Only show tool versions currently specified in a mise.toml

### `-g --global`

Only show tool versions currently specified in the global mise.toml

### `-i --installed`

Only show tool versions that are installed (Hides tools defined in mise.toml but not installed)

### `-J --json`

Output in JSON format

### `-m --missing`

Display missing tool versions

### `--prefix <PREFIX>`

Display versions matching this prefix

### `--no-header`

Don't display headers

Examples:

    $ mise ls
    node    20.0.0 ~/src/myapp/.tool-versions latest
    python  3.11.0 ~/.tool-versions           3.10
    python  3.10.0

    $ mise ls --current
    node    20.0.0 ~/src/myapp/.tool-versions 20
    python  3.11.0 ~/.tool-versions           3.11.0

    $ mise ls --json
    {
      "node": [
        {
          "version": "20.0.0",
          "install_path": "/Users/jdx/.mise/installs/node/20.0.0",
          "source": {
            "type": "mise.toml",
            "path": "/Users/jdx/mise.toml"
          }
        }
      ],
      "python": [...]
    }
