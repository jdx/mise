---
description: "Install tools into a shared system directory so every user account on a machine can run one copy."
---

# System installs

Use a system install when several user accounts, or a container image built as
root, should share one copy of a tool. `mise install --system` installs into
`/usr/local/share/mise/installs`, and every user's mise uses a matching version
from there instead of installing its own.

## Install a tool for every user {#install-a-tool-for-every-user}

Run the command as your normal user, not with `sudo`:

```sh
mise install --system uv
```

On Unix, mise downloads, verifies, and unpacks the tool as you, then runs `sudo`
to move the prepared installation into the system directory. mise prints each
command it runs with `sudo`. Cache and lockfile updates stay owned by you. When
you can already write to the destination, or mise runs as root, it installs
directly without `sudo`. On Windows, mise never elevates; install into a
directory you can write to.

Installing does not select the tool. Declare its version in a config file that
the users read, such as the system config `/etc/mise/config.toml`:

```toml [/etc/mise/config.toml]
[tools]
uv = "0.12"
```

When a user's mise resolves `uv = "0.12"` and a matching version is in the
system directory, it uses that installation. To replace an installed version,
run `mise install --system --force uv`; mise prepares the replacement before it
changes the existing installation. `mise upgrade` keeps a system install in the
system directory.

::: warning
`sudo mise install --system` works, but it runs the whole installation as root
and can leave root-owned files in your home directory. mise warns when it
detects this. Run mise as your user and let it call `sudo` itself. Running mise
directly as root, for example in a container build, is supported.
:::

## Supported backends {#supported-backends}

When mise has to elevate, it supports tools from the `aqua`, `github`,
`gitlab`, `forgejo`, `http`, and `s3` backends. The tool must still work after
it moves from a temporary directory to its final location, and it must not have
a tool-level [`postinstall`](/dev-tools/#tool-postinstall-commands) command.
Symlinks inside the installation are kept; symlinks that point outside it are
rejected.

Other backends, such as `packslip`, `npm`, or the core tools, and tools that
record their own install path, such as Python virtual environments, need a
destination you can write to. These limits do not apply when mise installs
directly because the directory is writable or mise runs as root.

## Directories {#directories}

| Directory       | Default                          | Setting                                                                   | Environment variable       |
| --------------- | -------------------------------- | ------------------------------------------------------------------------- | -------------------------- |
| System data     | `/usr/local/share/mise`          |                                                                           | `MISE_SYSTEM_DATA_DIR`     |
| System installs | `$MISE_SYSTEM_DATA_DIR/installs` | [`system_installs_dir`](/configuration/settings.html#system_installs_dir) | `MISE_SYSTEM_INSTALLS_DIR` |
| System shims    | `$MISE_SYSTEM_DATA_DIR/shims`    | [`system_shims_dir`](/configuration/settings.html#system_shims_dir)       | `MISE_SYSTEM_SHIMS_DIR`    |
| System config   | `/etc/mise`                      |                                                                           | `MISE_SYSTEM_CONFIG_DIR`   |

Both settings are global-only, expand `~`, and must resolve to absolute paths.
Lazy tools declared in the system config install into the system installs
directory and publish their [bootstrap shims](/dev-tools/shims.html#lazy-tools)
in the system shim directory. See [Directories](/directories.html) for the
per-user locations.

### Share storage between system and user installs {#share-storage}

A distribution can keep its system config while storing every tool in the
user's home directory. Omarchy, for example, can do this with:

```toml
[settings]
system_installs_dir = "~/.local/share/mise/installs"
shims_dir = "~/.local/share/mise/shims"
system_shims_dir = "~/.local/share/mise/shims"
```

When the two shim directories are the same, mise manages one combined set of
shims with one lock. When the two install directories are the same, mise treats
the directory as user storage rather than scanning it twice.

## Shims for system installs {#system-shims}

`mise reshim --system` creates shims in the system shim directory for the tools
in the system installs directory. Run it after `mise install --system` if a
command's shim is missing. Like installs, shim updates use `sudo` when the
directory is not writable. [`mise activate --shims`](/dev-tools/shims.html)
puts the system shim directory on `PATH` behind the user's own.

## Permissions and sudo {#permissions-and-sudo}

- An interactive install can prompt for your `sudo` password. A noninteractive
  install, such as one in CI, fails unless `sudo` works without a password.
- For an elevated install, the destination and every existing parent directory
  must be owned by root and must not be writable by group or other users. A
  root-owned directory with the sticky bit, such as `/tmp`, is allowed. mise
  checks symlinked parents after resolving them.
- Set [`system_packages.sudo`](/configuration/settings.html#system_packages.sudo)
  to `false` to stop mise from calling `sudo`. Installs that need root then
  fail, and installs into writable directories still work.
- The elevated step runs `mise` as root without loading any config file, and it
  writes only inside the system installs and shims directories. A custom
  location must reach it through the environment, so keep
  `MISE_SYSTEM_DATA_DIR`, or `MISE_SYSTEM_INSTALLS_DIR` and
  `MISE_SYSTEM_SHIMS_DIR`, in sudo's `env_keep`. For example, in a file under
  `/etc/sudoers.d/` edited with `visudo`:

  ```text
  Defaults env_keep += "MISE_SYSTEM_DATA_DIR"
  ```

## Other shared directories {#other-shared-directories}

`mise install --shared <DIR>` installs into any directory you choose. To have
mise look for installed versions there, list the directory in
[`shared_install_dirs`](/configuration/settings.html#shared_install_dirs). mise
reads those directories but never installs into them on its own. The system
installs directory is always searched when it exists, so it does not need to be
listed.
