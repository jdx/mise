---
description: "Install and remove MacPorts ports on macOS with port from mise.toml."
---

# MacPorts (macports)

The `macports` manager installs and removes [MacPorts](https://www.macports.org/)
ports on macOS with `port`. It uses [sudo](/bootstrap/packages/#sudo) when mise
is not running as root.

```toml
[bootstrap.packages]
"macports:git" = "latest"
"macports:ripgrep" = "latest"
"macports:bc" = { state = "absent" }
```

```sh
mise bootstrap packages apply --manager macports --dry-run
mise bootstrap packages apply --manager macports
```

## Prerequisites

The manager is available on macOS when `port` is on `PATH` or at
`/opt/local/bin/port`. The MacPorts installer adds that directory to `PATH` only
in shell profiles, so mise also looks there directly. On other machines, its
entries show as [`skipped`](/bootstrap/packages/#choose-platforms).

mise does not install MacPorts. To set it up on a new Mac, install it from a
[`pre-packages` hook](/bootstrap/#hooks) with the
[MacPorts installer package](https://www.macports.org/install.php).

`macports` is not in the default order for
[unprefixed packages](/bootstrap/packages/#unprefixed-packages). To use it for
them, list it in
[`system_packages.prefer`](/configuration/settings.html#system_packages.prefer).

## Package names

Use the port name, such as `git` or `py312-numpy`. Names are case-insensitive.
Because `port` has no `--` separator and reads its arguments as options,
variants (`+name`), version selectors (`@1.0`), globs, and pseudo-ports such as
`outdated`, mise rejects any name that is not a plain port name before it runs
`port`. Variants are not supported; install a port with specific variants
yourself.

## Version pins

A ports tree holds only the current Portfile of each port, so mise cannot
install an older version. A pin is checked against the active installation and
may be the version (`2.47.0`), `version_revision` (`2.47.0_1`), or the whole
spec with variants (`2.47.0_1+perl5_34`). A mismatched pin shows in `status`; an
entry with a pin is skipped by `apply` with a warning.

## What mise runs

| Operation                       | Command                                        |
| ------------------------------- | ---------------------------------------------- |
| Check installed state (no sudo) | `port -q installed <ports>`                    |
| Install                         | `port -N install <ports>`                      |
| `apply --update`                | `port -N sync` first                           |
| Upgrade                         | `port -N sync`, then `port -N upgrade <ports>` |
| Remove                          | `port -N uninstall <ports>`                    |

`-N` makes `port` fail instead of asking a question. `sync` refreshes the ports
tree only; mise never runs `port selfupdate`, so it does not upgrade MacPorts
itself.

Only the active version of a port counts as installed. A port that is installed
but inactive is reported as missing, and `port install` activates it again.

## Remove packages

```toml
[bootstrap.packages]
"macports:bc" = { state = "absent" }
```

`apply` runs `port -N uninstall` for an installed port. `port` refuses to
uninstall a port that other installed ports depend on, and mise reports that
error; mise does not pass `--follow-dependents`.
