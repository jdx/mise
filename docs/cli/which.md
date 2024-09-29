# `mise which [flags] <BIN_NAME>`

Shows the path that a bin name points to

## Arguments

### `<BIN_NAME>`

The bin name to look up

## Flags

### `--plugin`

Show the plugin name instead of the path

### `--version`

Show the version instead of the path

### `-t --tool <TOOL@VERSION>`

Use a specific tool@version
e.g.: `mise which npm --tool=node@20`

Examples:

    $ mise which node
    /home/username/.local/share/mise/installs/node/20.0.0/bin/node
    $ mise which node --plugin
    node
    $ mise which node --version
    20.0.0
