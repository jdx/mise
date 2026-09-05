# Bootstrap generations

Every mutating bootstrap command records a **generation**: a named machine
state you can list, inspect, and compare. A generation holds what ran, a
snapshot of the global config directory and `dotfiles.root` taken before and
after the run, the global lockfile, and a journal of what the run changed.

```sh
mise bootstrap generations               # newest first
mise bootstrap generations show latest   # one generation in detail
mise bootstrap generations show 12 --files
mise bootstrap generations diff 11 12    # what changed between two runs
```

Generations are what make a bootstrap run a known transition rather than an
unrecorded mutation: after `mise bootstrap`, `mise bootstrap dotfiles apply`,
any other `mise bootstrap <part> apply`, or a command that changes managed
files or config in place (`dotfiles add`, `unapply`, `edit`, `packages use`,
`import`, brew `tap`), there is a record of the state the machine was in
before and the state it reached — when recording is enabled and, for the
content snapshot, when git is available.

## What is recorded

| Field      | Contents                                                                                           |
| ---------- | -------------------------------------------------------------------------------------------------- |
| `command`  | The mise command line that ran, such as `bootstrap dotfiles apply --yes`                           |
| `snapshot` | Content snapshots of the config directory and `dotfiles.root` before and after the run             |
| `lockfile` | The global `mise.lock` (its hash, and its bytes inside the snapshot)                               |
| `journal`  | Entries describing what changed. Hook commands and the `bootstrap` task are noted as not journaled |
| `summary`  | The bootstrap parts the run covered                                                                |
| `status`   | `completed`, `failed`, or `pending` when the run did not finish                                    |

The snapshot covers two roots:

- the **config** root: the global mise config directory (`~/.config/mise`, or
  the directory holding `MISE_GLOBAL_CONFIG_FILE`), which is also where
  `mise bootstrap --from-git` checks a repository out
- the **dotfiles** root: [`dotfiles.root`](/dotfiles.html), where implied
  dotfile sources live

When both point at the same directory, or one contains the other, only one tree
is stored and the other root records how to find itself within it. A root that
does not exist is skipped. mise refuses to snapshot the home directory itself
or anything above it.

Snapshots are taken with the root's own `.gitignore` deliberately bypassed: an
ignored file is often exactly the credential or local-only setting a restore
must bring back. Regular files above 16 MiB, special files, and any mise
state, cache, or data directory nested under a root are left out, and a root
with more than 100,000 files or 1 GiB is skipped with a warning. A git
repository nested inside a root (a plugin checkout, say) is recorded as a
reference to its commit, not by content.

A run that changes nothing and records nothing in its journal leaves no
generation behind, so a daily `mise bootstrap` on a converged machine does not
accumulate entries. Dry runs never record.

## Where it lives

```text
$MISE_STATE_DIR/bootstrap/
├── generations.git/          bare git repository holding every snapshot
└── generations/
    ├── 000041.json
    └── 000042.json
```

The `bootstrap` directory is created private to your user (`0700`) because a
config directory commonly holds secrets — an `age.txt` identity, tokens in
`[env]`, files ignored by git — and the snapshot holds them too. Nothing is
encrypted. To stop recording set
[`bootstrap.generations.enabled`](/configuration/settings.html#bootstrap-generations-enabled)
to `false`; to discard everything recorded, delete the directory.

Snapshots live in a repository mise owns. Your own git checkout of
`~/.config/mise` or `~/.dotfiles` is never written to: no commits, no refs, no
index changes. The generation notes which checkout and commit each root was
at, for orientation only.

## Requirements

Content snapshots need a `git` binary. Without one — or, on macOS, when only
the `/usr/bin/git` shim is present and the Xcode Command Line Tools are not
installed — the generation is still recorded, with `snapshot.available` set
to `false` and the reason, and the lockfile is copied beside the record
instead. On Windows, dotfiles applied as copies are snapshotted as ordinary
files.

## Inspecting generations

`mise bootstrap generations` lists generations newest first:

```text
ID  Status     When              Command                          Parts     Snapshot
42  completed  2026-09-05 14:03  bootstrap dotfiles apply --yes   dotfiles  d44b1e2
41  completed  2026-09-05 09:12  bootstrap --yes                  ...       9c1f0aa
```

`--json` prints the full records; `-n` limits the count; `--pending` lists only
generations whose run did not finish. `show` prints one generation with its
roots, warnings, and journal; `show --files` lists every file in its snapshot.
Ids accept `latest` and `latest~N`.

## Comparing generations

`mise bootstrap generations diff` compares snapshots. With one id it shows what
that generation's run changed inside the roots — its snapshot before the run
against the one after. With two ids it compares the states the two runs left
behind, which is how to see what changed by hand between runs:

```sh
mise bootstrap generations diff 12          # what run 12 changed
mise bootstrap generations diff 11 12       # from the state after 11 to the state after 12
mise bootstrap generations diff 11 12 --patch
mise bootstrap generations diff 11 12 --root config/hypr
mise bootstrap generations diff 11 12 --exit-code   # exit 1 when they differ
```

Paths are prefixed by their root (`config/…`, `dotfiles/…`, and `mise.lock`).
The default output is a per-file summary; `--patch` prints the full patch, and
`--root LABEL[/PATH]` narrows it to one root or a path inside it. Journal
entries for the generations covered are printed after the diff unless
`--no-journal` is given.

A generation left `pending` means the run died before finishing. Its `before`
snapshot is intact, so the state prior to that run is still recorded.

## Retention

[`bootstrap.generations.keep`](/configuration/settings.html#bootstrap-generations-keep)
(default `20`) bounds how many generations are kept. After each run, the oldest
finished generations beyond that count are removed along with their snapshots;
pending generations and the newest completed one are always kept. Set it to
`0` to keep everything. Snapshots are content-addressed, so unchanged files
cost nothing per generation and pruning frees only what no remaining
generation refers to.

## Environment

While a generation is being recorded, mise sets `__MISE_BOOTSTRAP_GENERATION`
to its id. A `mise bootstrap` command run from a hook or the `bootstrap` task
sees it and records nothing of its own; its changes land in the parent run's
`after` snapshot.
