# Zig

The following are instructions for using the zig mise core plugin.

The code for this is inside the mise repository at
[`./src/plugins/core/zig.rs`](https://github.com/jdx/mise/blob/main/src/plugins/core/zig.rs).

## Usage

The following installs zig and makes it the global default:

```sh
mise use -g zig@0.13     # install zig 0.13.x
mise use -g zig@latest  # install latest zig
```

See available versions with `mise ls-remote zig`.

## zig Language Server
The `zig` language server ([zls](https://github.com/zigtools/zls)) needs to be installed separately. 
You can install it with `mise`:
```sh
mise use -g zls@0.13
```
Note that a tagged release of `Zig` should be used with the same tagged release of `ZLS`.
