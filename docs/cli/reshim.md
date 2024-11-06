# `mise reshim`

- **Usage**: `mise reshim [-f --force]`
- **Source code**: [`src/cli/reshim.rs`](https://github.com/jdx/mise/blob/main/src/cli/reshim.rs)

Creates new shims based on bin paths from currently installed tools.

This creates new shims in ~/.local/share/mise/shims for CLIs that have been added.
mise will try to do this automatically for commands like `npm i -g` but there are
other ways to install things (like using yarn or pnpm for node) that mise does
not know about and so it will be necessary to call this explicitly.

If you think mise should automatically call this for a particular command, please
open an issue on the mise repo. You can also setup a shell function to reshim
automatically (it's really fast so you don't need to worry about overhead):

    npm() {
      command npm "$@"
      mise reshim
    }

Note that this creates shims for _all_ installed tools, not just the ones that are
currently active in mise.toml.

## Flags

### `-f --force`

Removes all shims before reshimming

Examples:

    $ mise reshim
    $ ~/.local/share/mise/shims/node -v
    v20.0.0
