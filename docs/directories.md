# Directory Structure

mise uses the following directories.

::: tip
If you often find yourself using these directories (as I do), I suggest setting all of them to `~/.mise` for easy access.
:::

## `~/.config/mise`

- Override: `$MISE_CONFIG_DIR`
- Default: `${XDG_CONFIG_HOME:-$HOME/.config}/mise`

This directory stores the global config file `~/.config/mise/config.toml`. It is intended to go into your
dotfiles repo so it can be shared across machines.

## `~/.cache/mise`

- Override: `$MISE_CACHE_DIR`
- Default: `${XDG_CACHE_HOME:-$HOME/.cache}/mise`, _macOS: `~/Library/Caches/mise`._

Stores the internal cache mise uses for things like the list of all available versions of a
tool. Do not share this across machines. You may delete this directory any time mise isn't actively installing something;
do so with `mise cache clear`.
See [Cache Behavior](/cache-behavior) for more information.

## `~/.local/state/mise`

- Override: `$MISE_STATE_DIR`
- Default: `${XDG_STATE_HOME:-$HOME/.local/state}/mise`

Stores state local to the machine, such as which config files are trusted. This should not be shared across
machines.

## `~/.local/share/mise`

- Override: `$MISE_DATA_DIR`
- Default: `${XDG_DATA_HOME:-$HOME/.local/share}/mise`

This is the main directory mise uses; plugins and tools are installed here.
It is nearly identical to asdf's `~/.asdf`, so much so that you may be able to get by
symlinking the two together and using asdf and mise simultaneously. (Supporting this isn't a
project goal, however.)

This directory _could_ be shared across machines, but only if they run the same OS/arch. In general I wouldn't advise
doing so.

### `~/.local/share/mise/downloads`

This is where plugins may write downloaded assets such as tarballs during installation. mise removes these files by
default after install/uninstall; set `always_keep_download` to keep them for debugging backend/plugin install behavior.
This directory is not a supported download cache. Some backends may skip a download when the expected file already exists,
but that behavior is backend-specific and not guaranteed. Cache `~/.local/share/mise/installs` instead if you want to
avoid reinstalling tools in CI or offline workflows.

### `~/.local/share/mise/plugins`

mise installs plugins to this directory when running `mise plugins install`. If you are working on a
plugin, I suggest symlinking it manually by running:

```sh
ln -s ~/src/mise-my-tool ~/.local/share/mise/plugins/my-tool
```

### `~/.local/share/mise/installs`

This is where tools are installed when running `mise install`. For example,
`mise install node@20.0.0` installs to `~/.local/share/mise/installs/node/20.0.0`.

mise also creates symlinks in this directory for version prefixes ("20" and "20.15")
and matching aliases ("lts", "latest").
For example:

```sh
$ tree ~/.local/share/mise/installs/node
20 -> ./20.15.0
20.15 -> ./20.15.0
lts -> ./20.15.0
latest -> ./20.15.0
```

You can set the `MISE_INSTALLS_DIR` environment variable to override this location.

`MISE_INSTALLS_DIR` is read when mise starts. Set it in the environment before invoking mise and keep
it set for later mise and shim invocations. Do not set it in the `[env]` section of `mise.toml`: `[env]`
describes the environment mise exports, after mise has already selected its installation directory.
Setting it there can make an install use one directory while later commands and shims look in another.

### `~/.local/share/mise/shims`

This is where mise places shims. Generally these are used for IDE integration or when `mise activate`
does not work for some reason.

- Setting: `shims_dir`
- Environment override: `MISE_SHIMS_DIR`

The setting is global-only, expands `~`, and must resolve to an absolute path.

`mise reshim` can publish into a shared executable directory such as `~/.local/bin` or
`/usr/local/bin`; it only replaces or removes entries it recognizes as mise shims. Other mise
features still filter shim directories as whole `PATH` entries, so use a dedicated directory when
using `mise activate`, hook-env, or internal dependency lookups.

## System installs and shims

System installs default to `/usr/local/share/mise/installs` and system shims
default to `/usr/local/share/mise/shims`. Their locations can be changed with
the global-only `system_installs_dir` and `system_shims_dir` settings or the
`MISE_SYSTEM_INSTALLS_DIR` and `MISE_SYSTEM_SHIMS_DIR` environment variables.
Both paths expand `~` and must resolve to absolute paths.

`mise install --system` writes to the system install root and
`mise reshim --system` rebuilds the system farm. mise never elevates privileges
automatically. Preinstall with the necessary privileges or redirect these
settings when the defaults are not writable.

Distributions may collocate system and user storage while retaining system
configuration. For example, Omarchy can keep every tool artifact in the user's
home directory:

```toml
[settings]
system_installs_dir = "~/.local/share/mise/installs"
shims_dir = "~/.local/share/mise/shims"
system_shims_dir = "~/.local/share/mise/shims"
```

When the shim paths are equal, mise manages one union farm with one lock. When
the install roots are equal, the root is treated as local storage rather than
being scanned and classified twice.

### `~/.local/share/mise/command-wrappers/bin`

This is where mise places dispatch shims configured by `[wrappers]`. mise manages
this directory; use `mise reshim` after adding or removing a command wrapper.
