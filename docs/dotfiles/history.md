---
description: "Save versions of your tracked dotfiles in local Git history, compare them, roll back a change, and choose what gets saved."
socialDescription: "Save, compare, and roll back versions of your tracked dotfiles in local Git history."
---

# Dotfiles history

mise saves versions of the files you track in a local Git repository, so you
can compare them, restore an earlier version, and undo a change. History stays
on your machine until you [sync it across machines](/dotfiles/sync.html).

To track your first file, see [Track a file in place](/dotfiles.html#tracking-files-in-place),
or follow [Set up a machine](/bootstrap/setup.html) for the whole flow on two
machines.

## Requirements {#requirements-and-settings}

History needs `git` on `PATH` (on macOS, the Xcode Command Line Tools). Without
it, [`mise dot save`](/cli/dotfiles/save.html) fails, bootstrap commands still
run and record their operations without file contents, and
[`mise dot status`](/cli/dotfiles/status.html) says why.

History is on by default
([`history.enabled`](/configuration/settings.html#history.enabled)) but records
nothing until something is tracked: by [`mise dot track`](/cli/dotfiles/track.html), by a `[dotfiles]`
entry with `mode = "track"` in your global or system config (however that file
got there), or by an adopted [setup repository](/dotfiles/sync.html). Setting
`history.enabled` to `false` stops automatic and explicit saves. You can still
browse history and edit checkpoint descriptions, and existing history is kept.

Parallel jobs that share one `MISE_STATE_DIR`, such as CI jobs, can wait on
each other's [history lock](/dotfiles/reference.html#history-lock). Give each
job its own `MISE_STATE_DIR`, or set `MISE_HISTORY_ENABLED=false`, if they
should never interact.

## Save a checkpoint {#saving}

A checkpoint is a saved version of your tracked files, stored as a Git commit.
To save the current contents of one file:

```sh
mise dot save ~/.zshrc
```

To save all tracked files with a description:

```sh
mise dot save --description "before changing my theme"
```

A save that finds no changes creates no commit. With `--description`,
`--label`, or `--task`, `mise dot save` creates a checkpoint even when the
files have not changed.

`save` fails if it cannot save anything, for example because Git is missing,
history is disabled, or the requested path is not tracked. `--best-effort`
turns a failure to write the checkpoint, such as missing Git, into a warning for
scripts that should continue; a disabled history or an untracked path still
fails.

Commands that change tracked files, such as `mise bootstrap`, `mise dot apply`,
and `mise dot rollback`, save their own checkpoints before and after their
work; see [operation checkpoints](/dotfiles/reference.html#operation-checkpoints).

## Save automatically {#automatic-saves}

The watcher is a background service that saves changes to tracked files,
including edits made by your editor, an application, or another command. Add
it to your global config:

```toml
[bootstrap.services.mise-history]
builtin = "history-watch"
```

Install it and check that it is running:

```sh
mise bootstrap services apply
mise dot status
```

mise installs the watcher as a user service, without root: a systemd user unit
on Linux, a LaunchAgent on macOS, or a Scheduled Task on Windows. A full
`mise bootstrap` does the same. See [user services](/bootstrap/services.html#user-services)
if it fails to start. Without a service, `mise dot watch` runs the watcher in
the foreground, and `mise dot watch --once` runs one scan, for example from a
timer or cron job.

The watcher saves an ordinary edit once the file has been quiet for
[`history.watch.debounce`](/configuration/settings.html#history.watch.debounce).
A file that keeps changing is saved less often, so it can have unsaved edits
while other files are already saved; `mise dot paths --noisy` lists those
files. Explicit `save` commands still save immediately. The
[watcher reference](/dotfiles/reference.html#adaptive-scheduling) describes the
schedule.

### Save some files only on request {#save-on-request}

Logs, caches, databases, and session state usually belong outside history;
[exclude them](#exclude-files-from-one-directory). For a configuration file you
want to save only when you ask, turn off automatic saves when you track it:

```sh
mise dot track ~/.config/app/state.json --no-autosave
mise dot save ~/.config/app/state.json
```

This writes `autosave = false` on the entry. Tracking saves the first version
immediately, and later edits wait for `mise dot save <path>`. The watcher's
checkpoints keep the file's last saved version. Commands that change or
capture the file, such as `mise bootstrap` and `mise dot capture`, still save
it before they run.

### Check watcher health {#health}

If files are not being saved or shared, start with:

```sh
mise doctor
mise dot status
```

[`mise doctor`](/cli/doctor.html) reports a watcher that is declared but
stopped, repeated save failures, an unusable history store, and files saved
less often because they change constantly. It includes the command to start a
stopped watcher.

`mise dot status` gives more detail: whether the watcher is running, declared
but stopped, or not declared, and whether its saves are failing; the latest
save and full scan; the last failure; and each busy file's save interval, last
save, and pending edits.

Both commands also report a watcher service that runs an outdated mise, or
that watches another store because it uses a different `MISE_STATE_DIR`. Run
`mise bootstrap services apply` to restart it.

These commands read the watcher's saved health report without starting a sync
or changing files. Old reports are marked stale after a few full-scan
intervals. A busy file's longer save interval is informational. For desktop
notifications about sync conflicts, see
[Conflict notifications](/dotfiles/sync.html#conflict-notifications).

### Describe automatic checkpoints {#descriptions-from-an-agent}

[`history.describe_command`](/configuration/settings.html#history.describe_command)
runs a command, such as a coding agent, to describe each checkpoint the
watcher saves. Set it in your global or system config; mise ignores it in
project config. For example, with Claude Code installed:

```toml
[settings.history]
describe_command = "claude -p --output-format text --no-session-persistence 'Describe this change to my configuration files in one line of at most 120 characters, plain text, no quotes.'"
```

This sends change details, including the diffs of unencrypted files, to the
command you configure. Excluded files are not named, and the contents of
encrypted files are not sent.

The command receives one JSON object on stdin with the fields `uuid`,
`trigger`, the computed `description`, the `added`, `modified`, and `removed`
paths, and a unified `diff` of changed unencrypted files. The diff is limited
to 64 KiB, and `diff_truncated` reports when it was cut. Contents reach the
command through stdin, never through shell interpolation.

The command prints one line of at most 200 characters, which becomes the
checkpoint's description, recorded with `description_source: command`. The
checkpoint is saved before the command runs. If the command fails, prints
nothing, or runs longer than 30 seconds, the checkpoint keeps its computed
description. Commands run one at a time, once per checkpoint the watcher
saves; filesystem events and retries do not trigger extra runs.

To describe any checkpoint by hand, run
[`mise dot history describe <ref> <text>`](/cli/dotfiles/history/describe.html).

## Compare versions {#comparing}

List the checkpoints where a file changed, newest first:

```sh
mise dot history --path ~/.zshrc
```

```text
2  2026-10-07 18:12  save      edited ~/.zshrc   1
1  2026-10-07 18:12  baseline  tracked ~/.zshrc  1
```

Each line shows the checkpoint ID, when it was saved, what triggered it, its
description, and how many files it changed. Without `--path`,
[`mise dot history`](/cli/dotfiles/history.html) lists recent checkpoints for
all files: 20 by default, and every one with `--limit 0`. `--trigger` and
`--label` filter the list.

Use an ID from the list to inspect one checkpoint. The examples below use
checkpoint 12:

```sh
mise dot history show 12
mise dot history diff 12 --patch --path ~/.zshrc
```

`history show` displays what triggered the checkpoint and which files changed.
Add `--files` to list every file in it, or `--json` for structured output.
`history diff 12` shows what changed in that checkpoint. To compare two
checkpoints, give both IDs:

```sh
mise dot history diff 11 12 --patch --path ~/.zshrc
```

To compare your current file with its latest saved version:

```sh
mise dot history diff --path ~/.zshrc
```

This also shows unsaved edits in files with `autosave = false`. Add
`--exit-code` to make a difference return exit status 1, or omit `--path` to
compare all tracked files.

## Refer to a checkpoint {#referring-to-checkpoints}

Use a checkpoint ID from `history`, or one of these references:

| Reference      | Meaning                                                            |
| -------------- | ------------------------------------------------------------------ |
| `12`           | Checkpoint with local ID 12                                        |
| `latest`       | Most recent checkpoint                                             |
| `latest~1`     | Checkpoint before the most recent one; use `~N` to go farther back |
| `commit:<sha>` | Git commit identified by a full hash or an unambiguous prefix      |

With `--path`, `latest~N` counts only checkpoints where that path changed:

```sh
mise dot history show latest~1 --path ~/.zshrc
```

This selects the checkpoint before the file's most recent change, even if
other files have changed since then.

Numeric IDs are local, and they can change if mise rebuilds its index. Use a
Git commit hash when you need a stable reference. A plain number always
selects a checkpoint ID; a commit prefix that is not all digits also works
without `commit:`.

## Roll back {#rolling-back}

To restore a file to its most recent saved version that differs from its
current contents, preview the change and then apply it:

```sh
mise dot rollback ~/.zshrc --dry-run
mise dot rollback ~/.zshrc
```

mise saves the current contents before replacing them. To reverse that
rollback, run:

```sh
mise dot undo
```

[`mise dot undo`](/cli/dotfiles/undo.html) restores the tracked files that the
newest operation changed, and other files keep their current contents. Both
rollback and undo create new commits, so earlier versions stay available and
the restored version can sync to other machines.

To choose a checkpoint, use an ID from `history` or a
[checkpoint reference](#referring-to-checkpoints):

```sh
mise dot rollback ~/.zshrc --to 42
mise dot rollback --to latest~3 --all --dry-run
```

`--all` selects everything the chosen checkpoint covers. If that checkpoint
recorded a file as absent, rollback deletes the file. This can also happen
without `--to`: an earlier saved state where the file was absent counts as a
version to restore.

The preview reports each path as:

| Action            | Meaning                                                                                       |
| ----------------- | --------------------------------------------------------------------------------------------- |
| `write`           | Replace the file with different saved contents                                                |
| `delete`          | Remove a file recorded as absent                                                              |
| `remove if empty` | Remove a directory this rollback emptied, if nothing else remains in it                       |
| `unchanged`       | Keep a file that already matches                                                              |
| `skip`            | Leave a path the checkpoint did not cover or omitted                                          |
| `conflict`        | Resolve a changed path type, such as a file becoming a directory; replacement needs `--force` |

Git does not record empty directories, so rolling back a directory leaves
empty folders it has no record of alone. Restoring configuration files leaves
installed packages and running services as they are; run `mise bootstrap` to
apply the restored configuration.

Rollback, undo, and pull write files one at a time and journal each one. If
one is interrupted, run [`mise dot recover`](/cli/dotfiles/recover.html) to
finish it; see [Recovery](/dotfiles/reference.html#recovery-details).

### Reload an application after restoring files {#reload-an-application-after-restoring-files}

Add commands to your global config to reload an application after its files
change:

```toml
[history.reload]
"~/.config/hypr/**" = "hyprctl reload"
```

Each matching command runs once, after rollback, undo, or
[pull](/dotfiles/sync.html#sync-immediately) has written the files. mise reads
these commands from your system and global config before the operation
starts.

The same commands run when `mise dot apply` or the dotfiles step of
`mise bootstrap` writes a matching path: a link or copy it creates, a template
it renders, or an edit it applies. Nothing reloads on a dry run, on a
bootstrap that skips dotfiles, or on `mise dot sync`, which never writes live
files. A glob also matches paths inside a symlinked directory:
`"~/.config/hypr/**"` fires for a `"~/.config/hypr" = "dotfiles/hypr"` entry.

Reload commands react to file changes; they are not a place for machine setup.
Because mise reads them before the restore, commands that arrive in the same
update, including the first `mise bootstrap --adopt`, do not run for it. Put
setup steps such as installing shell plugins or fixing permissions in the
[`bootstrap` task](/bootstrap.html#the-bootstrap-task), which runs on adoption
and on each `mise bootstrap` after a shared update.

## Capture an external command {#capturing-an-external-command}

To see what an update did to your tracked files, wrap it with
[`mise dot capture`](/cli/dotfiles/capture.html). For example, on Omarchy:

```sh
mise dot capture --label "omarchy update" -- omarchy-update
mise dot history --label "omarchy update"
mise dot history diff --operation --patch
```

`capture` saves your tracked files before and after the command, including
files with `autosave = false`. It records the label and whether the command
succeeded. The diff compares those two checkpoints, even if other saves
happened later. To inspect an older operation, give its checkpoint ID:

```sh
mise dot history diff 42 --operation --patch
```

The command runs directly, with inherited input, output, and environment. Use
`-- sh -c '...'` when you need shell syntax. If capturing fails, mise warns and
the command still runs with its own exit status. Failed commands, and commands
that change nothing, still keep their checkpoint pair.

Other history writers wait until the pair is complete. Editors and other
programs can still change files during that time, and their edits appear in
the comparison too. `undo` restores tracked-file changes across the whole
interval. To restore only some files, find the operation's Before checkpoint
with `history show`, then run `mise dot rollback <path> --to <before-ref>`.
Packages, service state, and untracked files are outside this recovery.

If the wrapper is killed, the next command that changes history recovers its
pending operation record. External writes are not journaled one by one. If the
wrapped command untracks a file, the after checkpoint leaves it out; its
earlier checkpoint stays available, and recovery keeps it untracked.

## Stop tracking a file {#stop-tracking-a-file}

```sh
mise dot untrack ~/.zshrc
```

[`mise dot untrack`](/cli/dotfiles/untrack.html) removes the tracking entry.
The file stays in place. Future checkpoints leave it out, but its earlier
versions remain in Git and can still be shared. To keep a file's history
without ever sharing it, track it with
[`mode = "track-local"`](/dotfiles/history.html#local-only) instead. For an entry inherited from another config file, such as
system config, `untrack` sets `enabled = false` in your `config.local.toml`
instead. You can turn off any inherited entry the same way, with
`enabled = false` in a later config file.

To stop saving part of a tracked directory, such as its logs or caches,
[exclude it](#exclude-files-from-one-directory) instead.

## Choose what history saves {#explicit-tracking-and-exclusions}

Declare each file or directory you want to save in your global config:

```toml
[dotfiles]
"~/.zshrc" = { mode = "track" }
"~/templates" = { mode = "track" }
"~/.gitconfig" = { mode = "template", source = "~/templates/gitconfig.tera" }
```

History saves `~/.zshrc` and the files in `~/templates`. The template creates
`~/.gitconfig`; track that output separately if you want its history too.
mise reads tracking entries only from system and global config. It warns about
and ignores `mode = "track"` in a project config.

Track exact paths under your home directory or your mise config directory.
Tracking entries name paths, not globs. A tracked directory includes files
added beneath it later, subject to the rules below. To share tools, services,
and templates, track your mise config and the sources it uses as well;
bootstrap reports required files that are missing from history.

A symlink is saved as a link. Track its target separately to save the target's
contents. If a parent directory is a symlink, track that link, and use the
real directory path to track files beneath it. Your home directory and mise
config directory can themselves be symlinks; mise maps them to the matching
directories on each machine.

A tracking entry cannot contain `source`, `content`, `manifest`,
`permissions`, `remove_empty`, `relative`, `dot_prefix`, or variant `target`
overrides; see the [modes and keys table](/dotfiles/reference.html#modes-and-keys).
`mise dot paths` reports such an entry as invalid and leaves it out of
history, and `mise dot track` exits non-zero if the entry it writes is not
active.

Start with individual configuration files, so that you choose what to save.
Leave logs, caches, databases, and application session state out of history.
Files ending in `.local.toml` are machine-local config and are never saved,
even with `encrypt = true`.

### Preview before tracking {#preview-before-tracking}

Tracking a directory saves everything under it, so preview a large one first:

```sh
mise dot track --dry-run ~/.codex
# ~/.codex: 22,972 files, 1.2 GiB
```

`mise dot paths --preview ~/.codex` shows the same count and size and also
lists the files. Both commands show what would be left out, including
credential files and nested repositories, and neither writes anything. They
read file metadata, not contents, and skip unreachable subdirectories, so a
preview can show a selected-file count without a total for the whole
directory.

`mise dot track` prints the same count and size before it saves, and warns
when a tree is larger than 5,000 files or 256 MiB. The warning does not stop
tracking, and a scan that reaches its limit is reported as incomplete.
Trim the tree with an exclusion before you track it, for example
`mise dot exclude '~/.codex/sessions/**'`.

### Select files in a directory {#choose-which-files-a-directory-saves}

Use `include` when a directory holds a few configuration files among many
caches, logs, or session files:

```toml
[dotfiles]
"~/.codex" = { mode = "track", include = ["config.toml", "rules/**"] }
```

This saves `config.toml` and the files under `rules/`. New files elsewhere in
`~/.codex` stay out of history without another exclusion.

| Configuration              | Selection                                                    |
| -------------------------- | ------------------------------------------------------------ |
| No `include` field         | The directory's files, with built-in credential filtering    |
| `include = []`             | No files                                                     |
| A populated `include` list | Files matching any pattern, including credential-named files |
| An explicit exclusion      | Removes matching files from any of the above selections      |

Include patterns are relative to the tracked directory and follow the
[pattern syntax](/dotfiles/reference.html#patterns); in an include list, `*`
never crosses `/`. Literal names and globs have the same authority:
`include = ["**"]` also selects credential-named files. Encryption is a
separate choice; `encrypt = true` encrypts the selected files.

::: warning Credential files selected by name are saved in plaintext
Without `encrypt = true`, credential-named files that an include list selects
are saved in plaintext and can be shared with your origin. Previews and path
listings label them `plaintext:`, and saves produce
[capture warnings](#capture-warnings). Adding encryption later does not remove
plaintext from earlier commits.
:::

For example, to save a shell function whose name triggers the credential
filter:

```toml
[dotfiles]
"~/.config/fish" = { mode = "track", include = ["functions/secrets.fish"] }
```

That entry selects only this file. To save real credentials, encrypt them:

```toml
[dotfiles]
"~/.aws" = { mode = "track", include = ["credentials"], encrypt = true }
```

Configure [encryption recipients](/dotfiles/encryption.html#choose-recipients)
first. Changing `include` does not turn off encryption or rewrite existing
history. If you remove encryption and save plaintext at a path that was
encrypted before, the [publication check](/dotfiles/encryption.html#allow-plaintext-history)
still covers that path throughout the history.

Include lists apply only to tracked directories. For a single file, track it
directly, or track its parent and select the file by relative path. Invalid
include or exclude globs make a tracked entry invalid; fix the entry before
you track it again. Include lists are shared with your other machines and
recorded in checkpoints, so rollback knows which files were outside them; see
[mixed mise versions](/dotfiles/sync.html#mixed-mise-versions).

::: tip `mise dot include` edits a different list
`mise dot include <glob>` removes a rule from the global `[history] exclude`
list. It does not change a directory's `include` field; edit the `[dotfiles]`
entry for that.
:::

### Exclude files from one directory {#exclude-files-from-one-directory}

Put an `exclude` list on a tracked entry to leave out files beneath that path
without affecting other entries:

```toml
[dotfiles]
"~/.codex" = { mode = "track", exclude = ["sessions", "*.log"] }
```

Patterns are relative to the tracked directory and follow the
[pattern syntax](/dotfiles/reference.html#patterns). `sessions` excludes every
directory with that name, at any depth, with its contents; `*.log` excludes
matching files at any depth; and `/cache` excludes only `~/.codex/cache`.

The entry's list and the global `[history] exclude` list both apply, and a
global `!glob` cannot bring back a file the entry excludes. `mise dot paths`
and `mise dot track --dry-run` show the lists in effect.

### Exclude files across tracked entries {#exclude-files-across-tracked-entries}

[`mise dot exclude`](/cli/dotfiles/exclude.html) adds a glob to
`[history] exclude` in your global config. Quote it so that your shell does
not expand it:

```sh
mise dot exclude '~/.codex/sessions/**'
mise dot paths
```

An absolute pattern applies only to that location, so this example leaves
`~/.config/kitty/sessions/` alone. A relative pattern, such as `cache`, matches
beneath every tracked entry. To remove the rule, pass the same glob to
[`mise dot include`](/cli/dotfiles/include.html):

```sh
mise dot include '~/.codex/sessions/**'
```

A later `!glob` in `[history] exclude` brings back a path that an earlier rule
excluded. Removing one rule does not override other rules that still exclude
the path. The [pattern syntax](/dotfiles/reference.html#global-exclude-patterns)
reference covers matching order, absolute patterns, and invalid rules. After
you fix a rule that mise warned about, check the result with
`mise dot paths`.

### Credential filtering {#credential-filtering-and-omissions}

Without encryption, built-in filename rules leave out `.netrc`, `*.age`,
`*.key`, `*.pem`, `*.gpg`, `*.kdbx`, `id_*`, `*token*`, `*secret*`,
`credentials*`, and `oauth*`. Under your mise config directory,
`github_tokens.toml`, `hosts.yml`, and `age.txt` are left out too.

The rules look at the file's own name, not its contents or the names of its
parent directories. Both `id_ed25519` and `id_ed25519.pub` match `id_*`, and a
shell function named `secrets.fish` matches `*secret*`. Names ending in
`.example`, `.sample`, or `.template`, such as `secrets.sh.example`, are
templates and are not left out. `mise dot save` and
`mise dot track` report what they leave out, `mise dot status` shows omission
counts, and `mise dot paths` lists each path and its reason. To save a
credential, [encrypt it](/dotfiles/encryption.html).

When you track a file directly and its name looks like a credential store,
`mise dot track` asks whether to save it in plaintext. The default answer is
no, and `--yes` does not approve it. For a noninteractive command, pass
`--allow-plaintext`, but only after you check that the file is safe to put in
Git history and on any connected origin:

```sh
mise dot track --allow-plaintext ~/commit-mossy-token.md
```

The choice is saved as `allow_plaintext = true` on that file's entry, so later
saves and your other machines use the same policy without asking or warning
again. `mise dot paths` still lists the file as plaintext. Use
`mise dot track --encrypt` for a real credential. `--allow-plaintext` can
approve a file before it exists; if that path later becomes a directory, the
directory stays tracked and the usual filter applies to its contents.

### Capture warnings {#capture-warnings}

Saves warn when an include list selects a credential-named file for plaintext
storage. They also report when a changed include list leaves out files that an
earlier checkpoint contained: those paths stop appearing in new checkpoints,
and their earlier versions stay in Git history.

An include pattern also selects matching files created later. With
`include = ["**"]` on `~/.config/fish`, a `functions/secrets.fish` added later
is saved in plaintext on the next capture, with a warning. Warnings do not
block saving or sharing, so a background save can share the file with your
origin before you see the warning. To encrypt everything selected beneath the
directory, set `encrypt = true` on the entry before you save private files
there, and see [remove plaintext from history](/dotfiles/encryption.html#remove-plaintext-from-history)
for versions already saved.

Where a warning appears depends on what saved the file:

| Capture                    | Where to read the warning                                              |
| -------------------------- | ---------------------------------------------------------------------- |
| Explicit save or tracking  | In that command's output                                               |
| Bootstrap                  | During the bootstrap command                                           |
| Watcher or automatic apply | At the next `mise dot` command other than `watch`, or `mise bootstrap` |

For example, after the watcher applies a narrower include list, `mise dot paths`
prints the pending notice, naming the earlier checkpoint that holds the
omitted paths, before it lists the current selection. A command shows each
warning once, even when it fails. A warning from a background save, or from
the protective checkpoint of an operation that then failed, waits for the next
foreground command.

### Nested repositories {#nested-repositories}

A directory with its own `.git` inside a tracked directory is a separate
repository. mise skips its contents and reports its path when it tracks,
saves, shows status, and lists paths. History records no files and no commit
pointer for it.

To save a nested repository's working files, track its root as its own entry:

```sh
mise dot track ~/.hammerspoon/Spoons/SkyRocket.spoon
```

The repository's files then follow that entry's settings, and `.git` is always
left out. An include pattern on the parent entry does not reach into a nested
repository. You can also leave the repository to the tool that installs it, or
remove its `.git` to treat the directory as ordinary files.

Checkpoints record skipped repositories as omissions, and rollback leaves
files under those paths alone even if `.git` has since been removed. When a
history from an older mise contains commit pointers, pull and adoption skip
them instead of restoring unavailable Git objects or removing an existing
checkout.

### Different contents on different machines {#variants}

A variant lets a tracked file have different contents on different machines,
at the same path on each one. For example, to keep separate versions of
`~/.zshrc` for macOS and Linux:

```toml
[dotfiles]
"~/.zshrc" = { mode = "track", variants = [{ os = "macos" }, { os = "linux" }] }
```

Add this to your global config, or run `mise dot track ~/.zshrc --os macos` to
add one variant. `os` takes an optional architecture after a `/`, so
`os = "macos/arm64"` matches only Apple silicon Macs (platform config file
names write the same thing as `macos-arm64`). `os = "unix"` shares one version
between Linux and macOS and skips the path on Windows:

```toml
[dotfiles]
"~/.zshrc" = { mode = "track", variants = [{ os = "unix" }] }
```

Use `profile` to select a [config environment](/configuration/environments.html):

```toml
[dotfiles]
"~/.gitconfig-work" = { mode = "track", variants = [{ profile = "work" }, { default = true }] }
```

When several variants match, the most specific one wins, and a tie makes mise
report the ambiguity and skip the path until you fix it. When nothing matches,
mise uses the variant marked `default = true`; without one, it neither saves
nor applies the path on that machine. Checkpoints keep the versions that other
machines saved. The [variant selectors](/dotfiles/reference.html#variant-selectors)
reference has the scoring rules. Tracked variants do not accept `target`;
to deploy a file to a different path on each machine, see
[Different targets on different machines](/dotfiles/managed.html#platform-specific-destinations).

#### One version per machine {#machine-variants}

Some files describe the machine itself, such as a monitor layout or a
trackpad setting. Give every machine its own version with a `machine`
variant:

```toml
[dotfiles]
"~/.config/hypr/monitors.lua" = { mode = "track", variants = [{ machine = true }] }
```

Or run `mise dot track ~/.config/hypr/monitors.lua --machine`.

Each machine saves, rolls back, and restores its own version. Sync shares the
other tracked files as usual, but never applies one machine's version on
another. Each version is still pushed to the origin with the rest of the
history, so it is kept off the machine and can be restored on it later.

The stream is named after the machine, for example `machine-omarchy-3f2a9c1b`:
its hostname and a random suffix, chosen the first time and kept in
`$MISE_STATE_DIR/history/machine`. Renaming the host does not change it, and
two machines with the same hostname still get separate versions. To choose the
name yourself, set it in the machine's global config, for example in
`~/.config/mise/config.local.toml`:

```toml
[history]
machine = "desk"
```

After reinstalling a machine, set its earlier name to continue that machine's
history. A `machine` variant must be the entry's only variant and cannot be
combined with `encrypt`. Upgrade every machine that shares the setup before
using it: older versions of mise refuse the setup rather than apply one
machine's version on the others.

### Local-only history {#local-only}

Some files are worth keeping a history of but should never leave this
machine: application state, a work-only configuration, a credential you want
to roll back. Track them with `mode = "track-local"`:

```toml
[dotfiles]
"~/.config/app/state.json" = { mode = "track-local" }
```

Or run `mise dot track --local ~/.config/app/state.json`.

Their versions are saved in this machine's own history, under
`$MISE_STATE_DIR/history-local`, which no origin ever reaches: neither their
contents nor their enrollment metadata enter shared history or a push. A
declaration written in a separately shared config file is still shared as part
of that file's text. Local files are saved, browsed, and restored like any
tracked file:

```sh
mise dot save                          # saves both histories
mise dot history --path ~/.config/app/state.json
mise dot rollback ~/.config/app/state.json
mise dot --local history               # list the local-only checkpoints
mise dot --local undo                  # undo the latest local-only rollback
```

Commands that name a path go to the history that keeps it; one command cannot
name paths of both. `mise dot --local` selects the local-only history for
commands without a path. `mise dot capture` saves a labeled checkpoint there
before and after the command, and the history watcher watches both.

A local-only path may lie inside a tracked directory, such as
`~/.config/app/state.json` inside a shared `~/.config/app`: the shared history
then leaves that file out. Versions already saved in the shared history before
the path became local stay there, as with
[untracking](/dotfiles/history.html#stop-tracking-a-file). If another machine
shares the same path, this machine neither applies its versions nor publishes
its own. Everything inside a local-only directory is local-only too: a shared
declaration of a path in it is refused.

`track-local` takes no `encrypt` or `variants`, since its history never leaves
the machine. Credential-named files are still left out unless the entry sets
`allow_plaintext = true`. Older versions of mise skip a `track-local` entry as
an unknown mode, so it never falls back to shared tracking.

### Track files that mise also deploys {#ownership}

You can save the history of a file that mise copies, links, renders, or edits.
For example, tracking `~/.zshrc` also saves the changes that a managed block
or [shell activation](/bootstrap/shell.html) writes into it.

When you track a file that a `[dotfiles]` entry already manages, mise keeps
that entry and writes the tracking entry to
`~/.config/mise/conf.d/dotfiles-tracking.toml`. Untracking removes only the
tracking entry. A tracked directory can also contain managed files or template
sources, and their copy, link, or template entries keep applying. Entries
that would create the same target are still rejected.

## Configuration reference {#configuration-reference}

History reads two tables. `[history]` holds what history saves and where it
goes, and mise reads it only from your global and system config, never from a
project. `[settings.history]` holds the [settings](/configuration/settings.html)
that control the watcher and sync, which you can also set with
`mise settings set` or environment variables.

| Table                      | Keys                                                                                                          | Purpose                                                                                                                                               |
| -------------------------- | ------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| `[history]`                | `exclude`                                                                                                     | Globs that history never saves; see [Exclude files across tracked entries](#exclude-files-across-tracked-entries)                                     |
| `[history]`                | `git_email`                                                                                                   | Author and committer email for history commits; see [Identify commits by machine](/dotfiles/sync.html#identify-commits-by-machine)                    |
| `[history.reload]`         | `"<glob>" = "<command>"`                                                                                      | Commands to run after rollback, undo, pull, or apply writes matching files; see [Reload an application](#reload-an-application-after-restoring-files) |
| `[history.encryption]`     | `recipients`                                                                                                  | Public keys that encrypted files are encrypted to; see [Encrypted files](/dotfiles/encryption.html)                                                   |
| `[history.origin]`         | `url`, `branch`                                                                                               | The origin, written to `config.local.toml` by `mise dot origin set`; see [Sync across machines](/dotfiles/sync.html)                                  |
| `[settings.history]`       | `enabled`, `sync`, `sync_interval`, `fetch_interval`, `notify`, `allow_plaintext_history`, `describe_command` | When history records and how it syncs                                                                                                                 |
| `[settings.history.watch]` | `debounce`, `max_interval`, `reconcile`                                                                       | Watcher timing                                                                                                                                        |

Write settings as TOML tables, for example:

```toml
[settings.history]
sync = "manual"
notify = false
```

`allow_plaintext_history` and `describe_command` are read only from global or
system config. Changes to the `history.watch` settings in global config take
effect while the watcher runs.

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="history" :level="3" />
