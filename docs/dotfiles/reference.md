---
description: "Look up every [dotfiles] key, which keys each mode accepts, variant and pattern rules, status states, and the files history keeps."
socialDescription: "Look up every [dotfiles] key, pattern rule, status state, and state file."
---

# Dotfiles reference

Look up `[dotfiles]` entry keys, the keys each mode accepts, variant
selectors, pattern syntax, status states, and the state that mise keeps for
dotfiles and their history. The keys of `[dotfile_groups]` are in
[Groups](/dotfiles/groups.html#group-keys), and the `[history]` table and
history settings are in the
[history configuration reference](/dotfiles/history.html#configuration-reference).

## Entry keys {#entry-keys}

A whole-file entry is keyed by its target path, absolute or starting with
`~/`. A string value is shorthand for `{ source = "<value>" }`.

| Key               | Type           | Meaning                                                                                                                                                                                                                   |
| ----------------- | -------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `source`          | path           | The file or directory to deploy. Relative paths start at the declaring config file's directory. Defaults to the target's path under [`dotfiles.root`](/configuration/settings.html#dotfiles.root).                        |
| `mode`            | string         | `symlink`, `symlink-each`, `copy`, `template`, `absent`, `track`, or `track-local`. Defaults to [`dotfiles.default_mode`](/configuration/settings.html#dotfiles.default_mode). See [Modes](/dotfiles/managed.html#modes). |
| `content`         | string         | The whole file, written inline. See [Inline content](/dotfiles/managed.html#inline-content).                                                                                                                              |
| `permissions`     | octal string   | Permissions for the target, such as `"0600"`. On its own, manages an existing path's permissions only. See [Permissions](/dotfiles/managed.html#permissions).                                                             |
| `exclude`         | list of globs  | Paths to skip in a walked source directory or a tracked directory. See [Pattern syntax](#patterns).                                                                                                                       |
| `manifest`        | `"git"`        | Deploy only files in the source repository's Git index. See [Deploy only files committed to Git](/dotfiles/managed.html#git-tracked-directories).                                                                         |
| `dot_prefix`      | boolean        | Deploy source names `dot-<name>` as `.<name>`. See [Visible source names](/dotfiles/managed.html#dot-prefix).                                                                                                             |
| `relative`        | boolean        | `symlink` and `symlink-each`: link by a relative path. Overrides [`dotfiles.relative_symlinks`](/configuration/settings.html#dotfiles.relative_symlinks).                                                                 |
| `remove_empty`    | boolean        | Remove the target when the template renders empty. See [Remove the target when a template renders empty](/dotfiles/managed.html#remove-empty).                                                                            |
| `variants`        | list of tables | Per-machine selection: a target per machine for deployed entries, separate contents for tracked ones. See [Variant selectors](#variant-selectors).                                                                        |
| `enabled`         | boolean        | `false` turns off an entry inherited from another config file.                                                                                                                                                            |
| `include`         | list of globs  | Track only: the files a tracked directory saves. See [Select files in a directory](/dotfiles/history.html#choose-which-files-a-directory-saves).                                                                          |
| `autosave`        | boolean        | Track only: `false` saves the path only on `mise dot save <path>`. See [Save some files only on request](/dotfiles/history.html#save-on-request).                                                                         |
| `encrypt`         | boolean        | Track only: encrypt contents before saving them to Git. See [Encrypted files](/dotfiles/encryption.html).                                                                                                                 |
| `allow_plaintext` | boolean        | Track only: save a credential-named file that you track directly in plaintext. See [Credential filtering](/dotfiles/history.html#credential-filtering-and-omissions).                                                     |

A `[dotfiles]` entry takes no `group` key; declare a group's entries under
[`[dotfile_groups.<name>.entries]`](/dotfiles/groups.html#group-entries).

## Edit keys {#edit-keys}

An edit entry is keyed by the target path followed by `/<id>`, such as
`"~/.zshrc/aliases"`. Ids can contain letters, digits, `_`, `-`, and `.`. See
[Edit part of a file](/dotfiles/edits.html).

| Key        | Type                      | Meaning                                                                                                                                                                                                           |
| ---------- | ------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `block`    | string                    | Inline content to keep between marker comments.                                                                                                                                                                   |
| `source`   | path                      | A file with the block content (with `template = "tera"`) or the keys to merge (with `merge`).                                                                                                                     |
| `template` | `"tera"`                  | Render `source` with Tera first.                                                                                                                                                                                  |
| `comment`  | string                    | The marker comment prefix. Defaults to one inferred from the file extension.                                                                                                                                      |
| `line`     | string                    | One exact line to ensure exists.                                                                                                                                                                                  |
| `position` | `"append"` or `"prepend"` | Where a missing `line` goes. Defaults to `"append"`.                                                                                                                                                              |
| `merge`    | `true` or `"missing"`     | Set the keys in `source` on a `.json`, `.toml`, `.yaml`, or `.yml` target. `"missing"` sets only keys the target has no value for; see [Defaults the application may change](/dotfiles/edits.html#merge-missing). |

`block` and `source` are mutually exclusive, and neither combines with
`line`. `position` applies only to `line`. A `merge` entry without `source`
reads the target's path under
[`dotfiles.root`](/configuration/settings.html#dotfiles.root), and takes no
`block`, `line`, `position`, or `comment`. An edit entry takes no
`permissions`. Edits are never encrypted.

## Modes and keys {#modes-and-keys}

The table shows which whole-file keys each kind of entry accepts. An entry
with `content` is an inline-content entry, and one with only `permissions`
(no `source`, `content`, or `mode`) is a permissions-only entry. Every entry
also accepts `enabled`.

| Entry            | Accepts                                                                                                                                        | Accepted, no effect              |
| ---------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------- |
| `symlink`        | `source`, `relative`, `variants` (selectors and `target`)                                                                                      | `exclude`, `autosave`, `encrypt` |
| `symlink-each`   | `source` (a directory), `exclude`, `manifest`, `dot_prefix`, `relative`, `variants` (selectors and `target`)                                   | `autosave`, `encrypt`            |
| `copy`           | `source`, `variants` (selectors and `target`); with a directory source, `exclude`, `manifest`, `dot_prefix`; with a file source, `permissions` | `autosave`, `encrypt`            |
| `template`       | `source` (a file), `permissions`, `remove_empty`, `variants` (selectors and `target`)                                                          | `exclude`, `autosave`, `encrypt` |
| `content`        | `content`, `permissions`, `variants` (selectors only)                                                                                          | `autosave`                       |
| Permissions only | `permissions`, `variants` (selectors and `target`)                                                                                             | `autosave`                       |
| `absent`         | `variants` (selectors and `target`)                                                                                                            | `autosave`                       |
| `track`          | `exclude`, `include`, `autosave`, `encrypt`, `allow_plaintext`, `variants` (selectors only)                                                    |                                  |
| `track-local`    | `exclude`, `include`, `autosave`, `allow_plaintext`                                                                                            |                                  |

For `encrypt` and `relative`, the table means the `true` value; `encrypt =
false` and `relative = false` are accepted everywhere except that
permissions-only and `track-local` entries reject any `encrypt` key, including
`encrypt = false`. See [Local-only history](/dotfiles/history.html#local-only)
for `track-local`.

mise warns about and ignores an entry that sets one of the
[entry keys](#entry-keys) its kind does not accept; for a `track` entry,
`mise dot paths` also lists it as invalid. A key that is not an entry key at
all is ignored without a warning, except in configuration arriving from
another machine, which a pull refuses to apply. `permissions` with a
directory source, and `manifest` or `dot_prefix` with a file source, stop the
command with an error. An entry that sets both `permissions` and `manifest` is
ignored with a warning.

## How entries combine {#how-entries-combine}

- Entries merge across the [config hierarchy](/configuration.html#configuration-hierarchy).
  Whole-file entries merge by target path, and edit entries by target path
  and id. A more local config file replaces an entry with the same key,
  including its variants.
- If a more local declaration is invalid, or none of its variants matches this
  machine, the inherited entry stays active.
- A tracking entry and a deployed entry for the same path are separate
  entries; see [Track files that mise also deploys](/dotfiles/history.html#ownership).
- mise reads tracking entries, and the `[history]` table, only from system and
  global config.
- mise warns about and skips an entry with an unknown mode or edit operation,
  so a config written for a newer mise still loads.

## Variant selectors {#variant-selectors}

Each element of `variants` selects machines with these keys:

| Key       | Matches                                                                                                                                                                                       |
| --------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `os`      | An operating system (`linux`, `macos`, `windows`), the `unix` family, or either with an architecture after `/`, such as `linux/arm64` or `unix/x64`. A list matches any of its values.        |
| `profile` | A [config environment](/configuration/environments.html) selected with `-E` or `MISE_ENV`.                                                                                                    |
| `default` | `true` marks the fallback for machines that match no other variant. It takes no `os` or `profile`, and an entry has at most one.                                                              |
| `machine` | `true` gives each machine its own version; see [One version per machine](/dotfiles/history.html#machine-variants). It must be the entry's only variant and cannot be combined with `encrypt`. |

A variant with both `os` and `profile` matches only when both do. When several
variants match, mise scores each one and the highest score wins:

| Selector                      | Points |
| ----------------------------- | ------ |
| `profile`                     | 4      |
| `os` naming one system        | 2      |
| `os = "unix"`                 | 1      |
| An architecture after the `/` | 2 more |

On a Mac, `os = "macos"` therefore beats `os = "unix"`, and a profile-only
variant ties with an OS-and-architecture variant. A tie for the highest score
makes the entry invalid until you fix it. When nothing matches, mise uses the
`default` variant; without one, it skips the entry on that machine. A skipped
tracked path is neither saved nor applied there, and checkpoints keep the
versions other machines saved.

On a deployed entry, a variant can add `target` to choose the path; see
[Different targets on different machines](/dotfiles/managed.html#platform-specific-destinations).
On a tracked entry, each variant is a separate stream of contents at the same
path; see [Different contents on different machines](/dotfiles/history.html#variants).

## Pattern syntax {#patterns}

Patterns in a whole-file entry's `exclude`, a group's `exclude`, and a tracked
entry's `exclude` and `include` are relative to the entry's directory: the
source directory for deployed entries and groups, and the tracked directory
for tracked entries.

| Pattern             | Matches                                                                             |
| ------------------- | ----------------------------------------------------------------------------------- |
| `mise.toml`, `*.md` | A name without `/` matches any single path component, at any depth                  |
| `nvim/spell`        | A pattern with `/` is anchored to the entry's directory: only that path             |
| `/cache`            | A leading `/` anchors the pattern too, as in `.gitignore`: only a top-level `cache` |
| `nvim/**/*.bak`     | `**` matches across directories: `nvim/init.bak` and `nvim/after/init.bak`          |
| `sessions`          | A pattern that matches a directory covers everything beneath it                     |

`/nvim/spell` and `nvim/spell` are the same pattern. A leading `/` is never an
absolute path in these lists; the global list below is different.

In an `include` list, `*` never crosses `/`, as in `.gitignore`: `rules/*.md`
selects the Markdown files directly in `rules`, and `rules/**/*.md` also
selects those in its subdirectories.

::: warning Deprecated: `*` crossing `/` in exclusions
In an `exclude` pattern that contains `/`, `*` still matches across
directories, so `"nvim/*.bak"` also skips `nvim/after/init.bak`. mise warns
when a pattern skips a path only for that reason. Write `**` instead: `*` will
stop at `/` in exclusions, as it already does in `.gitignore`, in include
lists, and in patterns with a leading `/`.
:::

An invalid glob in a deployed entry's `exclude` is reported, and the rest of
the list still applies. An invalid glob in a tracked entry's `exclude` or
`include` makes the whole entry invalid, so nothing is saved by mistake.

### Global exclude patterns {#global-exclude-patterns}

The `[history] exclude` list, which [`mise dot exclude`](/cli/dotfiles/exclude.html)
writes, applies beneath every tracked entry:

| Pattern                | Matches                                                                 |
| ---------------------- | ----------------------------------------------------------------------- |
| `cache`                | Any file or directory named `cache` beneath a tracked entry             |
| `sessions/**`          | Contents of a `sessions` directory at any depth beneath a tracked entry |
| `~/.codex/sessions/**` | Contents of this specific directory                                     |
| `keys/*.pem`           | PEM files directly inside any `keys` directory beneath a tracked entry  |
| `keys/**/*.pem`        | PEM files at any depth inside those `keys` directories                  |

- A relative pattern matches at any depth beneath each tracked entry, never
  relative to your shell's working directory. A leading `./` is ignored.
- An absolute pattern can start with `~`, and it keeps matching through
  symlinked parent directories.
- `*` stops at a path separator and `**` crosses separators. On Windows, both
  `/` and `\` are separators.
- The last rule that matches a path, or one of its parents below the tracked
  root, decides whether it is excluded. A later `!glob` can select a file
  inside an excluded directory. mise skips an excluded directory without
  reading it unless a later rule could select something inside.
- Environment variables are not expanded; use `~/` or an absolute path.
  `mise dot exclude` rejects patterns that contain `$` and invalid globs. If
  such a pattern is already in your config, mise warns and ignores that rule,
  and the others still apply.

Checkpoints record which version of these rules they were saved with. When
rollback cannot interpret an older checkpoint's coverage reliably, it reports
the affected paths as skipped instead of deleting live files.

## Status states {#status-states}

[`mise dot status`](/cli/dotfiles/status.html) prints one row per entry, edit,
and orphaned group file, followed by the history state: what is tracked, the
latest checkpoint, unfinished operations, whether edits are saved
automatically, and the origin.

| State                                              | Meaning                                                                                      | What to do                                                            |
| -------------------------------------------------- | -------------------------------------------------------------------------------------------- | --------------------------------------------------------------------- |
| `applied`                                          | The target matches its entry.                                                                | Nothing.                                                              |
| `applied (target absent; permissions not applied)` | A permissions-only target does not exist.                                                    | Nothing, or create the path.                                          |
| `missing`                                          | The target or edit does not exist yet.                                                       | `mise dot apply`                                                      |
| `differs (<reason>)`                               | The target exists but its content, link, or permissions differ, or it cannot be checked.     | `mise dot diff`, then `mise dot apply`, with `--force` for a conflict |
| `source missing`                                   | The entry's source does not exist.                                                           | Create the source, or fix `source`.                                   |
| `tracked`                                          | A `mode = "track"` entry. `tracked (2 omitted, 1 nested)` counts files that saves leave out. | `mise dot paths` lists them.                                          |
| `absent`                                           | An `absent` entry whose target is gone.                                                      | Nothing.                                                              |
| `would remove (<reason>)`                          | An `absent` entry whose file or symlink is still there.                                      | `mise dot apply`                                                      |
| `orphaned`                                         | A file a group deployed that no active entry deploys any more.                               | `mise dot apply --prune` or `mise dot unapply --group <name>`         |
| `orphaned (changed since applied)`                 | An orphaned file you changed after mise wrote it.                                            | Remove it yourself, or pass `--force` to remove a changed copy.       |

`mise dot status --missing` prints the same rows and exits with status 1 when
any entry or edit is `missing`, `source missing`, `differs`, or
`would remove`. Orphaned files do not affect the exit status.

### JSON output {#json-output}

`mise dot status --json` prints an object with `files`, `edits`, and
`history`, plus `orphaned` when a group left files behind.

- Each `files` element has `target`, `source`, `mode`, `origin`, `state`, and
  the `omitted` and `nested` counts of a tracked entry. `mode` is one of the
  modes, `content`, or `permissions`. `state` is `applied`, `missing`,
  `differs`, `source_missing`, or `tracked`.
- `source` is a display path with `~` for your home directory, and `null` for
  inline content, permissions-only, and `absent` entries. Use `origin.source`
  for the absolute path.
- `origin` describes where the entry came from: the config file, its
  `config_root`, any config environment in the file name, and the resolved
  source path. These paths are strings when they are valid UTF-8; on Unix, a
  path with other bytes is written as `mise:path-bytes:<base64url>`.
- An entry that sets `permissions` includes them as an octal string.
- A `differs` entry has a `reason`. An `absent` entry is `applied` once the
  target is gone and `differs` while it is there, with a reason that ends in
  `; will be removed`. A permissions-only entry whose target does not exist is
  `applied` with a `reason`.
- Each `edits` element has `path`, `edit` (such as `block:aliases`), `origin`,
  and `state`.
- Each `orphaned` element has `target`, `group`, and `state`, which is
  `orphaned` or `orphaned_changed`.

## State files {#state-files}

mise keeps records under [`MISE_STATE_DIR`](/directories.html)
(`~/.local/state/mise` by default). Keep them: they are how mise recognizes
what it wrote.

| Path                                           | Holds                                                                                                                                   |
| ---------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| `$MISE_STATE_DIR/dotfiles/`                    | The links each `symlink-each` entry created                                                                                             |
| `$MISE_STATE_DIR/dotfiles/targets/`            | For each target, a digest of what mise last wrote there and the parent directories it created                                           |
| `$MISE_STATE_DIR/dotfiles/groups/`             | What each [group](/dotfiles/groups.html) deployed, so its files can be found after the group stops deploying them                       |
| `$MISE_STATE_DIR/history/repo.git`             | The history: a bare Git repository of checkpoints                                                                                       |
| `$MISE_STATE_DIR/history/`                     | The checkpoint index, which mise can rebuild from Git, operation journals and recovery copies, `health.json`, and `watch-schedule.json` |
| `~/.config/mise/config.local.toml`             | `[history.origin]`, written by `mise dot origin set`                                                                                    |
| `~/.config/mise/conf.d/dotfiles-tracking.toml` | Tracking entries for files that another `[dotfiles]` entry already manages                                                              |

mise records a digest for every template target, so turning on
`remove_empty` later still allows a safe removal. An apply that finds a target
already identical to the rendered output records it too: that file counts as
written by mise, and any later edit to it makes a removal a conflict. Without
these records, mise cannot remove links or directories it created earlier,
cannot tell its own template output from your edits, and cannot find orphaned
group files.

## Checkpoint contents {#what-a-checkpoint-records}

Each checkpoint is a Git commit in `$MISE_STATE_DIR/history/repo.git`,
separate from the files you edit. It holds the tracked files as ordinary Git
tree entries, plus metadata for their paths, tracking settings, variants,
encryption, and permissions.

Permission metadata covers non-default modes: files that are not `0644` or
`0755`, tracked directories that are not `0755`, and directories between a
tracked path and your home or mise config directory that are not `0755`.
Tracking `~/.claude/settings.json` inside a `0700` `~/.claude` records that
mode, so a fresh machine recreates the directory as private rather than
world-readable. Your home and mise config directories themselves are never
recorded.

A symlink is saved as a link. mise reports oversized files, special files,
and unreadable paths that it cannot save, and keeps their previous saved
versions while it saves other files, so check reported omissions before you
rely on a checkpoint. Exclusions remove paths from future checkpoints. An
encryption failure stops a save rather than storing plaintext.

Checkpoints restore file contents only. They do not restore installed
packages or the running state of a service; use your system's backup tools
for that, and run `mise bootstrap` to apply restored configuration.

### Operation checkpoints {#operation-checkpoints}

Commands that change or capture tracked files save checkpoints before and
after their work, such as `mise bootstrap` and its part subcommands,
`mise dot apply`, `add`, `unapply`, `edit`, `rollback`, `undo`, `pull`, and
`capture`. Their metadata includes the operation's label and the link between
the two checkpoints, which `mise dot undo` and
`mise dot history diff --operation` use. Raw command arguments, environment
contents, and temporary recovery copies stay out of committed metadata.

When an operation changes a tracked file with `autosave = false`, it saves
that file's actual contents first; unrelated manual edits stay unsaved. If the
operation writes a file several times, the before checkpoint still holds the
contents from before the first write. Encrypted files are encrypted in these
checkpoints too.

`undo` uses an operation's before checkpoint to restore the tracked paths it
changed. Untracked files that an operation writes get temporary recovery
copies only for finishing or recovering interrupted writes, and those copies
are deleted afterwards, so untracked files have no undo.

## Recovery {#recovery-details}

Before rollback changes any file, mise saves the current contents in a
`rollback-before` checkpoint. If it cannot save every file it would change, it
stops before writing. It checks the plan again after that checkpoint, and
checks each path again immediately before replacing it; a concurrent edit that
invalidates the plan stops the operation.

Files are written one at a time, with a journal that records each affected
path, so an interruption can leave some writes done. Run
[`mise dot recover`](/cli/dotfiles/recover.html) to retry the unfinished
writes. A retry never overwrites later edits; when it cannot continue, it
keeps those edits and the recovery copies and lists the paths to inspect. To
accept the current files instead:

```sh
mise dot recover <operation> --keep-current
```

After you confirm, this discards that operation's temporary recovery copies
and keeps your current files and committed history. Unresolved recovery data
is kept until you resolve it.

Ordinary bootstrap writes warn and continue when they cannot save temporary
recovery contents, for example for an oversized file, and such a write cannot
be recovered automatically after an interruption. Pull, rollback, and undo
refuse to write without the recovery data they need. Turning history off lets
ordinary bootstrap run without the history store, but explicit history
commands still need their recovery data.

## History lock {#history-lock}

Operations that record history take a lock on the history store in
`MISE_STATE_DIR`. A run that records history waits up to 30 seconds for the
lock, then fails.

A `mise bootstrap` with nothing tracked and no existing history touches the
history store only when it first rewrites a dotfile or edit, to keep a private
journal for recovering interrupted writes. If another mise process holds the
lock at that point, it warns and continues without the journal instead of
failing. A run that starts with nothing to record but declares tracking before
it ends waits the same 30 seconds at the end. If the lock is still busy, or an
earlier write already ran without the journal, it warns that its outcome was
not recorded and keeps its result.

## Watcher {#watcher-reference}

The [watcher](/dotfiles/history.html#automatic-saves) watches tracked
directories recursively. For a tracked file it watches the parent directory,
and for a missing path the nearest existing parent. It never watches files
with `autosave = false`. Only one watcher runs per history store.

### Adaptive scheduling {#adaptive-scheduling}

mise schedules each tracked file on its own:

1. An ordinary edit is saved after
   [`history.watch.debounce`](/configuration/settings.html#history.watch.debounce)
   of quiet time.
2. If a file keeps changing without settling between saves, its save interval
   doubles, up to [`history.watch.max_interval`](/configuration/settings.html#history.watch.max_interval).
   The interval doubles when at least two changes arrived since the previous
   save and the file changed again within its settling period, so ordinary
   editor saves spaced further apart keep the usual interval.
3. When the file stops changing, mise saves its final contents after a
   fraction of that interval, at most five minutes.
4. After a quiet period of four intervals, and at least five minutes, the file
   returns to the base interval.

The longer interval affects only that file. Other files save normally, and
explicit saves and the checkpoints before bootstrap, rollback, or undo still
run immediately. A busy file is still saved periodically; mise never excludes
it or switches it to manual saving. When another file triggers a checkpoint,
or mise scans the full tracked set, a busy file keeps its last saved contents
until its own save is due.

The watcher stores these schedules, with each busy file's last save and
pending edits, in `watch-schedule.json`, so they survive a restart. At
startup, a busy file keeps its saved version until its next save is due, even
if it changed while the watcher was stopped. Changes to the `history.watch`
settings in global config take effect while the watcher runs.

When a full scan finds a new tracking entry, it saves the first version even
with `autosave = false`. Later scans carry that version forward until you save
the path explicitly.

### Scans and failures {#reconciliation-and-failures}

The watcher also scans every tracked file to catch changes that filesystem
notifications missed: at startup, at shutdown, when configuration changes, and
every [`history.watch.reconcile`](/configuration/settings.html#history.watch.reconcile).
Set that setting to `0` to turn off the periodic scan. Edits to your global
config, including `conf.d/`, reload the tracked paths and update the watches,
and setting `history.enabled = false` stops the watcher.

A failed save stays pending and is retried with delays that grow from one
second to five minutes. A save that overlaps bootstrap, rollback, or undo
waits and retries after that operation finishes. At shutdown, the watcher
waits briefly for a running operation and reports anything it could not save.

Every minute, the watcher checks that the mise executable it started from has
not been replaced. After an upgrade, it saves what is pending and exits with a
failure status, so the service manager restarts it on the new version. A
watcher you started by hand with `mise dot watch` stops, and you start it
again. If the executable is gone with nothing in its place, the watcher keeps
running and `mise dot status` and `mise doctor` report it as outdated; run
`mise bootstrap services apply` to restart it. The report clears if the
executable comes back.

### `mise dot watch` output and exit codes {#watch-output}

[`mise dot watch --json`](/cli/dotfiles/watch.html) prints one JSON object per
line. The events are `started`, `already-running`, `captured`, `unchanged`,
`deferred`, `replan`, `throttled`, `settled`, `degraded`, `sync`, `synced`,
`sync-error`, `applied`, `described`, `describe-error`, `describe-skipped`,
`outdated`, `restarting`, `unsaved`, `unavailable`, `disabled`, `error`, and
`stopped`.

`mise dot watch` exits 0 when history is disabled or another watcher already
runs for the store, and 1 when Git is unusable, the store cannot be opened, or
no watch can be installed. `mise dot watch --once` runs one scan and one
sync, and exits 1 if its save was deferred or failed, or if its sync with the
origin failed.

## Retention {#retention}

mise keeps all reachable Git history indefinitely; checkpoints never expire
and are never compacted. Untracking a path does not erase its old commits, and
encrypting its latest version does not erase earlier plaintext; see
[Remove plaintext from history](/dotfiles/encryption.html#remove-plaintext-from-history).
