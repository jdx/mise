# `mise plugins link`

**Usage**: `mise plugins link [-f --force] <NAME> [PATH]`

**Aliases**: ln

Symlinks a plugin into mise

This is used for developing a plugin.

## Arguments

### `<NAME>`

The name of the plugin
e.g.: node, ruby

### `[PATH]`

The local path to the plugin
e.g.: ./mise-node

## Flags

### `-f --force`

Overwrite existing plugin

Examples:

    # essentially just `ln -s ./mise-node ~/.local/share/mise/plugins/node`
    $ mise plugins link node ./mise-node

    # infer plugin name as "node"
    $ mise plugins link ./mise-node
