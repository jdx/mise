---
description: Find where daemon data and generated config live, keep data in a checkout, reset a database, and prune deleted projects.
socialDescription: Find, reset, and prune daemon data.
---

# Data and cleanup <Badge type="warning" text="experimental" />

By default, mise keeps each project's daemon state and preset data outside the
project, under `$MISE_STATE_DIR/daemons/`, and never deletes it on its own. Use
`mise daemons ls --json` to find it and `mise daemons prune` to remove what
deleted projects and worktrees left behind.

::: warning Experimental
Daemons are experimental. Enable them with `experimental = true` under
`[settings]`; see [Requirements](/daemons.html#requirements).
:::

## Where data lives

mise writes each project's generated pitchfork config to
`$MISE_STATE_DIR/daemons/<project-hash>/` and registers it with pitchfork. See
[Directories](/directories.html) for where `$MISE_STATE_DIR` is.
By default, nothing is written into the project tree. A registered file
overrides an ordinary pitchfork definition with the same daemon ID. To change a
daemon, edit its `[daemons]` declaration, not the generated file.

A preset keeps its data in `data/<daemon-name>/` beside the generated config.
The data survives version request changes and the removal of the daemon's
declaration. A new major version needs an explicit migration or reset:
incompatible data fails before the daemon starts. Use the database's own
migration tools to keep data across an incompatible upgrade.

If first-time initialization fails, mise moves no data into place, so the next
start initializes again from scratch.

[Shared server providers](/daemons/sharing.html#shared-server-providers) keep
their data separately, in `$MISE_STATE_DIR/daemon-providers/<name>/data`.

## Keep data in the checkout

To keep a preset's data inside each checkout, set `data_dir`:

```toml
[daemons.postgres]
preset = "postgres"
version = "18"
data_dir = ".data/postgres"
```

A relative path resolves from the declaring project's root; an absolute path
also works, and `~/` expands to your home directory. Add `/.data/` to
`.gitignore`. Each worktree then owns its data, while runtime state and the
generated config stay in mise's state directory. Do not point two running
instances at the same directory.

Changing `data_dir` does not move existing data. Stop the daemon before you
copy or migrate its data, and keep a backup until the new location works.
`mise daemons prune` leaves data outside the state directory alone, but
removing a worktree or cleaning ignored files deletes data stored inside it.

## Find daemon data

Use `mise daemons ls --json` to locate a project's daemon data and check its
size before you delete the project or a worktree. Each daemon row includes:

| Field             | Value                                                                                     |
| ----------------- | ----------------------------------------------------------------------------------------- |
| `root`            | Project directory                                                                         |
| `data_dir`        | Resolved preset data directory (`null` for custom commands and provider consumers)        |
| `state_dir`       | Directory with the generated config, state, and default data                              |
| `data_size`       | Total size of default data under `state_dir` in bytes (excludes `data_dir` paths you set) |
| `data_size_human` | The same size formatted for display                                                       |

Except for `data_dir`, these fields describe the whole project, so daemons of
the same project report the same values.

## Reset a database

Stop the daemon, find its data directory, remove that directory, and start the
daemon again. The next start initializes fresh data. Back up anything you want
to keep first.

```sh
mise daemons stop db
mise daemons ls --json | jq -r '.[] | select(.name == "db") | .data_dir'
rm -rf <that directory>
mise daemons start db
```

Remove the directory itself, not only its contents: mise refuses to start a
preset whose data directory exists without the marker it writes during
initialization.

This applies to a daemon that runs its own preset. A daemon that uses a
[shared server provider](/daemons/sharing.html#shared-server-providers) has no
data directory of its own, so its `data_dir` is `null`. Drop its database with
the server's own client instead. Never remove the provider's data directory to
reset one checkout: it holds every checkout's database.

## Clean up deleted projects

Each project, including each linked Git worktree, has its own daemon state and
data. Deleting the project directory, for example with `git worktree remove`,
leaves that data on disk and its daemons registered with pitchfork.

Use [`mise daemons prune`](/cli/daemons/prune.html) to clean up after deleted
projects. It scans all project state under `$MISE_STATE_DIR/daemons/`:

```sh
# Preview the projects, state directories, and sizes
mise daemons prune --dry-run

# Review the list and confirm removal
mise daemons prune
```

Pruning stops the affected daemons, unregisters their generated config, and
deletes their config, state, and data. The confirmation prompt shows how many
state directories it removes and their total size, and it defaults to no.
Removal cannot be undone, so back up any data you want to keep.

State for projects that still exist is kept, even if they no longer declare any
daemons. `mise daemons start` prints a reminder when it finds state that prune
would remove; it does not delete anything itself.

### Non-interactive cleanup

Pass the global `--yes` flag to confirm ordinary removals without a prompt:

```sh
mise daemons prune --yes
```

If a missing project could be on an unmounted volume or behind a deleted
symlink, mise asks about it separately, and `--yes` skips it. Run without
`--yes` to review these entries, and confirm only if the project itself was
deleted. This extra check applies when a parent directory was deleted, an
ancestor is empty or unreadable, the project is rooted at a mount point such as
`/Volumes/Disk`, or the recorded path differs from the one used to create the
state directory.

### When state is kept

Pruning requires pitchfork. mise keeps a project's state, and says why, when it
cannot read the project path or state file, take the project lock, confirm that
the daemons have stopped, or unregister their config. It also keeps state if
the project directory reappears before removal.

A database's PID or lock file can also block removal. A PID file naming a live
process blocks pruning; a stale PID does not. An unreadable marker, or one
without a valid PID, counts as possibly active. Resolve the reported condition
and run `mise daemons prune` again.
