---
description: "Install, move between channels, and remove Snap packages with snapd from mise.toml."
---

# Snap packages (snap)

The `snap` manager installs and removes snaps from the Snap Store with the
[`snap`](https://snapcraft.io/docs/snap-howto) command. Snaps are installed
system-wide, and mise uses [sudo](/bootstrap/packages/#sudo) when it is not
running as root.

```toml
[bootstrap.packages]
"snap:jq" = "latest"
"snap:code" = { classic = true }
"snap:node" = { channel = "22/stable", classic = true }
```

```sh
mise bootstrap packages apply --manager snap --dry-run
mise bootstrap packages apply --manager snap
```

## Prerequisites

The manager is available on Linux when `snap` is on `PATH`. On other machines,
its entries show as [`skipped`](/bootstrap/packages/#choose-platforms). mise
does not install snapd. When snapd is installed but not running, `status` and
`apply` fail with snap's own error instead of reporting every snap as missing.

## Package names

Use the name that `snap install` takes, such as `node` or `code`. A parallel
instance, such as `lxd_test`, is its own entry. Snap commands land in
`/snap/bin`, not in mise shims.

## Channels

Without a `channel`, an entry is satisfied by the snap from any channel, and
mise installs from snap's default, `latest/stable`. With one, mise installs from
that channel and reports a snap that tracks a different one as drift:

```text
snap  node  20.18.0 from 20/stable  version mismatch
```

`apply` moves it with `snap refresh --channel=<channel>`, which also replaces
the installed revision with that channel's. A channel is
`<track>/<risk>[/<branch>]`, where the risk is `stable`, `candidate`, `beta`,
or `edge`. A risk alone, such as `edge`, means `latest/edge`, and a track
alone, such as `22`, means `22/stable`. An entry with a misspelled risk is
skipped with a warning.

`snap list` does not show branch names, so for a branch channel such as
`22/edge/fix-1`, mise checks the track and risk only. A snap on another branch
of `22/edge` is not reported as drift.

## Classic confinement

A snap published with classic confinement only installs with `--classic`,
which gives it the same access to the system as a traditional package. Set
`classic = true` on the entries you accept that for. Without it, snap refuses
to install the snap and says that it needs `--classic`.

## Version pins and refreshes

snapd installs the current revision of a channel and refreshes snaps in the
background, usually four times a day, so mise cannot install or hold a specific
version. A pinned version, such as `"snap:jq" = "1.6"`, shows as
`version mismatch` while another version is installed, and `apply` and
`upgrade` skip it with a warning. Declare a channel instead, or use a track
when the publisher offers one per major version.

mise leaves the refresh schedule to snapd. To stop a snap from refreshing, run
`snap refresh --hold <name>`; see
[Managing updates](https://snapcraft.io/docs/managing-updates).

## What mise runs

| Operation                                | Command                                                  |
| ---------------------------------------- | -------------------------------------------------------- |
| Check installed state (no sudo)          | `snap list`                                              |
| Install snaps without options            | `snap install <names>`                                   |
| Install a snap with a channel or classic | `snap install [--channel=<channel>] [--classic] <name>`  |
| Move a snap to its declared channel      | `snap refresh --channel=<channel> [--classic] <name>`    |
| Upgrade                                  | `snap refresh <names>`, or one snap at a time with flags |
| Remove                                   | `snap remove <names>`                                    |

snap accepts `--channel` and `--classic` for one snap at a time, so each entry
with either option gets its own command. `apply --update` has no effect for
snap.

## Remove packages

Set `state = "absent"` to remove a snap:

```toml
[bootstrap.packages]
"snap:hello-world" = { state = "absent" }
```

mise runs `snap remove`, which keeps a snapshot of the snap's data for 31 days.
