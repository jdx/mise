---
description: "Look up every [dotfiles] and [history] key, the keys each mode accepts, pattern and variant rules, status states, and what history saves."
socialDescription: "Look up every [dotfiles] and [history] key, pattern rule, status state, and state file."
---

# Dotfiles reference

Look up `[dotfiles]` entry keys, the keys each mode accepts, variant
selectors, pattern syntax, status states, and the state that mise keeps for
dotfiles and their history. The second half is the history reference: what a
checkpoint saves and leaves out, recovery, the watcher, and the
[`[history]` table and history settings](#configuration-reference). The keys
of `[dotfile_groups]` are in [Groups](/dotfiles/groups.html#group-keys). To
save, compare, and restore versions day to day, start with
[Dotfiles history](/dotfiles/history.html).

## Entry keys {#entry-keys}

A whole-file entry is keyed by its target path, absolute or starting with
`~/`. A string value is shorthand for `{ source = "<value>" }`.

| Key                 | Type            | Meaning                                                                                                                                                                                                                           |
| ------------------- | --------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `source`            | path            | The file or directory to deploy. Relative paths start at the declaring config file's directory. Defaults to the target's path under [`dotfiles.root`](/configuration/settings.html#dotfiles.root).                                |
| `mode`              | string          | `symlink`, `symlink-each`, `copy`, `template`, `absent`, `fnox`, `track`, or `track-local`. Defaults to [`dotfiles.default_mode`](/configuration/settings.html#dotfiles.default_mode). See [Modes](/dotfiles/managed.html#modes). |
| `content`           | string          | The whole file, written inline. See [Inline content](/dotfiles/managed.html#inline-content).                                                                                                                                      |
| `permissions`       | octal string    | Permissions for the target, such as `"0600"`. On its own, manages an existing path's permissions only. See [Permissions](/dotfiles/managed.html#permissions).                                                                     |
| `exclude`           | list of globs   | Paths to skip in a walked source directory or a tracked directory. See [Pattern syntax](#patterns).                                                                                                                               |
| `manifest`          | `"git"`         | Deploy only files in the source repository's Git index. See [Deploy only files committed to Git](/dotfiles/managed.html#git-tracked-directories).                                                                                 |
| `dot_prefix`        | boolean         | Deploy source names `dot-<name>` as `.<name>`. See [Visible source names](/dotfiles/managed.html#dot-prefix).                                                                                                                     |
| `relative`          | boolean         | `symlink` and `symlink-each`: link by a relative path. Overrides [`dotfiles.relative_symlinks`](/configuration/settings.html#dotfiles.relative_symlinks).                                                                         |
| `main_checkout`     | boolean         | `symlink` and `symlink-each`: link a source inside a linked Git worktree at the same path in the main checkout. See [Links from a Git worktree](/dotfiles/managed.html#main-checkout).                                            |
| `remove_empty`      | boolean         | Remove the target when the template renders empty. See [Remove the target when a template renders empty](/dotfiles/managed.html#remove-empty).                                                                                    |
| `create_if_missing` | boolean         | Write the target only when nothing exists there; an existing file is left untouched. See [Create a file only if it is missing](/dotfiles/managed.html#create-if-missing).                                                         |
| `variants`          | list of tables  | Per-machine selection: a target per machine for deployed entries, separate contents for tracked ones. See [Variant selectors](#variant-selectors).                                                                                |
| `depends`           | list of strings | Groups (by name) and whole-file entries (by target path) mise writes before it renders or reads this entry. See [Wait for other entries](/dotfiles/managed.html#depends). Not for edits or track modes.                           |
| `enabled`           | boolean         | `false` turns off an entry inherited from another config file.                                                                                                                                                                    |
| `include`           | list of globs   | Track only: the files a tracked directory saves. See [Select files in a directory](/dotfiles/history.html#choose-which-files-a-directory-saves).                                                                                  |
| `autosave`          | boolean         | Track only: `false` saves the path only on `mise dot save <path>`. See [Save some files only on request](/dotfiles/history.html#save-on-request).                                                                                 |
| `encrypt`           | boolean         | Track only: encrypt contents before saving them to Git. See [Encrypted files](/dotfiles/encryption.html).                                                                                                                         |
| `allow_plaintext`   | boolean         | Track only: save a credential-named file that you track directly in plaintext. See [Credential filter](#credential-filtering-and-omissions).                                                                                      |

A `[dotfiles]` entry takes no `group` key; declare a group's entries under
[`[dotfile_groups.<name>.entries]`](/dotfiles/groups.html#group-entries).

## Edit keys {#edit-keys}

An edit entry is keyed by the target path followed by `/<id>`, such as
`"~/.zshrc/aliases"`. Ids can contain letters, digits, `_`, `-`, and `.`. See
[Edit part of a file](/dotfiles/edits.html).

| Key           | Type                                                | Meaning                                                                                                                                                                                                                             |
| ------------- | --------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `block`       | string                                              | Inline content to keep between marker comments.                                                                                                                                                                                     |
| `source`      | path                                                | A file with the block content (with `template = "tera"`) or the keys to merge (with `merge`).                                                                                                                                       |
| `template`    | `"tera"`                                            | Render `source` with Tera first.                                                                                                                                                                                                    |
| `comment`     | string                                              | The marker comment prefix. Defaults to one inferred from the file extension.                                                                                                                                                        |
| `line`        | string                                              | One exact line to ensure exists.                                                                                                                                                                                                    |
| `position`    | `"append"` or `"prepend"`                           | Where a missing `line` goes. Defaults to `"append"`.                                                                                                                                                                                |
| `merge`       | `true` or `"missing"`                               | Set the keys in `source` on a `.json`, `.jsonc`, `.toml`, `.yaml`, `.yml`, or `.ini` target. `"missing"` sets only keys the target has no value for; see [Defaults the application may change](/dotfiles/edits.html#merge-missing). |
| `format`      | `"json"`, `"jsonc"`, `"toml"`, `"yaml"`, or `"ini"` | With `merge`, the format when the target's extension doesn't name it, such as `"jsonc"` for VS Code's `settings.json`.                                                                                                              |
| `permissions` | octal string                                        | `merge` only: the mode of the target when apply creates it; an existing file keeps its mode. See [Set permissions when mise creates the file](/dotfiles/edits.html#merge-permissions).                                              |

`block` and `source` are mutually exclusive, and neither combines with
`line`. `position` applies only to `line`, and `format` and `permissions`
only to `merge`. A `merge` entry without `source` reads the target's path
under [`dotfiles.root`](/configuration/settings.html#dotfiles.root), and takes
no `block`, `line`, `position`, or `comment`. Edits are never encrypted.

## Modes and keys {#modes-and-keys}

The table shows which whole-file keys each kind of entry accepts. An entry
with `content` is an inline-content entry, and one with only `permissions`
(no `source`, `content`, or `mode`) is a permissions-only entry. Every
whole-file entry also accepts `enabled`; block, line, and merge edits do not,
and mise warns and ignores an edit that sets it. Every kind except `track` and
`track-local` accepts `depends`.

| Entry            | Accepts                                                                                                                                                             | Accepted, no effect              |
| ---------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------- |
| `symlink`        | `source`, `relative`, `main_checkout`, `variants` (selectors and `target`)                                                                                          | `exclude`, `autosave`, `encrypt` |
| `symlink-each`   | `source` (a directory), `exclude`, `manifest`, `dot_prefix`, `relative`, `main_checkout`, `variants` (selectors and `target`)                                       | `autosave`, `encrypt`            |
| `copy`           | `source`, `variants` (selectors and `target`); with a directory source, `exclude`, `manifest`, `dot_prefix`; with a file source, `permissions`, `create_if_missing` | `autosave`, `encrypt`            |
| `template`       | `source` (a file), `permissions`, `remove_empty`, `create_if_missing`, `variants` (selectors and `target`)                                                          | `exclude`, `autosave`, `encrypt` |
| `content`        | `content`, `permissions`, `create_if_missing`, `variants` (selectors only)                                                                                          | `autosave`                       |
| Permissions only | `permissions`, `variants` (selectors and `target`)                                                                                                                  | `autosave`                       |
| `absent`         | `variants` (selectors and `target`)                                                                                                                                 | `autosave`                       |
| `fnox`           | `permissions`, `variants` (selectors and `target`)                                                                                                                  | `autosave`                       |
| `track`          | `exclude`, `include`, `autosave`, `encrypt`, `allow_plaintext`, `variants` (selectors only)                                                                         |                                  |
| `track-local`    | `exclude`, `include`, `autosave`, `allow_plaintext`                                                                                                                 |                                  |

For `encrypt`, `relative`, and `main_checkout`, the table means the `true`
value; `encrypt = false`, `relative = false`, and `main_checkout = false` are
accepted everywhere except that permissions-only and `track-local` entries
reject any `encrypt` key, including `encrypt = false`. See [Local-only history](/dotfiles/history.html#local-only)
for `track-local`.

mise warns about and ignores an entry that sets one of the
[entry keys](#entry-keys) its kind does not accept; for a `track` entry,
`mise dot paths` also lists it as invalid. A key that is not an entry or edit
key at all is ignored with an `unknown key` warning, and a pull refuses to
apply configuration from another machine that has one. `permissions` with a
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
- mise reads tracking entries only from system and global config, like the
  [`[history]` table](#configuration-reference).
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

A variant with both `os` and `profile` matches only when both do. A `profile`
matches when its name is anywhere in the selected list, so
`MISE_ENV=ssh,work` matches `profile = "work"` and `profile = "ssh"`. When several
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

[`mise dot status`](/cli/dotfiles/status.html) prints a row for each entry, edit,
and orphaned group file that needs attention, and counts the ones that are
applied or tracked; `--verbose` lists them all, and naming targets lists those
targets. Sections follow for the history state: Tracking (entries, files, the
latest checkpoint, unsaved changes), Repository (the origin, branch, mode, and
sync times), Conflicts, Automatic capture, and Bootstrap. A Next steps list
names the commands that clear whatever needs attention.

| State                                              | Meaning                                                                                      | What to do                                                            |
| -------------------------------------------------- | -------------------------------------------------------------------------------------------- | --------------------------------------------------------------------- |
| `applied`                                          | The target matches its entry.                                                                | Nothing.                                                              |
| `applied (target absent; permissions not applied)` | A permissions-only target does not exist.                                                    | Nothing, or create the path.                                          |
| `applied (a regular file, not a symlink)`          | On Windows, a `symlink` entry whose target is a regular file, such as a fallback copy.       | Nothing; see [Windows](/dotfiles/managed.html#windows).               |
| `missing`                                          | The target or edit does not exist yet.                                                       | `mise dot apply`                                                      |
| `differs (<reason>)`                               | The target exists but its content, link, or permissions differ, or it cannot be checked.     | `mise dot diff`, then `mise dot apply`, with `--force` for a conflict |
| `source missing`                                   | The entry's source does not exist.                                                           | Create the source, or fix `source`.                                   |
| `tracked`                                          | A `mode = "track"` entry. `tracked (2 omitted, 1 nested)` counts files that saves leave out. | `mise dot paths` lists them.                                          |
| `not on this machine (variants: <names>)`          | A `track` entry whose variants all select other machines, such as an inactive profile.       | Nothing, or select a variant, such as with `-E <profile>`.            |
| `ambiguous variants (<names> match equally)`       | A `track` entry with two variants that match this machine equally, which skips it.           | Make one variant more specific.                                       |
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
  inline content, permissions-only, `absent`, and `fnox` entries. Use
  `origin.source` for the absolute path.
- `origin` describes where the entry came from: the config file, its
  `config_root`, any config environment in the file name, and the resolved
  source path. These paths are strings when they are valid UTF-8; on Unix, a
  path with other bytes is written as `mise:path-bytes:<base64url>`.
- An entry that sets `permissions` includes them as an octal string.
- A `differs` entry has a `reason`. An `absent` entry is `applied` once the
  target is gone and `differs` while it is there, with a reason that ends in
  `; will be removed`. A permissions-only entry whose target does not exist is
  `applied` with a `reason`. A `tracked` entry whose variants all select
  other machines, or tie, has a `reason` that names them.
- Each `edits` element has `path`, `edit` (such as `block:aliases`), `origin`,
  and `state`.
- Each `orphaned` element has `target`, `group`, and `state`, which is
  `orphaned` or `orphaned_changed`.

## State files {#state-files}

mise keeps records under [`MISE_STATE_DIR`](/directories.html)
(`~/.local/state/mise` by default). Keep them: they are how mise recognizes
what it wrote.

| Path                                           | Holds                                                                                                                                                                                                                                    |
| ---------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `$MISE_STATE_DIR/dotfiles/`                    | The links each `symlink-each` entry created                                                                                                                                                                                              |
| `$MISE_STATE_DIR/dotfiles/targets/`            | For each target, a digest of what mise last wrote there and the parent directories it created                                                                                                                                            |
| `$MISE_STATE_DIR/dotfiles/groups/`             | What each [group](/dotfiles/groups.html) deployed, so its files can be found after the group stops deploying them                                                                                                                        |
| `$MISE_STATE_DIR/history/repo.git`             | The history: a bare Git repository of checkpoints                                                                                                                                                                                        |
| `$MISE_STATE_DIR/history/`                     | The checkpoint index, which mise can rebuild from Git, operation journals and recovery copies, `health.json`, `watch-schedule.json`, and `machine`, the name generated for [`machine` variants](/dotfiles/history.html#machine-variants) |
| `$MISE_STATE_DIR/history-local/`               | The separate store for [local-only history](/dotfiles/history.html#local-only)                                                                                                                                                           |
| `~/.config/mise/config.local.toml`             | `[history.origin]`, written by `mise dot origin set`                                                                                                                                                                                     |
| `~/.config/mise/conf.d/dotfiles-tracking.toml` | Tracking entries for files that another `[dotfiles]` entry already manages                                                                                                                                                               |

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

A symlink is saved as a link. If a parent directory is a symlink, track that
link, and use the real directory path to track files beneath it. Your home
directory and mise config directory can themselves be symlinks; mise maps them
to the matching directories on each machine.

mise reports oversized files, special files, and unreadable paths that it
cannot save, and keeps their previous saved versions while it saves other
files, so check reported omissions before you rely on a checkpoint. Exclusions
remove paths from future checkpoints. An encryption failure stops a save
rather than storing plaintext.

Checkpoints restore file contents only. They do not restore installed
packages or the running state of a service; use your system's backup tools
for that, and run `mise bootstrap` to apply restored configuration.

### Credential filter {#credential-filtering-and-omissions}

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
credential, [encrypt it](/dotfiles/encryption.html). A populated
[`include` list](/dotfiles/history.html#choose-which-files-a-directory-saves)
selects credential-named files too.

When you track a file directly and its name looks like a credential store,
`mise dot track` asks whether to save it in plaintext. The default answer is
no, and neither `--yes` nor a piped answer approves it. For a noninteractive
command, including a script that pipes a file list into `mise dot track`, pass
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
shared history contains commit pointers, pull and adoption skip them instead
of restoring unavailable Git objects or removing an existing checkout.

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

If [`mise dot capture`](/dotfiles/history.html#capturing-an-external-command)
is killed, the next command that changes history recovers its pending
operation record. The wrapped command's writes are not journaled one by one.
If the wrapped command untracks a file, the after checkpoint leaves it out;
its earlier checkpoint stays available, and recovery keeps it untracked.

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
lock, then fails. [`history.lock_wait`](/configuration/settings.html#history.lock_wait)
changes the wait; `MISE_HISTORY_LOCK_WAIT=5m mise dot save` waits up to five
minutes, and `0` fails at once. `mise dot watch` and `mise dot recover` never
wait.

A `mise bootstrap` with nothing tracked and no existing history touches the
history store only when it first rewrites a dotfile or edit, to keep a private
journal for recovering interrupted writes. If another mise process holds the
lock at that point, it warns and continues without the journal instead of
failing. A run that starts with nothing to record but declares tracking before
it ends waits the same time at the end. If the lock is still busy, or an
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
5. Files at the base interval wait for each other: when several change
   together, mise saves them in one checkpoint once the last of them is quiet,
   at most 30 seconds after the first changed.
6. An automatic save that changes only files the newest checkpoint changed
   replaces that checkpoint instead of following it, when that checkpoint is
   also an automatic save, is less than five minutes old, and has not been
   synced. The replacement keeps its number and its start time. A change to
   what is tracked, an explicit save, or a sync starts a new checkpoint.

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

### Description command {#describe-command}

When [`history.describe_command`](/configuration/settings.html#history.describe_command)
is set, the watcher runs it to
[describe each checkpoint it saves](/dotfiles/history.html#descriptions-from-an-agent).
The command receives one JSON object on stdin:

| Field                          | Contents                                                    |
| ------------------------------ | ----------------------------------------------------------- |
| `uuid`                         | The checkpoint's UUID                                       |
| `trigger`                      | What triggered the checkpoint                               |
| `description`                  | The description mise computed                               |
| `added`, `modified`, `removed` | The paths that changed                                      |
| `diff`                         | A unified diff of changed unencrypted files, at most 64 KiB |
| `diff_truncated`               | `true` when the diff was cut at that limit                  |

Contents reach the command through stdin, never through shell interpolation.
For variant files, the diff compares the variant saved in each checkpoint.
This also applies to added and removed files inside a tracked variant
directory.

The command prints one line of at most 200 characters, which becomes the
checkpoint's description, recorded with `description_source: command`. The
checkpoint is saved before the command runs. If the command fails, prints
nothing, or runs longer than 30 seconds, the checkpoint keeps its computed
description. Commands run one at a time, once per checkpoint the watcher
saves; filesystem events and retries do not trigger extra runs.

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
To erase a path's saved versions, see
[Erase a file from history](/dotfiles/history.html#purge).

## History configuration {#configuration-reference}

History reads two tables. `[history]` holds what history saves and where it
goes, and mise reads it only from your global and system config, never from a
project. `[settings.history]` holds the [settings](/configuration/settings.html)
that control the watcher and sync, which you can also set with
`mise settings` or environment variables.

| Table                      | Keys                                                                                                                                              | Purpose                                                                                                                                                                     |
| -------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `[history]`                | `exclude`                                                                                                                                         | Globs that history never saves; see [Global exclude patterns](#global-exclude-patterns)                                                                                     |
| `[history]`                | `git_email`                                                                                                                                       | Author and committer email for history commits; see [Identify commits by machine](/dotfiles/sync.html#identify-commits-by-machine)                                          |
| `[history]`                | `git_name`                                                                                                                                        | Author and committer name for history commits; see [Identify commits by machine](/dotfiles/sync.html#identify-commits-by-machine)                                           |
| `[history]`                | `machine`                                                                                                                                         | This machine's name for [`machine` variants](/dotfiles/history.html#machine-variants)                                                                                       |
| `[history.reload]`         | `"<glob>" = "<command>"`                                                                                                                          | Commands to run after rollback, undo, pull, or apply writes matching files; see [Reload an application](/dotfiles/history.html#reload-an-application-after-restoring-files) |
| `[history.encryption]`     | `recipients`                                                                                                                                      | Public keys that encrypted files are encrypted to; see [Encrypted files](/dotfiles/encryption.html)                                                                         |
| `[history.origin]`         | `url`, `branch`                                                                                                                                   | The origin, written to `config.local.toml` by `mise dot origin set`; see [Sync across machines](/dotfiles/sync.html)                                                        |
| `[settings.history]`       | `enabled`, `sync`, `sync_interval`, `fetch_interval`, `notify`, `lock_wait`, `allow_plaintext_history`, `allow_secret_values`, `describe_command` | When history records and how it syncs                                                                                                                                       |
| `[settings.history.watch]` | `debounce`, `max_interval`, `reconcile`                                                                                                           | Watcher timing; see [Adaptive scheduling](#adaptive-scheduling) and [Scans and failures](#reconciliation-and-failures)                                                      |

Write settings as TOML tables, for example:

```toml
[settings.history]
sync = "manual"
notify = false
```

`allow_plaintext_history`, `allow_secret_values`, and `describe_command` are
read only from global or system config.

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="history" :level="3" />
