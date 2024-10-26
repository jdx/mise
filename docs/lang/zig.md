---
head:
  - - link
    - rel: canonical
      href: https://mise.jdx.dev/lang/zig
---

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
