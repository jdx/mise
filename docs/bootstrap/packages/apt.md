---
description: "Install Debian and Ubuntu packages with apt-get from mise.toml, including version pins."
---

# Debian and Ubuntu packages (apt)

The `apt` manager installs packages on Debian, Ubuntu, and other Debian-based
distributions with `apt-get`. It checks installed state with `dpkg-query` and
uses [sudo](/bootstrap/packages/#sudo) when mise is not running as root.

```toml
[bootstrap.packages]
"apt:libssl-dev" = "latest"
"apt:curl" = "8.5.0-2ubuntu10" # version pin
"apt:gcc:arm64" = "latest"     # architecture qualifier
```

```sh
mise bootstrap packages apply --manager apt --dry-run
mise bootstrap packages apply --manager apt
```

## Prerequisites

The manager is available on Linux when `apt-get` is on `PATH`. On other
machines, its entries show as
[`skipped`](/bootstrap/packages/#choose-platforms).

mise sets `DEBIAN_FRONTEND=noninteractive`, so debconf questions take their
defaults. A maintainer script that reads from the terminal directly can still
prompt.

## Package names

Use the package name as `apt-get install` takes it. Append `:<arch>` to select
an architecture, as in `gcc:arm64`. The machine's dpkg architectures and apt
sources must already provide that architecture; the declaration does not enable
multiarch or add a repository.

## Version pins

mise passes a pin to apt as `name=version`. Versions are specific to a
distribution release, so the `8.5.0-2ubuntu10` pin above is only an example.
Run `apt-cache policy curl` on the target to see the candidates, and pin one
that its sources offer. mise cannot install a version your apt sources no
longer carry.

## Package lists {#metadata-refresh}

mise runs `apt-get update` before installing when `/var/lib/apt/lists` holds
no package lists (fresh containers), or when a simulated install
(`apt-get --simulate`, which needs no root) fails against the current lists,
for example because of a package or pinned version the lists do not contain. If
the simulation succeeds, mise installs without refreshing. To force a refresh
anyway:

```sh
mise bootstrap packages apply --update
```

## What mise runs

| Operation                       | Command                                                                  |
| ------------------------------- | ------------------------------------------------------------------------ |
| Check installed state (no sudo) | `dpkg-query -W <packages>`                                               |
| Install                         | `apt-get install -y -- <packages>`, after `apt-get update` when needed   |
| `apply --update`                | `apt-get update` first                                                   |
| Upgrade                         | `apt-get update`, then `apt-get install -y --only-upgrade -- <packages>` |

`upgrade` acts only on configured packages that are installed. apt still
installs any new dependencies the upgrades need.

## Remove packages

mise does not remove apt packages. Deleting an entry leaves the package
installed; run `apt-get remove` yourself.
