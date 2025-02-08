# Bun

`mise` can be used to install and manage multiple versions of [bun](https://bun.sh/) on the same system. 

> The following are instructions for using the bun mise core plugin. This is used when there isn't a
git plugin installed named "bun".

The code for this is inside the mise repository at
[`./src/plugins/core/bun.rs`](https://github.com/jdx/mise/blob/main/src/plugins/core/bun.rs).

## Usage

The following installs bun and makes it the global default:

```sh
mise use -g bun@0.7     # install bun 0.7.x
mise use -g bun@latest  # install latest bun
```

See available versions with `mise ls-remote bun`.

> [!NOTE]
> Avoid using `bun upgrade` to upgrade bun as `mise` will not be aware of the change.
