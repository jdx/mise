---
description: "Find where mise keeps config, installed tools, caches, and state, and which MISE_*_DIR variable moves each one."
socialDescription: "Find where mise keeps config, installed tools, caches, and state, and how to move them."
---

# Directories

mise keeps config, installed tools, cache, and machine-local state in separate
directories. The defaults below apply when no `MISE_*_DIR` or `XDG_*` variable
overrides them.

| Purpose                     | Linux                         | macOS                   | Windows                           | Override                                                                                                                          |
| --------------------------- | ----------------------------- | ----------------------- | --------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| Global config               | `~/.config/mise`              | `~/.config/mise`        | `%USERPROFILE%\.config\mise`      | `MISE_CONFIG_DIR`; otherwise `XDG_CONFIG_HOME` + `/mise`                                                                          |
| Cache                       | `~/.cache/mise`               | `~/Library/Caches/mise` | `%TEMP%\mise`                     | `MISE_CACHE_DIR`; otherwise `XDG_CACHE_HOME` + `/mise`                                                                            |
| Local state                 | `~/.local/state/mise`         | `~/.local/state/mise`   | `%USERPROFILE%\.local\state\mise` | `MISE_STATE_DIR`; otherwise `XDG_STATE_HOME` + `/mise`                                                                            |
| Installed tools and plugins | `~/.local/share/mise`         | `~/.local/share/mise`   | `%LOCALAPPDATA%\mise`             | `MISE_DATA_DIR`; otherwise `XDG_DATA_HOME` + `/mise`                                                                              |
| System config               | `/etc/mise`                   | `/etc/mise`             | `\etc\mise` on the current drive  | `MISE_SYSTEM_CONFIG_DIR` (older name: `MISE_SYSTEM_DIR`)                                                                          |
| Temporary files             | `$TMPDIR/mise` or `/tmp/mise` | `$TMPDIR/mise`          | `%TEMP%\mise-tmp`                 | `MISE_TMP_DIR`; otherwise `mise` in the system temporary directory, or `mise-tmp` when `mise` there is inside the cache directory |

These directories live inside the data directory and can be moved on their own:

| Directory        | Default                       | Override                                                                 |
| ---------------- | ----------------------------- | ------------------------------------------------------------------------ |
| Installed tools  | `installs/`                   | `MISE_INSTALLS_DIR`                                                      |
| Install store    | `installs/` (`i\` on Windows) | `MISE_INSTALL_STORE_DIR`; see [installs](#local-share-mise-installs)     |
| Plugins          | `plugins/`                    | `MISE_PLUGINS_DIR`                                                       |
| Downloads        | `downloads/`                  | `MISE_DOWNLOADS_DIR`                                                     |
| Shims            | `shims/`                      | [`shims_dir`](/configuration/settings.html#shims_dir) (`MISE_SHIMS_DIR`) |
| Command wrappers | `command-wrappers/bin/`       | None                                                                     |

`mise cache path` prints the cache directory in use, and `mise doctor` reports
the resolved data, config, cache, state, and shims directories. Set these
variables in the environment that starts mise, such as your shell startup file
or CI job, and keep them the same for shells, editors, and CI. Do not set them
in `[env]`: mise chooses its directories before it reads `mise.toml`, so a value
there can make one command use a different directory than the next.

Keep the directories separate. In particular, do not point `MISE_CACHE_DIR` at a
directory that holds config or installed tools: clearing the cache removes its
contents.

## `~/.config/mise` {#config-mise}

Holds [global config](/configuration.html#global-config), normally
`config.toml`, along with `conf.d/` fragments and `miserc.toml`. You can keep
portable config in a dotfiles repository, but leave credentials and
machine-specific values out of shared files. Project config lives in the
project; see [mise.toml](/configuration.html#mise-toml).

## `~/.cache/mise` {#cache-mise}

Holds data mise can rebuild, such as the list of available versions of each
tool and cached `exec()` output. `mise cache clear` removes it; run it while no
install is in progress. Clearing the cache does not uninstall tools as long as
the cache and data directories are separate. See [Caches](/cache-behavior.html).

## `~/.local/state/mise` {#local-state-mise}

Holds trust decisions, tracked config paths, the encrypted
[environment cache](/cache-behavior.html#environment-caching), task freshness
records, and state for [dotfiles](/dotfiles.html) and their
[history](/dotfiles/history.html), [daemons](/daemons.html),
[project dependencies](/dev-tools/deps.html), bootstrap packages, and packslip
signer pins. Keep it local to the machine. Deleting it loses that state; use
`mise cache clear` when you only need to refresh cached values.

## `~/.local/share/mise` {#local-share-mise}

Holds tool installations, plugins, shims, and command wrappers. Do not let asdf
and mise manage the same data directory: some layouts look alike, but the two
do not coordinate changes.

Installed tools depend on the OS, architecture, config, and native libraries of
the machine that installed them. Copying this directory between unrelated
machines is not a supported way to install tools. For caching it in CI, see
[Continuous integration](/continuous-integration.html).

### `downloads` {#local-share-mise-downloads}

Backends may write archives here while they install a tool. mise deletes them
afterwards unless
[`always_keep_download`](/configuration/settings.html#always_keep_download) is
set. This is not a download cache. To avoid reinstalling tools in CI, cache
`installs/` instead, together with the install store when it is a separate
directory.

### `plugins` {#local-share-mise-plugins}

`mise plugins install` installs plugins here. To work on a plugin, link your
checkout:

```sh
mise plugins link my-tool ~/src/mise-my-tool
```

### `installs` {#local-share-mise-installs}

Holds installed versions: `mise install node@24.0.0` installs into
`installs/node/24.0.0`. mise also creates links for prefixes and aliases, such
as `installs/node/24`. Use `mise where node` or `mise which node` instead of
building paths from them.

`MISE_INSTALLS_DIR` moves this directory. Like the other directory variables, set
it where mise starts, not in `[env]`, or an install can use one directory while
later commands and shims look in another.

The experimental [identity install layout](/dev-tools/install-layout.html)
installs into `installs/<label>-<hash>/`, such as `installs/age-hlencrst`, and
makes `installs/age/1.2.1` a link to it. Its catalog in `installs/.mise/`
records which directory holds each installation; it is not a cache, so back it
up with the installs. On Windows the installations go into `i\` beside
`installs` (`%LOCALAPPDATA%\mise\i`), a shorter path that leaves more room under
the 260-character limit. `MISE_INSTALL_STORE_DIR` chooses where installations go
on any platform; a directory inside `installs/` is ignored.

### `shims` {#local-share-mise-shims}

Shims let editors, scripts, and non-interactive shells run mise-managed tools
without `mise activate`. See [Shims](/dev-tools/shims.html).

[`shims_dir`](/configuration/settings.html#shims_dir) moves this directory. It
can be set only in global config, expands `~`, and must resolve to an absolute
path. `mise reshim` can publish shims into a shared directory such as
`~/.local/bin`, and replaces or removes only entries it recognizes as mise
shims. mise still treats shim directories as whole `PATH` entries in other
places, so use a dedicated directory if you also use `mise activate`.

### `command-wrappers/bin` {#local-share-mise-command-wrappers-bin}

Holds the dispatch shims for [`[wrappers]`](/dev-tools/shims.html#command-wrappers).
mise manages this directory; run `mise reshim` after adding or removing a
wrapper.

## System config {#system-config}

`/etc/mise` holds config for every user on the machine, such as
`/etc/mise/config.toml`, `conf.d/` fragments, and `miserc.toml`. It has the
lowest precedence; see
[Global and system config](/configuration.html#global-config).

## System installs and shims {#system-installs-and-shims}

`mise install --system` installs into `/usr/local/share/mise/installs` and
`mise reshim --system` writes shims to `/usr/local/share/mise/shims`.
`MISE_SYSTEM_DATA_DIR` moves both at once (default `/usr/local/share/mise`), and
[`system_installs_dir`](/configuration/settings.html#system_installs_dir)
(`MISE_SYSTEM_INSTALLS_DIR`) and
[`system_shims_dir`](/configuration/settings.html#system_shims_dir)
(`MISE_SYSTEM_SHIMS_DIR`) move each one. On Unix, mise runs `sudo` to write
there when the directories are not writable. See
[System installs](/dev-tools/system-installs.html).
