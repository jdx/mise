# Bun in rtx

The following are instructions for using the bun rtx core plugin. This is used when there isn't a 
git plugin installed named "bun".

The code for this is inside the rtx repository at
[`./src/plugins/core/bun.rs`](https://github.com/jdx/rtx/blob/main/src/plugins/core/bun.rs).

## Usage

The following installs bun and makes it the global default:

```sh-session
$ rtx use -g bun@0.7     # install bun 0.7.x
$ rtx use -g bun@latest  # install latest bun
```

See available versions with `rtx ls-remote bun`.
