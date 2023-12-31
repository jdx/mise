---
outline: 'deep'
---

# Directory Structure

The following are the directories that rtx uses.
These are the default directories, see
[Configuration](/configuration) for information on changing the locations.

> **Tip**
>
> If you often find yourself using these directories (as I do), I suggest setting all of them to `~/.rtx` for easy access.

## `~/.config/rtx`

This directory stores the global configuration file `~/.config/rtx/config.toml`. This is intended to go into your
dotfiles repo to share across machines.

## `~/.cache/rtx`

_On macOS this is `~/Library/Caches/rtx`._

Stores internal cache that rtx uses for things like the list of all available versions of a
plugin. Do not share this across machines. You may delete this directory any time rtx isn't actively installing something.
Do this with `rtx cache clear`.
See [Cache Behavior](/cache-behavior) for more information.

## `~/.local/state/rtx`

Used for storing state local to the machine such as which config files are trusted. These should not be shared across
machines.

## `~/.local/share/rtx`

This is the main directory that rtx uses and is where plugins and tools are installed into.
It is nearly identical to `~/.asdf` in asdf, so much so that you may be able to get by
symlinking these together and using asdf and rtx simultaneously. (Supporting this isn't a
project goal, however).

This directory _could_ be shared across machines but only if they run the same OS/arch. In general I wouldn't advise
doing so.

### `~/.local/share/rtx/downloads`

This is where plugins may optionally cache downloaded assets such as tarballs. Use the
`always_keep_downloads` setting to prevent rtx from removing files from here.

### `~/.local/share/rtx/plugins`

rtx installs plugins to this directory when running `rtx plugins install`. If you are working on a
plugin, I suggest
symlinking it manually by running:

```sh
ln -s ~/src/rtx-my-tool ~/.local/share/rtx/plugins/my-tool
```

### `~/.local/share/rtx/installs`

This is where tools are installed to when running `rtx install`. For example, `rtx install
node@20.0.0` will install to `~/.local/share/rtx/installs/node/20.0.0`

This will also create other symlinks to this directory for version prefixes ("20" and "20.15")
and matching aliases ("lts", "latest").
For example:

```sh
$ tree ~/.local/share/rtx/installs/node
20 -> ./20.15.0
20.15 -> ./20.15.0
lts -> ./20.15.0
latest -> ./20.15.0
```

### `~/.local/share/rtx/shims`

This is where rtx places shims. Generally these are used for IDE integration or if `rtx activate`
does not work for some reason.
