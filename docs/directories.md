# Directory Structure

mise separates configuration, installed tools, disposable metadata, and machine-local state.
Use the table to locate files, then see the sections below for what is safe to share or remove.
These are defaults when no `MISE_*` or `XDG_*` directory overrides are set.

| Purpose                     | Linux                 | macOS                   | Windows                           | Override          |
| --------------------------- | --------------------- | ----------------------- | --------------------------------- | ----------------- |
| Global configuration        | `~/.config/mise`      | `~/.config/mise`        | `%USERPROFILE%\.config\mise`      | `MISE_CONFIG_DIR` |
| Cache                       | `~/.cache/mise`       | `~/Library/Caches/mise` | `%TEMP%\mise`                     | `MISE_CACHE_DIR`  |
| Local state                 | `~/.local/state/mise` | `~/.local/state/mise`   | `%USERPROFILE%\.local\state\mise` | `MISE_STATE_DIR`  |
| Installed tools and plugins | `~/.local/share/mise` | `~/.local/share/mise`   | `%LOCALAPPDATA%\mise`             | `MISE_DATA_DIR`   |

`mise cache path` prints the cache path in use. `mise doctor` reports the resolved mise
directories. Set directory overrides in the environment that starts mise, and keep them
consistent between shells, editors, and CI jobs.

Keep these directories separate. In particular, do not point `MISE_CACHE_DIR` at a directory
containing configuration or installed tools: cache cleanup removes its contents.

## `~/.config/mise`

- Override: `$MISE_CONFIG_DIR`
- Default: `${XDG_CONFIG_HOME:-$HOME/.config}/mise`

Stores global configuration, normally `config.toml`. You can version portable configuration
in a dotfiles repository, but keep credentials and machine-specific values out of shared files.
Project configuration lives alongside the project; see [configuration](/configuration.html).

## `~/.cache/mise`

- Override: `$MISE_CACHE_DIR`
- Default: `${XDG_CACHE_HOME:-$HOME/.cache}/mise`, _macOS: `~/Library/Caches/mise`._

Stores the internal cache mise uses for things like the list of all available versions of a
tool. Use `mise cache clear` to clear metadata while no installs are in progress. This does
not uninstall tools when the cache and install directories are separate.
See [Cache Behavior](/cache-behavior) for more information.

## `~/.local/state/mise`

- Override: `$MISE_STATE_DIR`
- Default: `${XDG_STATE_HOME:-$HOME/.local/state}/mise`

Stores trust records, tracked configuration paths, and the encrypted environment cache.
Keep this local to the machine. Removing it loses trust decisions and other state; use
`mise cache clear` when you only need to refresh cached values.

## `~/.local/share/mise`

- Override: `$MISE_DATA_DIR`
- Default: `${XDG_DATA_HOME:-$HOME/.local/share}/mise`

Contains tool installations, plugins, shims, and command wrappers. Do not have asdf and mise
manage the same installation directory. Some filesystem layouts are similar, but the tools
do not coordinate changes to shared state.

For CI, installed-tool caches must match the OS, architecture, configuration, and any native
library requirements. Copying this directory between arbitrary machines is not a portable
installation method.

### `~/.local/share/mise/downloads`

This is where plugins may write downloaded assets such as tarballs during installation. mise removes these files by
default after install/uninstall; set `always_keep_download` to keep them for debugging backend/plugin install behavior.
This directory is not a supported download cache. Some backends may skip a download when the expected file already exists,
but that behavior is backend-specific and not guaranteed. Cache `~/.local/share/mise/installs` instead if you want to
avoid reinstalling tools in CI or offline workflows.

### `~/.local/share/mise/plugins`

mise installs plugins to this directory when running `mise plugins install`. If you are working on a
plugin, use a symlink to your checkout when no plugin is already installed at that path:

```sh
mkdir -p ~/.local/share/mise/plugins
ln -s ~/src/mise-my-tool ~/.local/share/mise/plugins/my-tool
```

### `~/.local/share/mise/installs`

Stores installed tool versions. For example, `mise install node@24.0.0` installs into
`installs/node/24.0.0` under the data directory. mise may also create version-prefix and alias
symlinks that point at concrete installations. Use `mise where node` or `mise which node` to
find the selected installation or executable, rather than constructing a path from an alias.

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

### `~/.local/share/mise/command-wrappers/bin`

This is where mise places dispatch shims configured by `[wrappers]`. mise manages
this directory; use `mise reshim` after adding or removing a command wrapper.

## System installs and shims

System installs default to `/usr/local/share/mise/installs` and system shims
default to `/usr/local/share/mise/shims`. Their locations can be changed with
the global-only `system_installs_dir` and `system_shims_dir` settings or the
`MISE_SYSTEM_INSTALLS_DIR` and `MISE_SYSTEM_SHIMS_DIR` environment variables.
Both paths expand `~` and must resolve to absolute paths.

`mise install --system` writes to the system install root and
`mise reshim --system` rebuilds system shims. mise never elevates privileges
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

When the shim paths are equal, mise manages one combined set of shims with one lock. When
the install roots are equal, the root is treated as local storage rather than
being scanned and classified twice.
