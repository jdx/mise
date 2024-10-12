# Directory Structure

The following are the directories that mise uses.

::: tip
If you often find yourself using these directories (as I do), I suggest setting all of them to `~/.mise` for easy access.
:::

## `~/.config/mise`

- Override: `$MISE_CONFIG_DIR`
- Default: `${XDG_CONFIG_HOME:-$HOME/.config}/mise`

This directory stores the global configuration file `~/.config/mise/config.toml`. This is intended to go into your
dotfiles repo to share across machines.

## `~/.cache/mise`

- Override: `$MISE_CACHE_DIR`
- Default: `${XDG_CACHE_HOME:-$HOME/.cache}/mise`, _macOS: `~/Library/Caches/mise`._

Stores internal cache that mise uses for things like the list of all available versions of a
plugin. Do not share this across machines. You may delete this directory any time mise isn't actively installing something.
Do this with `mise cache clear`.
See [Cache Behavior](/cache-behavior) for more information.

## `~/.local/state/mise`

- Override: `$MISE_STATE_DIR`
- Default: `${XDG_STATE_HOME:-$HOME/.local/state}/mise`

Used for storing state local to the machine such as which config files are trusted. These should not be shared across
machines.

## `~/.local/share/mise`

- Override: `$MISE_DATA_DIR`
- Default: `${XDG_DATA_HOME:-$HOME/.local/share}/mise`

This is the main directory that mise uses and is where plugins and tools are installed into.
It is nearly identical to `~/.asdf` in asdf, so much so that you may be able to get by
symlinking these together and using asdf and mise simultaneously. (Supporting this isn't a
project goal, however).

This directory _could_ be shared across machines but only if they run the same OS/arch. In general I wouldn't advise
doing so.

### `~/.local/share/mise/downloads`

This is where plugins may optionally cache downloaded assets such as tarballs. Use the
`always_keep_downloads` setting to prevent mise from removing files from here.

### `~/.local/share/mise/plugins`

mise installs plugins to this directory when running `mise plugins install`. If you are working on a
plugin, I suggest
symlinking it manually by running:

```sh
ln -s ~/src/mise-my-tool ~/.local/share/mise/plugins/my-tool
```

### `~/.local/share/mise/installs`

This is where tools are installed to when running `mise install`. For example, `mise install
node@20.0.0` will install to `~/.local/share/mise/installs/node/20.0.0`

This will also create other symlinks to this directory for version prefixes ("20" and "20.15")
and matching aliases ("lts", "latest").
For example:

```sh
$ tree ~/.local/share/mise/installs/node
20 -> ./20.15.0
20.15 -> ./20.15.0
lts -> ./20.15.0
latest -> ./20.15.0
```

### `~/.local/share/mise/shims`

This is where mise places shims. Generally these are used for IDE integration or if `mise activate`
does not work for some reason.
