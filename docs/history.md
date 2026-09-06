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
  `mise bootstrap dotfiles history describe <ref> "…"` replaces the description,
  and so can a command of yours (below).
- **The trigger**: `save`, `baseline` (a newly tracked path), or the two halves
  of an operation: `bootstrap-before` (the protective checkpoint taken before
  a bootstrap command changes anything) and `bootstrap` (the outcome, with a
  journal of every path the run touched and its prior state).

`mise bootstrap dotfiles history show <ref>` prints all of it; `--files` lists the snapshot,
`--json` gives the record.

### Descriptions from an agent

`settings.history.describe_command` names a command that describes the
checkpoints the watcher saves. It gets one JSON object on stdin, `uuid`,
`trigger`, the computed `description`, the changed paths that are not
private (`added`, `modified`, `removed`), and `diff`, a unified diff of the
changed files that are backed up (at most 64 KiB, `diff_truncated` says
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
(`*.local.toml`, a credential store) is never named, and a file tracked with
`backup = false` never has its contents sent.

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
mise bootstrap dotfiles undo                                         # reverse the newest rollback, undo, or pull
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
snapshots do not record them. An explicitly selected empty folder can still
be removed, and undo restores it.

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

`mise bootstrap dotfiles undo` restores exactly the paths an operation touched from the
protective checkpoint it took, leaving everything else as it is now, so
unrelated work done since is preserved. That includes an operation that
failed midway (only the paths it did change are reversed), a type change it
forced, and an empty directory it replaced. It refuses when that checkpoint
was pruned. Undoing an undo re-applies the operation; an undo that changed
nothing does not count as having reversed it.

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
A busy file never delays an ordinary one. Explicit saves and the protective
checkpoints before a bootstrap, rollback, or undo always read every file
live.

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

The watcher never notifies you. It persists its health (`health.json` in
the history store) and two commands read it, without starting a sync,
applying anything, or prompting:

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

One setup repository holds the shared setup and every machine's recovery
refs. Connect it once per machine; nothing else is ever done with git by
hand ([Set up a machine](/bootstrap/setup.html) walks through connecting,
editing, resolving a conflict, and setting up the next machine):

```sh
mise bootstrap dotfiles origin set https://github.com/you/setup.git --name laptop
mise bootstrap dotfiles status                  # publication, fetch, pending changes, conflicts
mise bootstrap dotfiles sync                    # publish and fetch now (the watcher does this on its own)
mise bootstrap dotfiles pull                    # write incoming shared changes
mise bootstrap dotfiles machines                # every machine with recovery refs
```

`pull` and `apply` are different commands on purpose: `mise bootstrap
dotfiles apply` keeps deploying your own `[dotfiles]` declarations (symlinks,
copies, templates, edits), while `pull` writes what other machines shared
through the setup repository.

`origin set` prints exactly what will happen before anything leaves the
machine and asks for confirmation (`--yes` skips it): the sync mode, what is
published per stream, what is not shared and why, what is backed up (in
plain form: anyone who can read the repository can read every file in
those snapshots; the setup branch is always plaintext, so use a private
repository), whether existing checkpoints are included (only new ones by
default; `--include-existing`), names that look like secrets with the
`track … --no-share --no-backup` line for each, and private content already
committed in the repository's history, which stops the connection unless
`--allow-committed-private` is passed (rewriting history is your decision).

**What leaves the machine, in plain text.** Two things, both readable by
anyone who can read the repository: the setup branch (the shared
configuration, the sources it references, and the shared version of every
tracked entry, per its policies) and this machine's recovery refs
(`refs/mise-history/<machine>/…`: a snapshot of every tracked file with
`backup = true`, with private paths and paths with `backup = false` removed
from the snapshot, the metadata, and the descriptions). `share = false`
keeps a file out of the setup branch and out of other machines;
`backup = false` keeps it out of the recovery refs; `*.local.toml` and
credential stores are both unless a per-file declaration says otherwise.
Everything else stays on this machine. Encrypted recovery refs are not
implemented yet: `--encrypt-backups` and `encrypt_backups = true` are refused
rather than silently uploading in plain text, so a file you would only back
up encrypted is a file to track with `--no-backup` for now.
The declaration goes to `[history.origin]` in `config.local.toml` next to the
global config (machine-local, never published: each machine names the
repository the way it reaches it, and a fresh machine's own declaration
never conflicts with the configuration it pulls), the mode
to `settings.history.sync`.

**What is synchronized.** The setup branch mirrors the global configuration
directory at its root (`config.toml`, `conf.d/`, `tasks/`, templates),
publishes `[dotfiles]` sources under `sources/dotfiles/…` (relative to
`dotfiles.root`, resolved through the other machine's root) and
`sources/home/…`, and holds the shared version of every tracked entry under
`tracked/home/…` (a variant stream under `tracked/home@<variant>/…`). A
physical path maps to exactly one branch path, so the same content is
published once. Never in it: `*.local.toml`, credential stores, entries with
`share = false`, rendered or copied outputs, machine state, sources outside
`$HOME` (reported as not portable). Files that belong to the repository as a
repository (a README, a license, `.github/`, `.gitignore`) are neither
published from the configuration directory nor written into it: they stay in
git. A history-enabled repository carries
`.mise-history/format.toml`; a repository without it is an ordinary
repository (its `--from`/`--from-git` behaviour is unchanged) until you
confirm its adoption, and a newer format stops with an upgrade message.

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

**How a sync decides.** For every path the saved version here (ours is
always the saved version, never a live edit in progress), the fetched
upstream version, and the recorded acknowledged/reconciled/applied versions
run through one table: a local change publishes, an upstream change is
applied, both changed cleanly merges (relative to the acknowledged base, so a
stale local file is never published as a reversal), and a clash is a
conflict to decide: the same lines, delete/modify, a type change, a binary
file, a file that exists on both sides with no common base (needs adoption),
or unsaved edits of a manual-save entry. Publication is a commit built in
mise's own bare repository and pushed with a lease; a rejection fetches and
retries; nothing is ever force-pushed or reset, so your own commits and
unrelated files survive. Repeating a sync changes nothing.

**Modes** (`settings.history.sync`) say what the watcher does on its own:

- `sync` (the default, recommended): after a save the watcher publishes
  within `history.sync_interval` (5 minutes); every `history.fetch_interval`
  (15 minutes) it fetches and applies nonconflicting incoming changes, with
  a protective checkpoint first. A conflict or an unsaved edit pauses
  publication and incoming application for the complete setup.
- `fetch-only`: the watcher fetches; nothing is ever published and no live
  file changes until you run `mise bootstrap dotfiles pull`.
- `manual`: no automatic network activity at all.

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

**Conflict notifications** are off by default. With
`settings.history.notify = true`, a desktop notification (`notify-send` on
Linux, `osascript` on macOS) names each conflict once, the first time it
needs a decision; retries of the same sync stay silent, and a notifier that
is missing or failing never holds up history or sync.

**Applying.** `mise bootstrap dotfiles pull` writes pending changes as one recoverable
transaction (a protective checkpoint first, each file journaled, reload
hooks afterwards, `mise bootstrap dotfiles undo` to reverse it). Any conflict
pauses publishing and incoming application for the entire setup. Local
history, fetching, and eligible recovery backups continue. Invalid incoming
configuration or unsafe local files block the complete application batch.
Status and doctor name the blocking paths and the last successful application.
Choose per file with `--take-remote <path>` or `--keep-local <path>`; choices
are recorded without partially applying the setup. Once every conflict is
resolved, mise recomputes the plan before sharing resumes. A later local or
remote edit invalidates a choice based on an older version. This is
all-or-nothing application with recovery, not atomic filesystem writes.

**Machine backups.** Eligible checkpoints (with content and at least one
`backup = true` entry) are pushed as parentless commits to
`refs/mise-history/<machine-id>/<uuid>`, rebuilt without every
`backup = false` and private path and with the record masked; journal blobs
never travel. Retention removes only this machine's remote refs for
checkpoints it pruned. `mise bootstrap dotfiles rollback --to <machine>/<ref> --all`
recovers another machine's backed-up files here; their journals are data
only, never replayed. `mise bootstrap dotfiles origin --purge` deletes this machine's
refs from the origin (objects may persist until the host runs gc; setup
commits are never deleted; forks and host backups may keep content: not
erasure) and disconnects; `--remove` only disconnects.

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

Sharing and recovery backups follow these policies when you connect an origin.

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
