# Dotfiles history

mise automatically commits changes to files you explicitly track, then
synchronizes those same commits with your origin. Edit native files with
your editor or an agent; there is no separate source format to maintain.

Only exact files and directories declared with `mode = "track"` are enrolled.
Directories include new descendants, subject to exclusions. The global mise
configuration, `dotfiles.root`, deployment outputs, template sources, and
symlink targets are not automatically tracked. A tracked symlink records the
link itself; enroll its target separately if you want its contents in history.

`mise bootstrap dotfiles history` browses the ordinary Git history. Automatic
saves and explicitly labeled before/after operation boundaries are commits
in that same history, retained indefinitely.

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

The bare repository lives under `$MISE_STATE_DIR/history/repo.git`, separate
from your live files. mise does not create `.git` in your home or application
configuration directories. Without an origin it stays local. Connecting an
origin makes **all committed versions** eligible for synchronization, including
commits made before connecting. There is no per-file local-only history mode.

Untracking stops future capture and removes the path from future committed
trees, without deleting its live contents. It does not erase previously
committed versions.

This also applies inside `capture -- command`: if the command untracks a file,
the outcome commit does not save its later contents. The before commit remains
available, and interrupted-command recovery does not enroll the file again.

## What a checkpoint records

Each commit contains the tracked files plus minimal repository metadata for
portable paths, enrollment, variants, encryption, and permissions. Files remain
ordinary Git tree entries, not a wrapper snapshot or filtered publication copy.

Symlinks are stored as links and nested Git repositories as pointers. Oversized
files, special files, and unreadable paths are reported rather than silently
claimed as saved. If a file cannot be captured, its previously saved version
remains in the tree while other files can still be saved; this does not claim
that its current contents were captured. Explicit exclusions still remove it
from future trees. Encryption failures stop capture instead of storing plaintext.
Operation boundaries carry labels and pairing information;
raw arguments, environment contents, and untracked recovery copies are not
published as operation metadata.

`mise bootstrap dotfiles history show <ref>` shows a saved version; `--files`
lists its files and `--json` returns the record.

### Descriptions from an agent

`settings.history.describe_command` names a command that describes the
checkpoints the watcher saves. It gets one JSON object on stdin, `uuid`,
`trigger`, the computed `description`, the changed paths that are not
excluded (`added`, `modified`, `removed`), and `diff`, a unified diff of the
changed unencrypted files (at most 64 KiB, `diff_truncated` says
when it was cut), and prints one line of at most 200 characters, which
becomes the description (`description_source: command`). With Claude Code:

```toml
[settings]
history.describe_command = "claude -p --output-format text --no-session-persistence 'Describe this change to my configuration files in one line of at most 120 characters, plain text, no quotes.'"
```

The checkpoint is saved before the command runs and keeps its computed
description when the command fails, prints nothing, or takes longer than 30
seconds. The command runs once per checkpoint the watcher saved, one at a
time, never per filesystem event or retry, and never with a shell
interpolation of file contents (the JSON is its stdin). A private file
(`*.local.toml`, a credential store) is never named, and an encrypted file never has its contents sent.

## Referring to checkpoints

Commands take a numeric ID, `latest`, `latest~N`, or `commit:<sha>` (a full hash
or an unambiguous prefix). Plain numbers always mean checkpoint IDs, never
commit prefixes; nonnumeric unambiguous commit prefixes also work directly. With
`--path`, `latest~N` counts only the checkpoints where that path changed, so
`mise bootstrap dotfiles history show latest~1 --path ~/.zshrc` is the state before its most
recent change however many other checkpoints came in between.

Numeric ids are local handles and can change when the index is rebuilt.
Git commit identities are stable.

Commit metadata contains only portable file information and operation labels.
Raw command arguments, environment values, and temporary recovery copies are
not part of the committed history.

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
disk. `mise bootstrap dotfiles history diff --path <file>` shows saved against live. The last saved version is in the ordinary parent commit, not a separate
promotion history.

## Rolling back

```sh
mise bootstrap dotfiles rollback ~/.config/hypr/bindings.lua        # its most recent saved version that differs from disk
mise bootstrap dotfiles rollback ~/.zshrc --to 42                    # that checkpoint's version
mise bootstrap dotfiles rollback --to latest~3 --all --dry-run       # everything the checkpoint covers
mise bootstrap dotfiles undo                                         # reverse the newest tracked-file operation
```

A rollback is planned first: for every selected path, `write` when the
checkpoint holds a different version, `delete` when the checkpoint knows the
path was absent, `unchanged`, `skip` when the checkpoint never covered or
omitted it, or `conflict` when the path changed type (a file became a
directory or a symlink) — conflicts need `--force`. Without `--to`, a
checkpoint that knew the path was absent counts as a version to return to,
so a file created since rolls back to "missing". `--dry-run` stops after the
plan.

Rolling back a parent directory leaves unrecorded empty folders alone: Git
does not record empty directories.

Then the current state of the affected paths is saved in a protective
checkpoint (`rollback-before`); the plan is verified against the working tree
again (an editor may have written meanwhile) and every path about to change
must be captured in that checkpoint as it is now, or the rollback stops
without touching anything. Files are written one at a time, each journaled
and recorded as affected as soon as it is written, and each checked once
more right before it is replaced (a file that appeared meanwhile stops the
rollback there). Only afterwards do `[history.reload]` commands run — once per matching
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

`mise bootstrap dotfiles undo` restores the tracked paths an operation changed,
using its before commit and leaving unrelated work alone. Undo itself creates
a new commit. Untracked files have no historical undo: temporary recovery
copies exist only to complete or safely recover interrupted writes, and are
deleted afterwards. Unresolved recovery is never expired; concurrent edits are
preserved and reported for action.

When bootstrap changes a tracked file with `autosave = false`, its actual
pre-operation contents are saved in the ordinary before history. Unrelated
manual edits stay unsaved. Repeated writes within the operation preserve the
first preimage, and encrypted preimages are encrypted before entering Git.

Ordinary bootstrap deployments warn and continue if a temporary preimage cannot
be captured, for example for an oversized destination. Such a write cannot be
automatically recovered after interruption. History-driven pull, rollback, and
undo instead refuse writes without the required recovery data. Disabling history
recording also removes ordinary bootstrap's dependency on the history store;
explicit history-driven writes retain their recovery safeguards.

Run `mise bootstrap dotfiles recover` to retry interrupted writes safely. If
later edits prevent recovery, inspect the reported paths first. You can then
explicitly accept the live files with
`mise bootstrap dotfiles recover <operation> --keep-current`. After confirmation,
this discards only that operation's temporary recovery copies; it does not
change the live files or erase committed history. Ordinary retries keep both
the later edits and recovery copies when they cannot proceed safely.

## Capturing an external command

```sh
mise bootstrap dotfiles capture --label "omarchy update" -- omarchy-update
mise bootstrap dotfiles history --label "omarchy update"
mise bootstrap dotfiles history diff --operation --patch
mise bootstrap dotfiles history diff 42 --operation --patch
```

`capture` saves tracked files before and after the command and records the link
between those checkpoints, the label, and whether the command succeeded. It runs
the command directly with inherited input, output, and environment; raw command arguments and environment contents are not committed. Use
`-- sh -c '...'` for shell syntax. Capture failures warn and the command still
runs with its own exit status. No-op and failed runs retain their checkpoint
pairs. Files with `autosave = false` are explicitly saved by this command too.

`history diff --operation` compares the newest operation with its own protective
checkpoint, even if later saves exist. Supply one checkpoint reference to select
an older operation. The paired commits remain in the ordinary history.

The operation lock keeps the watcher and other history writers from inserting
checkpoints into the pair. Concurrent changes by editors or other programs are
still part of the observed interval. An abruptly terminated wrapper leaves a
pending operational record for recovery by the next history mutation. Capture does
not journal external writes individually: `undo` restores the tracked-file
changes observed over the whole interval, including concurrent edits. To restore
selected files instead, use `history show` to find its **Before** checkpoint, then
`rollback <path> --to <before-ref>`. Package changes,
service restarts, and files outside the tracked set are not restored.

## Automatic saves

`mise bootstrap dotfiles watch` saves tracked files as they change, whatever
wrote them: an editor, a script, an agent, a distro update, or mise itself.
Declare it once as the built-in user service and `mise bootstrap` installs
and starts it on every platform (a systemd user unit, a LaunchAgent, or a
Scheduled Task):

```toml
[bootstrap.services.mise-history]
builtin = "history-watch"
```

```sh
mise bootstrap services apply        # or the full `mise bootstrap`
mise bootstrap dotfiles status       # watcher: running
```

The watcher installs filesystem watches for every autosaved entry (a tracked
directory recursively, a tracked file through its parent, a path that does
not exist yet through its nearest existing ancestor). Manual-save entries
(`autosave = false`) are never watched.

### Adaptive scheduling

Every file is scheduled on its own. An ordinary edit is saved once the file
has been quiet for `history.watch.debounce` (2s). A file that is rewritten
constantly is not saved on every change: when a save follows the previous
one without the file ever settling, that file's own interval doubles, up to
`history.watch.max_interval` (24h). It is still saved periodically at that
interval for as long as it keeps changing; nothing is ever excluded or
switched to manual saving automatically, and a checkpoint another file
triggers carries the throttled file's last saved version, not its live
content, so a whole-set reconciliation never defeats the throttling. As soon
as the file stops changing its final state is captured promptly (after a
fraction of its interval, at most five minutes), and a sustained quiet
period (four intervals, at least five minutes) resets it to the base interval.
A busy file never delays an ordinary one. Throttling does not delay explicit
saves or protective captures before bootstrap, rollback, or undo.

The thresholds are fixed: an interval doubles when a file changed again
within its settle time of the previous save and at least two changes
arrived since. A person saving from an editor every few seconds leaves gaps
longer than the settle time, so ordinary editing is never stretched. The
schedule is persisted (`watch-schedule.json` in the history store) with
each throttled file's last save and pending changes, so a restart of the
service continues where it stopped: the startup capture holds a throttled
file at its saved version until its next save is due, and a file rewritten
while the service was down is pending, not saved early. Editing
`history.watch.debounce`, `history.watch.max_interval`, or
`history.watch.reconcile` in the global configuration takes effect while
the service runs.

Constantly rewritten application state, logs, caches, and databases are
better excluded, and a file that genuinely holds configuration but changes
constantly can be tracked with `autosave = false` and saved explicitly:

```sh
mise bootstrap dotfiles paths --noisy                      # what is throttled right now
mise bootstrap dotfiles exclude '~/.config/hypr/plugins/**' # [history] exclude
mise bootstrap dotfiles include '~/.config/hypr/plugins/**'
mise bootstrap dotfiles track ~/.config/app/state.json --no-autosave
mise bootstrap dotfiles save ~/.config/app/state.json
```

Enrollment saves the initial version even with `autosave = false`, including
when a new declaration is first picked up by reconciliation. Later edits to
that entry require an explicit save; ordinary reconciliation carries its saved
version forward.

### Reconciliation and failures

The whole tracked set is reconciled at startup, every
`history.watch.reconcile` (10m; `0` disables), when the configuration
changes (an edit to `~/.config/mise/*.toml` or `conf.d/` reloads the
declarations and replans the watches; `history.enabled = false` stops the
watcher), and on shutdown, so an edit no watch reported is still saved.
`mise bootstrap dotfiles watch --once` runs one reconcile and exits, for a
timer or cron instead of the service.

A capture that fails is retried with backoff (1s to 5min) and never drops
the pending changes; one that would overlap another history operation (a
running bootstrap, rollback, or undo) is deferred and retried until that
operation finishes, whether or not any other save is due. The shutdown
capture waits a moment for a running operation and says what stays unsaved
if it cannot. `mise bootstrap dotfiles watch --once` exits 1 when nothing
could be saved (deferred or failed), so a timer notices. One watcher runs
per store: a second one exits 0 immediately.
`--json` prints one object per line (`started`, `captured`, `unchanged`,
`deferred`, `replan`, `throttled`, `settled`, `degraded`, `error`,
`stopped`).

### Health

The watcher speaks up only when sharing pauses for a conflict (a desktop
notification on Linux and macOS, on by default; see
[Sharing across machines](#sharing-across-machines) below). Otherwise it persists its
health (`health.json` in the history store) and two commands read it, without
starting a sync, applying anything, or prompting:

- `mise doctor` prints a concise `dotfiles` section: a watcher that is
  declared but not running (with the command that starts it), repeated
  capture failures or an unusable store, and heavily throttled files. A
  throttled file is informational, not a warning. Health older than a few
  reconcile intervals is reported as stale rather than current.
- `mise bootstrap dotfiles status` prints the detail: the watcher state
  (`running`, `declared but not running`, `not declared`), the last capture
  and reconcile, the last failure, and for every throttled file its
  effective interval, last save, and unsaved changes (changes seen since the
  last save, kept current as they happen).

## Sharing across machines

One ordinary repository holds your committed tracked files. Connect it once
per machine ([Set up a machine](/bootstrap/setup.html) walks through the workflow):

```sh
mise bootstrap dotfiles origin set https://github.com/jdx/dotfiles.git
mise bootstrap dotfiles status
mise bootstrap dotfiles sync
mise bootstrap dotfiles pull
```

`pull` and `apply` are different commands on purpose: `mise bootstrap
dotfiles apply` keeps deploying your own `[dotfiles]` declarations (symlinks,
copies, templates, edits), while `pull` writes what other machines shared
through the setup repository.

When `--sync` is omitted, interactive `origin set` asks whether to publish saved
edits and apply incoming changes automatically. Answering no selects `manual`;
local autosave continues. `--sync manual|sync|fetch-only` selects a mode directly.
`--yes` accepts the configured mode (default `sync`) without this question, so
scripts should specify `--sync` explicitly.

`origin set` previews the connection before publication. Every tracked file
is eligible to be pushed, including its accumulated history. Files that must
never reach origin must not be tracked. Protected credential-file defaults
exclude capture entirely; they do not create private recovery history.

The connection is recorded in `[history.origin]` in `config.local.toml`,
with the synchronization mode in `settings.history.sync`.

The repository uses portable `home/…` and `config/…` paths; variant streams
have a root suffix such as `home@macos/…`. Mapping does not change when a file
is also a template source or managed output. A README or other repository-owned
file stays in Git, not in a live configuration directory.

Encrypted tracked files enter Git as ciphertext from their very first capture.
Filenames and public metadata remain visible. See [Encrypted shared files](#encrypted-shared-files).

**Another machine.** `mise bootstrap --from-git <url>` on a machine that has
nothing yet recognizes the marker and sets the machine up from the
repository: the branch goes into mise's own store (no checkout in
`~/.config/mise`), the shared configuration, its sources, and this machine's
tracked files are written by one recoverable pull (the configuration first,
then what it declares; a file that already exists and differs is held for a
decision), the connection is remembered, and the ordinary bootstrap runs:
packages, tools, templates rendered from the sources that just arrived, the
watcher service. From then on the machine syncs like any other. Over SSH the
same happens through `mise bootstrap remote`; see
[remote bootstrap](/bootstrap/remote.html).

**How a sync decides.** Synchronization uses Git ancestry, fast-forwards,
and merge commits. It pushes the same local commits, without squashing
intermediate saves or constructing a separate publication history. A rejected
push fetches and reconciles again; mise never silently force-pushes or discards
divergent history. Unrelated nonempty histories must be reconciled explicitly,
not automatically replaced.

Incoming application is preflighted as a complete batch, including enrolled
configuration and required sources. Both committed and unsaved local versions
are checked again before writing. An unresolved conflict pauses publication
and incoming application for the entire setup; local commits and fetching
continue.

**Modes** (`settings.history.sync`) say what the watcher does on its own:

- `sync` (the default, recommended): after a save the watcher publishes
  within `history.sync_interval` (5 minutes); every `history.fetch_interval`
  (15 minutes) it fetches and applies incoming changes, with
  a protective checkpoint first. A conflict or an unsaved edit pauses
  publication and incoming application for the complete setup.
- `fetch-only`: the watcher fetches; nothing is ever published and no live
  file changes until you run `mise bootstrap dotfiles pull`.
- `manual`: no automatic network activity. Local commits continue; the next
  explicit sync pushes all accumulated intermediate commits unchanged.

`mise bootstrap dotfiles sync` (publish and fetch now) and `mise bootstrap
dotfiles pull` (write what is pending now, decide conflicts) work on request
in every mode: they are for when you do not want to wait, not something the
background mode needs you to run. A failed sync (the repository unreachable,
credentials missing) backs off from a minute to an hour and is retried;
saving continues meanwhile, and `mise bootstrap dotfiles status` and
`mise doctor` show the last error. The watcher never publishes a throttled
file's unsaved churn or a manual-save entry's unsaved edits: what it
publishes is what it saved. Applying never runs `mise bootstrap`, installs
or removes packages, or renders templates: when incoming configuration
changes declarations, `mise bootstrap dotfiles status` says to run
`mise bootstrap`.

**Conflict notifications** are on by default. A desktop notification
(`notify-send` on Linux, a bundled mise helper app on macOS) reports when sharing pauses
for the setup. Retries and additional conflicts during the same pause stay
silent; a later pause can notify again after recovery. Set
`settings.history.notify = false` to opt out. Missing or failing notifiers
never hold up history or sync.

Linux and macOS notifications use the mise logo. On macOS, allow notifications
for mise when asked on first use; you can change that permission in System
Settings > Notifications > mise. The helper is included in mise: no compiler,
extra notifier, or logo download is needed. Denied permissions never block sync.
Alerts name the conflicting file, explain that local saves still work, and
point to `mise bootstrap dotfiles status` for resolution steps.

**Applying.** `mise bootstrap dotfiles pull` writes pending changes as one recoverable
transaction (a protective checkpoint first, each file journaled, reload
hooks afterwards, `mise bootstrap dotfiles undo` to reverse it). Any conflict
pauses publishing and incoming application for the entire setup. Local
commits and fetching continue. Invalid incoming
configuration or unsafe local files block the complete application batch.
Status and doctor name the blocking paths and the last successful application.
Choose per file with `--take-remote <path>` or `--keep-local <path>`; choices
are recorded without partially applying the setup. Once every conflict is
resolved, mise recomputes the plan before sharing resumes. A later local or
remote edit invalidates a choice based on an older version. This is
all-or-nothing application with recovery, not atomic filesystem writes.

The per-file choices apply to active tracked files. Conflicting enrollment or
encryption policies, files belonging to an inactive platform variant, and
histories with multiple merge bases require reconciliation with Git in a
separate checkout. Status reports the repository-level failure instead of
offering a native-file choice that cannot resolve it. Sharing stays paused;
mise never force-pushes or discards either history to get past the conflict.

There are no separate machine-recovery refs or backup uploads. Git is the
durable history; every pushed commit carries the normal Git author identity.

**Private repositories.** Network commands run with your normal git
configuration (credential helpers, ssh, URL rewrites). For a private GitHub
repository the recommended path is the GitHub CLI through mise; `gh auth
setup-git` writes the helper into `~/.gitconfig`, so pin `gh` globally to
keep that path valid for the watcher's service environment:

```sh
curl https://mise.run | sh
export PATH="$HOME/.local/bin:$PATH"
mise use -g gh
mise x gh -- gh auth login --hostname github.com --git-protocol https --web
mise x gh -- gh auth setup-git --hostname github.com
mise bootstrap dotfiles origin set https://github.com/you/setup.git
```

Existing working credentials skip the two `gh` steps. SSH remotes work when
the service environment can reach an agent or an unencrypted key.

## Explicit tracking and exclusions

```toml
[dotfiles]
"~/.zshrc" = { mode = "track" }
"~/templates" = { mode = "track" }
"~/.gitconfig" = { mode = "template", source = "~/templates/gitconfig.tera" }
```

Only `.zshrc` and the template directory are tracked here. Rendering
`.gitconfig` does not enroll it. A tracking directory may contain managed
files or sources without taking over their deployment declarations.

Enrollment accepts files and directories, not globs. `[history] exclude`
accepts glob patterns; a later `!glob` can reverse an earlier exclusion.
`mise bootstrap dotfiles paths` lists enrolled paths and capture omissions.

Use `autosave = false` for a configuration file you want to save manually.
Logs, caches, databases, and frequently rewritten application state are usually
better left untracked. Exclusions stop capture, not merely publication.

To recreate tools, services, and templates on another machine, explicitly
track the relevant mise configuration and template sources as well. Bootstrap
reports missing prerequisites instead of silently enrolling them.

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

## Encrypted shared files

Set `encrypt = true` on an explicitly tracked file or directory:

```toml
[history.encryption]
recipients = ["<age-or-plugin-public-recipient>", "<recovery-public-recipient>"]

[dotfiles]
"~/.config/app/credentials" = { mode = "track", encrypt = true }
"~/templates/private" = { mode = "track", encrypt = true }
"~/.config/app/config" = { mode = "template", source = "~/templates/private/app.tera" }
```

Contents are encrypted before they enter the repository's object database,
not just before upload. Live files and rendered outputs remain native files.
Missing keys or recipients fail safely; plaintext is never a fallback.

Every commit reachable from a proposed push is checked for encrypted-path
policy violations, including intermediate commits and merge parents. Earlier
plaintext blocks publication even if the latest tree is encrypted. mise does
not rewrite that history automatically.

Keep private decryption keys outside tracking. Configure local identities with
`settings.age.identity_files`, `settings.age.key_file`, or the supported SSH
identity settings. Public recipients travel with the repository.
