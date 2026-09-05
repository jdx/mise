# Dotfiles history

> [!WARNING]
> Dotfile tracking and history are experimental. Enable them with
> `mise settings experimental=true`. Interfaces and storage formats may change.

mise keeps checkpoints of your configuration files: the global mise config
directory, the dotfiles root, every `[dotfiles]` entry, and any file you
[track](/dotfiles.html#tracking-files-in-place) where it is. A checkpoint is
recorded whenever those files change, regardless of whether an editor, an
agent, or a mise command changed them: by the watcher, by every mutating
bootstrap command (before and after the run), and by
`mise bootstrap dotfiles save`. `mise bootstrap dotfiles history` browses
them; from there you can see what changed, compare versions, and find the
version of a file you want.

A checkpoint holds files, not machine state. It does not journal other mise
activity and does not reverse system effects: restoring a package
declaration does not uninstall a package, and restoring a service definition
does not restore whether that service was running. Applying configuration
stays an explicit bootstrap action.

```sh
mise bootstrap dotfiles track ~/.zshrc ~/.config/hypr   # adopt files where they are
mise bootstrap dotfiles history                                            # newest first
mise bootstrap dotfiles history --path ~/.config/hypr/bindings.lua        # only where that file changed
mise bootstrap dotfiles history diff                                       # what changed by hand since the latest checkpoint
mise bootstrap dotfiles save --description "before the theme change"
```

Nothing leaves the machine: checkpoints live under `$MISE_STATE_DIR/history/`,
readable only by you.

## What a checkpoint records

Every checkpoint carries:

- **The snapshot**: the tracked files as they were, rooted at your home
  directory (`~/.zshrc`, `~/.config/mise/config.toml`). Symlinks are stored
  as links, nested git repositories as pointers, and files over 16 MiB,
  special files, and unreadable paths are listed as omitted with the reason.
- **The coverage**: which paths were tracked, under which policies, and which
  were excluded, so a later command can tell a file that was _absent_ from one
  that was never covered.
- **What changed** since the previous checkpoint, and a description computed
  from it (`edited hypr/bindings.lua; added omarchy/hooks/post-theme`).
  `mise bootstrap dotfiles history describe <ref> "…"` replaces the description.
- **The trigger**: `save`, `baseline` (a newly tracked path), or the two halves
  of an operation: `bootstrap-before` (the protective checkpoint taken before
  a bootstrap command changes anything) and `bootstrap` (the outcome, with a
  journal of every path the run touched and its prior state).

`mise bootstrap dotfiles history show <ref>` prints all of it; `--files` lists the snapshot,
`--json` gives the record.

## Referring to checkpoints

Commands take a numeric id, `latest`, `latest~N`, or a uuid prefix. With
`--path`, `latest~N` counts only the checkpoints where that path changed, so
`mise bootstrap dotfiles history show latest~1 --path ~/.zshrc` is the state before its most
recent change however many other checkpoints came in between.

Numeric ids are local handles: they can have gaps (a run that changed nothing
gives its ids back) and start over if the index is rebuilt from the
repository. Uuids are stable.

## Comparing

```sh
mise bootstrap dotfiles history diff                        # working tree against the latest checkpoint
mise bootstrap dotfiles history diff 12                     # what checkpoint 12 changed
mise bootstrap dotfiles history diff 11 12 --patch --path ~/.config/hypr
mise bootstrap dotfiles history diff --exit-code            # exit 1 when something differs
```

## Saving

`mise bootstrap dotfiles save` records a checkpoint now. It fails when nothing could be
saved — git missing, history disabled, a path that is not tracked — so a
script or an agent gets a trustworthy answer; `--best-effort` turns that into
a warning for `set -e` update scripts. Saving again without changes records
nothing, while a save with `--description`, `--label`, or `--task` always does.

A file tracked with `autosave = false` is a **manual-save** file: automatic
checkpoints carry its last saved version forward, and only
`mise bootstrap dotfiles save <path>` (or an operation that names it) promotes what is on
disk. `mise bootstrap dotfiles history diff --path <file>` shows saved against live. Promotions
are recorded in the repository (`refs/promoted`), never only in an index.

## Rolling back

```sh
mise bootstrap dotfiles rollback ~/.config/hypr/bindings.lua        # its most recent saved version that differs from disk
mise bootstrap dotfiles rollback ~/.zshrc --to 42                    # that checkpoint's version
mise bootstrap dotfiles rollback --to latest~3 --all --dry-run       # everything the checkpoint covers
mise bootstrap dotfiles undo                                         # reverse the newest rollback, undo, or apply
```

A rollback is planned first: for every selected path, `write` when the
checkpoint holds a different version, `delete` when the checkpoint knows the
path was absent, `unchanged`, `skip` when the checkpoint never covered or
omitted it, or `conflict` when the path changed type (a file became a
directory or a symlink) — conflicts need `--force`. `--dry-run` stops after
the plan.

Then the current state of the affected paths is saved in a protective
checkpoint (`rollback-before`); the plan is verified against the working tree
again (an editor may have written meanwhile) and every path about to change
must be captured in that checkpoint as it is now, or the rollback stops
without touching anything. Files are written one at a time, each journaled,
and only afterwards do `[history.reload]` commands run — once per matching
glob, resolved from the system and global configuration before the operation
began, so nothing a rollback writes can change which commands run:

```toml
[history.reload]
"~/.config/hypr/**" = "hyprctl reload"
```

A rollback is a new forward change: the outcome is a new checkpoint, the
version you left is still recoverable, and nothing is rewritten. Restoring a
mise configuration file never runs bootstrap; the outcome says when
declarations may differ from the applied setup.

`mise bootstrap dotfiles undo` restores exactly the paths an operation touched from the
protective checkpoint it took, leaving everything else as it is now, so
unrelated work done since is preserved. It refuses when that checkpoint was
pruned. Undoing an undo re-applies the operation.

## What is tracked

`mise bootstrap dotfiles paths` lists every entry with its mode, policies, the file that
declared it, and how many files it covers, followed by exclusions, derived
symlink targets, private files, and any declaration history could not honour.

- The global config directory and `dotfiles.root` are always tracked.
- `[dotfiles]` entries with `mode = "track"` are tracked where they are; every
  other mode enrolls the source it references (and, for `copy`, `template`, and
  `content`, the destination it produces, for local recovery only).
- A tracked symlink whose target lies inside your home directory tracks the
  target too, reported as `derived`.
- `[history] exclude` globs are never captured; patterns apply in order and
  the last match wins, so a later `!glob` re-includes what an earlier glob
  excluded (`["~/.config/app/**", "!~/.config/app/keep.conf"]`).

### Policies

Each entry carries three policies, set on the `[dotfiles]` entry or with
`mise bootstrap dotfiles track --no-autosave|--no-share|--no-backup`:

| Policy     | Meaning                                                               |
| ---------- | --------------------------------------------------------------------- |
| `autosave` | Save edits automatically (default `true`); `false` = manual-save      |
| `share`    | Publish the saved version to the shared setup (default `true`)        |
| `backup`   | Include the file in remote backups (default `true` for tracked files) |

Sharing and backups arrive with a later release; the policies are recorded in
every checkpoint from the start so nothing needs migrating.

`*.local.toml` files are private by default (`share = false`) wherever they
are found, and credential stores under the config directory
(`github_tokens.toml`, `age.txt`, `*.key`, …) are neither shared nor backed
up. A per-file `[dotfiles]` declaration is the only override, and
`mise bootstrap dotfiles paths` lists every private file so the choice stays visible.

## Retention

`history.keep.count` (500) caps the number of checkpoints and
`history.keep.age` (`90d`) their age. Pruning drops the oldest first,
automatic captures before explicit saves before operation pairs; a pair is
kept or dropped together, and pinned checkpoints survive regardless. `0`
disables a cap. Pruned content is freed from the repository.

## Requirements and settings

History needs a `git` binary (on macOS, the Xcode Command Line Tools). Without
one, `mise bootstrap dotfiles save` fails and bootstrap commands still run, recording
their journals without content; `mise bootstrap dotfiles status` says so.

| Setting              | Default | Env                       |
| -------------------- | ------- | ------------------------- |
| `history.enabled`    | `true`  | `MISE_HISTORY_ENABLED`    |
| `history.keep.count` | `500`   | `MISE_HISTORY_KEEP_COUNT` |
| `history.keep.age`   | `90d`   | `MISE_HISTORY_KEEP_AGE`   |

Deleting `$MISE_STATE_DIR/history/` discards everything recorded.
