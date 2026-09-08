---
description: "Save, inspect, restore, and share the history of your tracked dotfiles."
---

# Dotfiles history

mise saves versions of your tracked configuration files in Git so you can
inspect changes and restore an earlier version. History stays local until
you connect a remote repository for sharing.

If you are starting with your first file, follow the
[dotfiles guide](/dotfiles.html#tracking-files-in-place) to track it and
start automatic saves. This page covers everyday history commands, sharing,
and the detailed behavior to consult when something needs attention.

## Saving

A **checkpoint** is a saved version of your tracked files, stored as a Git
commit. To save the current contents of one file:

```sh
mise bootstrap dotfiles save ~/.zshrc
```

To save all tracked files with a description:

```sh
mise bootstrap dotfiles save --description "before changing my theme"
```

An ordinary save with no changes creates no commit. Supplying
`--description`, `--label`, or `--task` creates a checkpoint even when the
files have not changed.

For a file tracked with `autosave = false`, save its edits by naming it:
`mise bootstrap dotfiles save <path>`. Automatic checkpoints keep that
file's last saved version. Commands that explicitly modify or capture it
can also save it; see [operation checkpoints](#operation-checkpoints).

`save` fails if it cannot save anything, for example because Git is missing,
history is disabled, or the requested path is untracked. Use `--best-effort`
in a script if saving should warn and let the script continue.

## Automatic saves

The **watcher** is a background service that saves changes to tracked files,
including edits made by your editor, an application, or another command.
Add it to your global mise configuration:

```toml
[bootstrap.services.mise-history]
builtin = "history-watch"
```

Install it and check that it is running:

```sh
mise bootstrap services apply
mise bootstrap dotfiles status
```

mise uses a systemd user service on Linux, a LaunchAgent on macOS, or a
Scheduled Task on Windows. The full `mise bootstrap` also installs it.
See [user services](/bootstrap/services.html#user-services) if it fails to start.

Ordinary edits are saved after the file has been quiet for two seconds by
default. Constantly changing files are saved less often, so a busy file
can have unsaved edits while other files have already been saved. Explicit
`save` commands still save immediately. See
[watcher scheduling](#adaptive-scheduling) for timing and settings.

To see files being saved less often:

```sh
mise bootstrap dotfiles paths --noisy
```

Logs, caches, databases, and session state usually belong outside your
tracked files. For a configuration file you want to save only on request:

```sh
mise bootstrap dotfiles track ~/.config/app/state.json --no-autosave
mise bootstrap dotfiles save ~/.config/app/state.json
```

Tracking saves the first version immediately. Later edits wait for an
explicit save. Check [watcher health](#health) if expected saves are missing.

## Comparing

List the checkpoints where a file changed, newest first:

```sh
mise bootstrap dotfiles history --path ~/.zshrc
```

Run `mise bootstrap dotfiles history` without `--path` to see all checkpoints.
Use an ID from the list to inspect one. The examples below use checkpoint 12:

```sh
mise bootstrap dotfiles history show 12
mise bootstrap dotfiles history diff 12 --patch --path ~/.zshrc
```

`history show` displays checkpoint details, including what triggered it and
which files changed. Add `--files` to list all its files or `--json` for
structured output. `history diff 12` shows what changed in that checkpoint.
To compare two checkpoints, supply both IDs:

```sh
mise bootstrap dotfiles history diff 11 12 --patch --path ~/.zshrc
```

To compare your current file with its latest saved version:

```sh
mise bootstrap dotfiles history diff --path ~/.zshrc
```

This also shows unsaved edits in files with `autosave = false`. Add
`--exit-code` to make a difference return exit status 1, or omit `--path`
to compare all tracked files.

## Referring to checkpoints

Use a checkpoint ID from `history`, or one of these references:

| Reference      | Meaning                                                            |
| -------------- | ------------------------------------------------------------------ |
| `12`           | Checkpoint with local ID 12                                        |
| `latest`       | Most recent checkpoint                                             |
| `latest~1`     | Checkpoint before the most recent one; use `~N` to go farther back |
| `commit:<sha>` | Git commit identified by a full hash or an unambiguous prefix      |

With `--path`, `latest~N` counts only checkpoints where that path changed.
For example:

```sh
mise bootstrap dotfiles history show latest~1 --path ~/.zshrc
```

This selects the checkpoint before the file's most recent change, even if
other files have changed since then.

Numeric IDs are local and may change if mise rebuilds its index. Use a Git
commit hash when you need a stable reference. Plain numbers always select
checkpoint IDs; nonnumeric, unambiguous commit prefixes also work without
`commit:`.

## Rolling back

To restore a file to its most recent saved version that differs from its
current contents, preview the change and then apply it:

```sh
mise bootstrap dotfiles rollback ~/.zshrc --dry-run
mise bootstrap dotfiles rollback ~/.zshrc
```

mise saves the current contents before replacing them. To reverse that
rollback, run:

```sh
mise bootstrap dotfiles undo
```

`undo` restores the tracked files changed by the operation. Other files
keep their current contents. Both rollback and undo create new commits,
so earlier versions remain available and the restored version can sync to
other machines.

To choose a checkpoint, use an ID from `history` or a
[checkpoint reference](#referring-to-checkpoints):

```sh
mise bootstrap dotfiles rollback ~/.zshrc --to 42
mise bootstrap dotfiles rollback --to latest~3 --all --dry-run
```

`--all` selects everything covered by the chosen checkpoint. If that
checkpoint recorded a file as absent, rollback deletes the file. This can
also happen without `--to`: an earlier saved state where the file was
absent counts as a version to restore.

The preview reports each path as:

| Action      | Meaning                                                                                       |
| ----------- | --------------------------------------------------------------------------------------------- |
| `write`     | Replace the file with different saved contents                                                |
| `delete`    | Remove a file recorded as absent                                                              |
| `unchanged` | Keep a file that already matches                                                              |
| `skip`      | Leave a path the checkpoint did not cover or omitted                                          |
| `conflict`  | Resolve a changed path type, such as a file becoming a directory; replacement needs `--force` |

Git does not record empty directories, so rolling back a directory leaves
unrecorded empty folders alone. Restoring configuration files leaves
installed packages and running services as they are. Run `mise bootstrap`
when you want to apply the restored configuration.

### Reload an application after restoring files

Add commands to global configuration to reload an application after its
files change:

```toml
[history.reload]
"~/.config/hypr/**" = "hyprctl reload"
```

Each matching command runs once, after the files have been restored. mise
loads these commands from system and global configuration before starting
the operation. See [recovery details](#recovery-details) for interrupted
writes and concurrent edits.

## Sharing across machines

Connect a private Git repository, called an **origin**, to share tracked
files between machines. Install the [watcher](#automatic-saves) first and
make sure it can use your Git credentials. See
[repository authentication](#repository-authentication) if you need to set them up.

Before connecting, review your tracked files and their earlier checkpoints.
All committed versions are shared. Deleting a secret from the current file
leaves it in older commits. Configure [encryption](#encrypted-shared-files)
before first saving a file that needs it.

Create an empty private repository and replace `you/setup` with its name:

```sh
mise bootstrap dotfiles origin set https://github.com/you/setup.git --sync sync
mise bootstrap dotfiles status
```

Review the connection preview before confirming. With `--sync sync`, the
watcher pushes saved changes and periodically fetches and applies changes
from other machines. To bring another machine into this workflow, follow
[Set up a machine](/bootstrap/setup.html#set-up-another-machine).

### Choose a sync mode

The `history.sync` setting controls what the watcher does automatically:

| Mode         | Behavior                                                                                                                                                |
| ------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `sync`       | Push saved changes within `history.sync_interval` (5 minutes by default). Fetch and apply incoming changes every `history.fetch_interval` (15 minutes). |
| `manual`     | Keep saving locally. Exchange and apply changes when you run the commands below.                                                                        |
| `fetch-only` | Fetch remote changes. Wait for an explicit command to push or apply anything.                                                                           |

Set a mode with `--sync manual|sync|fetch-only` when connecting, or change
it later with `mise settings set history.sync MODE`.

Without `--sync`, interactive `origin set` asks whether to enable automatic
sharing. Declining selects `manual`. `--yes` uses the configured mode,
which defaults to `sync`; specify `--sync` in scripts to choose explicitly.

### Sync immediately

These commands work in every mode:

```sh
mise bootstrap dotfiles sync
mise bootstrap dotfiles pull
```

`sync` pushes saved commits and fetches remote changes. `pull` applies
pending changes to your files. In automatic mode, use them when you want
to sync before the next scheduled run.

Each push includes all accumulated commits, including intermediate saves
made in manual mode. Files waiting for a manual save or a delayed watcher
save contribute their last saved contents.

When incoming changes update tools, services, or template sources, run
`mise bootstrap` to apply that configuration. `pull` restores shared file
contents; `mise bootstrap dotfiles apply` creates files from the sources,
templates, and edits in your `[dotfiles]` configuration.

A conflict or an unsaved edit can pause pushing and applying incoming
changes for all tracked files. Local saves and fetching continue. If a
network or authentication attempt fails, automatic sync retries with
increasing delays from one minute to one hour. `status` and `mise doctor`
show the last error.

### Resolve a conflict

Inspect the reported files:

```sh
mise bootstrap dotfiles status
```

Choose the remote version of a file, or keep the local version:

```sh
mise bootstrap dotfiles pull --take-remote ~/.zshrc
mise bootstrap dotfiles pull --keep-local ~/.zshrc
```

Run the command for the choice you want. mise records each decision and
waits until every conflict is resolved before applying the incoming files.
It checks the plan again before continuing. A newer local or remote edit
can invalidate a decision, in which case you must choose again.

Invalid incoming configuration or unsafe local paths also block the
complete batch. `status` and `doctor` identify the paths and the last
successful application. For conflicts involving tracking or encryption
settings, inactive platform variants, or multiple Git merge bases, follow
the reported Git-level repair instructions in a separate checkout.

Pull saves a checkpoint first, records each file it writes, and runs reload
hooks afterwards. You can reverse it with `mise bootstrap dotfiles undo`.
Writes happen one file at a time; interrupted work uses the
[recovery process](#recovery-details).

### Conflict notifications

Desktop notifications are enabled by default for sharing conflicts. They
point to `mise bootstrap dotfiles status` for resolution steps. A pause
produces one notification; further retries during the same pause stay quiet.
A new pause after recovery can notify again.

On Linux, install `notify-send`. On macOS, allow notifications for mise
when prompted, or enable them in System Settings → Notifications → mise.
Unofficial macOS builds, including Homebrew, warn when connecting because
notifications are unavailable. Use `status` or `doctor` on Windows and
headless systems. Missing or failing notifications leave history and sync
running. Set `settings.history.notify = false` to disable notifications.

### Repository authentication

Network commands use your Git configuration, including credential helpers,
SSH, and URL rewrites. For a private GitHub repository, install the GitHub
CLI globally through mise and configure its credential helper:

```sh
mise use -g gh
mise x gh -- gh auth login --hostname github.com --git-protocol https --web
mise x gh -- gh auth setup-git --hostname github.com
```

The helper is written to `~/.gitconfig`. Keeping `gh` installed globally
keeps it available to the background watcher. If Git authentication already
works in the service's environment, you can use it as-is. SSH remotes need
an accessible agent or an unencrypted key in that environment.

### How shared history is stored

The origin is recorded in `[history.origin]` in `config.local.toml`.
The sync mode is recorded in `settings.history.sync`.

Git stores paths relative to `home/` or `config/`, so each machine can
restore them beneath its own home or mise configuration directory. Variants
use a suffix such as `home@macos/`. These mappings also apply to tracked
template sources and managed files. Repository files such as a README stay
in Git rather than being restored as live configuration.

`mise bootstrap --from-git <url>` recognizes this setup repository and
fetches it into the history store. It restores the shared configuration
first, then the tracked files it selects for this machine, and remembers
the origin. Once conflicts are resolved, it runs bootstrap to install
packages, tools, and services and render templates. The configuration and
required sources must have been tracked and shared for those steps to work.
See [repository bootstrap](/bootstrap.html#starting-from-a-repository) for
how this differs from cloning a global configuration repository.

Synchronization uses Git ancestry, fast-forwards, and merge commits. Pushes
retain the saved commits and their Git author identities. A rejected push
triggers another fetch and reconciliation. mise leaves divergent or
unrelated histories intact for you to resolve; it does not force-push.
Before writing incoming changes, it checks the complete batch, including
configuration, required sources, committed files, and unsaved local edits.

## Capturing an external command

To inspect what an update changed in your tracked dotfiles, wrap it with
`capture`. For example, on Omarchy:

```sh
mise bootstrap dotfiles capture --label "omarchy update" -- omarchy-update
mise bootstrap dotfiles history --label "omarchy update"
mise bootstrap dotfiles history diff --operation --patch
```

`capture` saves tracked files before and after the command, including files
with `autosave = false`. It records the label and whether the command
succeeded. The diff compares those two checkpoints, even if other saves
happened later. To inspect an older operation, supply its checkpoint ID:

```sh
mise bootstrap dotfiles history diff 42 --operation --patch
```

The command runs directly with inherited input, output, and environment.
Use `-- sh -c '...'` when you need shell syntax. A capture failure warns
and lets the command run with its own exit status. Failed commands and
commands that make no changes still retain their checkpoint pairs.

Other history writers wait until the pair is complete. Editors and other
programs can still change files during that time, and their edits appear
in the comparison too. `undo` restores tracked-file changes across the
whole interval. To restore selected files, find the operation's **Before**
checkpoint with `history show`, then use `rollback <path> --to <before-ref>`.
Packages, service state, and untracked files are outside this recovery.

If the wrapper is abruptly terminated, the next command that changes
history recovers its pending operation record. External writes are not
journaled individually. If the wrapped command untracks a file, the after
checkpoint leaves it out; its earlier checkpoint remains available, and
recovery keeps it untracked.

## Explicit tracking and exclusions

Use `mode = "track"` for every file or directory you want to save in history.
For example, in your global configuration:

```toml
[dotfiles]
"~/.zshrc" = { mode = "track" }
"~/templates" = { mode = "track" }
"~/.gitconfig" = { mode = "template", source = "~/templates/gitconfig.tera" }
```

Here, history saves `.zshrc` and the files in `~/templates`. The template
creates `.gitconfig`; track that output separately if you want its history
too. You can track files mise also copies, links, or edits.

Tracking takes exact file or directory paths. Directories include new
files added beneath them. Symlinks record the link itself; track their
targets separately to save those contents. To share tools, services, and
template setup, track the relevant mise configuration and sources too.
Bootstrap reports required files missing from history.

Use glob patterns to exclude files from history:

```sh
mise bootstrap dotfiles exclude '~/.config/hypr/plugins/**'
mise bootstrap dotfiles include '~/.config/hypr/plugins/**'
mise bootstrap dotfiles paths
```

Exclusions are stored in `[history] exclude`. A later `!glob` reverses an
earlier matching exclusion. `paths` lists tracked paths and files omitted
from saves. Protected credential files and `*.local.toml` are excluded by
default; use [encrypted tracking](#encrypted-shared-files) for credentials
you want to save.

Logs, caches, databases, and constantly rewritten session state usually
belong outside history. Use `autosave = false` for configuration you want
to save manually. An excluded file is left out of future saves entirely.

To stop tracking a file:

```sh
mise bootstrap dotfiles untrack ~/.zshrc
```

The file stays in place, while future checkpoints leave it out. Earlier
committed versions remain in Git and can still be shared. There is no
per-file local-only history setting.

## Encrypted shared files

Configure encryption before saving a file's private contents for the first
time. Add public recipients and mark the file or directory for encryption:

```toml
[history.encryption]
recipients = ["<age-or-plugin-public-recipient>", "<recovery-public-recipient>"]

[dotfiles]
"~/.config/app/credentials" = { mode = "track", encrypt = true }
```

Replace the placeholders with public recipients for your machines and an
independent recovery key. Keep private decryption keys outside tracking.
Configure local identities with `settings.age.identity_files`,
`settings.age.key_file`, or the supported SSH identity settings. Public
recipients travel with the repository.

mise encrypts contents before storing them in Git. Filenames and public
metadata remain visible. The files you edit or restore stay unencrypted.
Missing keys or recipients stop the operation; mise never falls back to
saving plaintext.

You can also encrypt tracked template sources:

```toml
[dotfiles]
"~/templates/private" = { mode = "track", encrypt = true }
"~/.config/app/config" = { mode = "template", source = "~/templates/private/app.tera" }
```

The rendered output stays unencrypted. Configure its permissions and any
tracking separately.

Adding encryption later leaves earlier plaintext versions in Git. Before a
push, mise checks all reachable commits, including intermediate saves and
merge parents, for violations of encrypted-path settings. An earlier
plaintext version blocks the push even if the newest version is encrypted.
You must explicitly rewrite or replace that history. This checks encryption
settings; it does not scan arbitrary unencrypted files for secrets.

## Checking watcher health {#health}

If files are not being saved or shared, start with:

```sh
mise doctor
mise bootstrap dotfiles status
```

`doctor` summarizes a watcher that is declared but stopped, repeated save
failures, an unusable history store, and files being saved less often because
they change constantly. It includes the command to start a stopped watcher.

`status` gives more detail: whether the watcher is running, declared but
stopped, or not declared; the latest save and full scan; the last failure;
and each busy file's save interval, last save, and pending edits.

These commands read the watcher's saved health report without starting
synchronization or changing files. The report lives in `health.json` in the
history store. Old reports are marked stale after a few full-scan intervals.
A busy file's longer save interval is informational.

The watcher sends desktop notifications when a sharing conflict needs
attention. See [conflict notifications](#conflict-notifications) for setup
and platform support.

## What a checkpoint records

History lives in a bare Git repository at `$MISE_STATE_DIR/history/repo.git`,
separate from the files you edit. Each checkpoint contains the tracked files
and metadata for their paths, tracking settings, variants, encryption, and
permissions. The files are ordinary Git tree entries.

A symlink is saved as a link. A nested Git repository is saved as a pointer
to its commit. mise reports oversized files, special files, and unreadable
paths it cannot save. It keeps their previous saved versions while saving
other files, so check reported omissions before relying on a checkpoint.
Explicit exclusions remove paths from future checkpoints. Encryption
failures stop a save rather than storing plaintext.

Commands that modify or capture tracked files save checkpoints before and
after their work. Their metadata includes operation labels and the link
between the two checkpoints. Raw command arguments, environment contents,
and temporary recovery copies are left out of committed metadata.

Checkpoints restore file contents. They do not restore installed packages
or the running state of a service. Use your system's backup tools for that
state, and run bootstrap explicitly to apply restored configuration.

### Descriptions from an agent

`settings.history.describe_command` can run a command to describe each
checkpoint saved by the watcher. For example, with Claude Code installed:

```toml
[settings]
history.describe_command = "claude -p --output-format text --no-session-persistence 'Describe this change to my configuration files in one line of at most 120 characters, plain text, no quotes.'"
```

This sends change details, including unencrypted file diffs, to the command
you configure. Excluded private files are not named, and encrypted file
contents are not sent.

The command receives one JSON object on stdin. Its fields are `uuid`,
`trigger`, the computed `description`, the `added`, `modified`, and `removed`
paths, and a unified `diff` of changed unencrypted files. The diff is limited
to 64 KiB; `diff_truncated` reports truncation.

Print one line of at most 200 characters. mise uses it as the checkpoint
description and records `description_source: command`. The checkpoint is
saved before the command runs. If the command fails, returns no text, or
exceeds 30 seconds, mise keeps its computed description.

Commands run one at a time, once per checkpoint saved by the watcher.
Filesystem events and retries do not trigger separate descriptions.
Contents are passed through stdin without shell interpolation.

## Recovery details

Before rollback changes any file, mise saves the current contents in a
`rollback-before` checkpoint. If it cannot save every file it would change,
it stops before writing. It checks the plan again after the checkpoint,
then checks each path immediately before replacing it. A concurrent edit
that invalidates the plan stops the operation.

Files are written one at a time, with a journal recording each affected
path. An interruption can leave some writes completed. Use
`mise bootstrap dotfiles recover` to retry unfinished writes. If later edits
prevent recovery, inspect the reported paths. To accept the current files:

```sh
mise bootstrap dotfiles recover <operation> --keep-current
```

After confirmation, this discards that operation's temporary recovery
copies. It keeps your current files and committed history. An ordinary
recovery retry preserves later edits and recovery copies when it cannot
continue. Unresolved recovery data is kept until you resolve it.

### Operation checkpoints

When bootstrap modifies a tracked file with `autosave = false`, it saves
that file's actual contents before the operation. Unrelated manual edits
stay unsaved. If the operation writes a file repeatedly, the before version
still holds the contents from before the first write. Encrypted files are
also encrypted in these checkpoints.

`undo` uses an operation's before checkpoint to restore the tracked paths
it changed. For untracked files, temporary recovery copies only support
completing or recovering interrupted writes; they are deleted afterwards.
Those files have no historical undo.

Ordinary bootstrap writes warn and continue if they cannot save temporary
recovery contents, for example for an oversized destination. Such a write
cannot be recovered automatically after interruption. Pull, rollback, and
undo refuse writes without the required recovery data. Disabling history
lets ordinary bootstrap run without the history store; explicit history
commands still require their recovery data.

## Watcher reference

The watcher watches tracked directories recursively. For a tracked file it
watches the parent directory; for a missing path it watches the nearest
existing ancestor. Files with `autosave = false` are left for explicit saves.

### Adaptive scheduling

mise schedules each tracked file separately. **Throttling** means waiting
longer between automatic saves for a file that changes constantly.

1. An ordinary edit is saved after `history.watch.debounce` of quiet time
   (two seconds by default).
2. If a file keeps changing without settling between saves, its save
   interval doubles, up to `history.watch.max_interval` (24 hours by default).
3. When the file stops changing, mise saves its final contents after a
   fraction of that interval, at most five minutes.
4. After a quiet period of four intervals, with a minimum of five minutes,
   the file returns to the base interval.

The longer interval affects only that file. Other files save normally, and
explicit saves and checkpoints before bootstrap, rollback, or undo still
run immediately. Throttled files continue to be saved periodically; they
are never automatically excluded or switched to manual saving.

When another file triggers a checkpoint, or mise scans the full tracked
set, a throttled file keeps its last saved contents until its own save is due.

The interval doubles when at least two changes have arrived since the
previous save and the file changed again within its settling period.
Ordinary editor saves spaced beyond that period keep the usual interval.

The watcher stores schedules in `watch-schedule.json`, including each busy
file's last save and pending edits. Restarting preserves these schedules.
At startup, a throttled file keeps its saved version until its next save is
due, including when it changed while the service was stopped.

Changes to `history.watch.debounce`, `history.watch.max_interval`, and
`history.watch.reconcile` in global configuration take effect while the
watcher is running.

When a full scan discovers a new tracking entry, it saves the initial
version even with `autosave = false`. Later scans carry that saved version
forward until you explicitly save the path.

### Reconciliation and failures

The watcher also scans all tracked files to catch changes missed by
filesystem notifications. It does this at startup, at shutdown, when
configuration changes, and every `history.watch.reconcile` (ten minutes by
default). Set that interval to `0` to disable periodic scans.

Edits to global TOML configuration or `conf.d/` reload the tracked paths
and update their watches. Setting `history.enabled = false` stops the watcher.
To run one scan from a timer or cron job, use
`mise bootstrap dotfiles watch --once`.

Failed saves remain pending and are retried with increasing delays from
one second to five minutes. A save that overlaps bootstrap, rollback, or
undo waits and retries after that operation finishes. At shutdown, the
watcher waits briefly for a running operation and reports anything it
could not save.

`watch --once` exits 1 if its save was deferred or failed. Only one watcher
can run per history store; starting another exits successfully immediately.
`--json` prints one object per line (`started`, `captured`, `unchanged`,
`deferred`, `replan`, `throttled`, `settled`, `degraded`, `error`, `stopped`).

## Retention

Reachable Git history is retained indefinitely. There is no automatic
checkpoint expiration or compaction. Untracking a path does not erase old
commits, and encrypting its latest version does not erase earlier plaintext.

## Requirements and settings

History needs a `git` binary (on macOS, the Xcode Command Line Tools). Without
one, `mise bootstrap dotfiles save` fails and bootstrap commands still run, recording
their journals without content; `mise bootstrap dotfiles status` says so.

`settings.history.enabled` defaults to `true`. Disabling it stops automatic
capture; it does not delete committed history.

Keep your repository and decryption identities recoverable independently.
Repository authentication and an age identity serve different purposes:
a replacement machine needs both Git access and a matching identity to
restore encrypted files. Test fresh-machine setup before relying on it.
