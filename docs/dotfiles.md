---
description: "Save the history of your configuration files, or use mise to copy, link, and generate them."
---

# Dotfiles

Dotfiles are configuration files such as `~/.zshrc` and `~/.gitconfig`.
Keep editing them with your usual editor; mise can save changes in the
background so you can return to an earlier version. With synchronization
enabled on your machines, an edit on your laptop can reach your desktop,
and edits on the desktop flow back.

Start by [tracking a file you already use](#tracking-files-in-place). If you
want mise to copy or link files into place, start with
[one managed file](#start-with-one-managed-file).

For an introduction to this workflow, read
[Dotfiles That Save Themselves](https://jdx.dev/posts/2026-09-07-dotfiles-that-save-themselves/).

## Tracking files in place

Start saving the history of a file you already have. For example, if you
use zsh:

```sh
mise bootstrap dotfiles track ~/.zshrc
```

mise leaves the file where it is and saves a **checkpoint**, a version you
can inspect or restore later. It also adds this entry to your global mise
configuration (`~/.config/mise/config.toml` by default):

```toml
[dotfiles]
"~/.zshrc" = { mode = "track" }
```

### Save edits automatically

Add the watcher service to the same global configuration file:

```toml
[bootstrap.services.mise-history]
builtin = "history-watch"
```

Install and start it, then check that it is running:

```sh
mise bootstrap services apply
mise bootstrap dotfiles status
```

`status` shows the file as `tracked` and the watcher as running. Edit the
file normally from now on. The watcher saves changes to local Git history.
See [automatic saves](/history.html#automatic-saves) for service details.

To save by hand, skip the service and run
`mise bootstrap dotfiles save ~/.zshrc` after editing.

### Try restoring a change

Add an alias to `~/.zshrc` in your editor:

```sh
alias ll='ls -lah'
```

Save a checkpoint now so you can try restoring it without waiting for the
watcher, then inspect the file's history:

```sh
mise bootstrap dotfiles save ~/.zshrc
mise bootstrap dotfiles history --path ~/.zshrc
```

If that was your only edit, rolling back removes the new alias. Preview
the restore, then apply it:

```sh
mise bootstrap dotfiles rollback ~/.zshrc --dry-run
mise bootstrap dotfiles rollback ~/.zshrc
```

Rollback restores the latest saved version that differs from the current
file. It saves the current contents first, so you can reverse the rollback
with `mise bootstrap dotfiles undo`. See [rolling back](/history.html#rolling-back)
to choose a particular checkpoint.

### Share with another machine

History is stored locally in Git until you configure a remote repository,
called an **origin**. Connect your own repository and enable automatic
synchronization to share saved edits in both directions. The watcher needs
Git credentials on each machine. Follow
[sharing across machines](/history.html#sharing-across-machines) to connect
the repository, or [set up another machine](/bootstrap/setup.html) to bring
your configuration to a new computer.

Use a private repository: synchronization sends earlier checkpoints too,
so temporary edits can become part of the shared history. Configure
[encryption](/history.html#encrypted-shared-files) before first saving files
that need it.

## Start with one managed file

mise can also create configuration files from a **source** file that you
maintain. The **target** is the path where an application reads the configuration.

Create `dotfiles/example.conf` next to your `mise.toml` with this content:

```ini
enabled = true
```

Add the following to `mise.toml`. Choose an unused target path for this example:

```toml
[dotfiles]
"~/.config/mise-dotfiles-example.conf" = { source = "dotfiles/example.conf", mode = "copy" }
```

Preview the copy, apply it, and check the result:

```sh
mise bootstrap dotfiles apply --dry-run
mise bootstrap dotfiles apply
mise bootstrap dotfiles status
```

The target now contains `enabled = true`, and `status` reports it as `applied`.
For future changes, edit `dotfiles/example.conf` and run `apply` again.

> [!WARNING]
> In `copy` mode, applying overwrites changes made directly to the target.
> Use [`add` to save those changes back to the source](#capturing-changes)
> before applying again.

To start from an existing file, run `mise bootstrap dotfiles add <target>`.
This saves the file under `dotfiles.root` (`~/.dotfiles` by default), adds a
configuration entry, and applies it using `dotfiles.default_mode`. That
setting defaults to `symlink`, which makes the original path a link to the
saved source. Select another mode with `--mode`, such as `--mode copy` to
keep a regular file at the target. Use `--no-apply` to review the source
and configuration before changing the original file.

`apply` also runs as part of [`mise bootstrap`](/bootstrap.html), with the
configured `pre-dotfiles` and `post-dotfiles` hooks. `mise install` and
`mise bootstrap packages` leave dotfiles alone.

To save history for a source or target you manage this way, [track that
path](#tracking-files-in-place) too.

## Modes

Choose how mise creates a target from its source:

| Mode           | What happens when you apply                                       | Use when                                                              |
| -------------- | ----------------------------------------------------------------- | --------------------------------------------------------------------- |
| `symlink`      | Create one link to a file or entire directory; the default.       | Edits through the target should change the source.                    |
| `symlink-each` | Create directories and link each file within them.                | The target directory also holds files you want mise to leave alone.   |
| `copy`         | Copy a file or directory, overwriting matching files.             | The application needs a regular file or writes its own configuration. |
| `template`     | Render a source file with the [template engine](/templates.html). | The output depends on machine-specific variables.                     |

For example, to link a directory:

```toml
[dotfiles]
"~/.config/nvim" = { source = "dotfiles/nvim", mode = "symlink" }
```

`symlink-each` requires a directory source. When you delete or exclude a
source file, the next apply removes its link. Other files in the target
directory stay in place. mise records these links under
`$MISE_STATE_DIR/dotfiles`; keep that directory so later applies can update
or remove previously created links.

Directory copies keep existing target files when you delete or exclude their
sources. Review and remove those leftover copies yourself.

### Platform-specific destinations

Use `variants` to deploy one source to different paths on different machines:

```toml
[dotfiles."vscode/settings.json"]
source = "dotfiles/vscode/settings.json"
mode = "copy"
variants = [
  { os = "macos", target = "~/Library/Application Support/Code/User/settings.json" },
  { os = "linux", target = "~/.config/Code/User/settings.json" },
  { os = "windows", target = "~/AppData/Roaming/Code/User/settings.json" },
]
```

Destination variants work with `copy`, `symlink`, `symlink-each`, and
`template`. They share the [tracking variant selectors](#variants): `os`
(optionally with an architecture), `profile` (a mise environment selected
with `-E` or `MISE_ENV`), and `default = true`. The most specific matching
variant wins; ties are reported as invalid, and no match without a default
skips the entry.

A variant's `target` overrides the table key. When every variant supplies a
`target`, the key can be a logical name, as above. Otherwise the key must
be an absolute or home-relative target path. Every destination must be
absolute or start with `~/`. Target overrides require an explicit `source`,
which stays the same across machines and resolves relative to the config
file. Omitting `target` uses the table key:

```toml
[dotfiles."~/.config/example/settings.json"]
source = "dotfiles/example/settings.json"
mode = "symlink"
variants = [
  { profile = "work", target = "~/.config/example-work/settings.json" },
  { default = true },
]
```

A later configuration file can replace a destination-variant declaration by
using the same key, even when it selects a different destination.

Commands such as `status`, `diff`, `apply`, and `unapply` use the selected
destination. Changing the selected destination does not remove a file
previously deployed elsewhere. Tracking variants continue to select separate
history streams at the same path and do not accept `target` overrides.

### Templates

```toml
[dotfiles]
"~/.gitconfig" = { source = "dotfiles/gitconfig.tera", mode = "template" }
```

Templates can use `env`, `vars`, `exec()`, and the rest of the
[template context](/templates.html). Applying a template writes its rendered
content and gives the target the source file's permissions. A later apply
also repairs changed permissions.

`status`, `diff`, and `apply` render templates to check their output. This
executes any `exec()` calls in those templates, using your trusted config.
With `--dry-run`, mise skips rendering dotfile templates and labels them
`(if changed)`. Other configuration expressions can still run during a dry
run, so use it with trusted configuration.

See [Windows](#windows) for differences in link behavior on that platform.

## Whole-file entries

Each entry in `[dotfiles]` uses the target path as its key. Use an absolute
path or one starting with `~/`. A source can be a file or a directory.

When you omit `source`, mise looks under `dotfiles.root`, which defaults to
`~/.dotfiles`. It uses the same path relative to your home directory:
`~/.zshrc` gets its source from `~/.dotfiles/.zshrc`, and
`~/.config/foo.toml` gets it from `~/.dotfiles/.config/foo.toml`.
For targets outside your home directory, specify `source` or inline `content`.

These entries use an inferred source and an explicit source, respectively:

```toml
[dotfiles]
"~/.zshrc" = { mode = "symlink" }
"~/.ssh/config" = { source = "ssh/config", mode = "copy" }
```

Relative source paths start from the directory containing the configuration
file. For example, `source = "ssh/config"` in
`~/.config/mise/config.toml` refers to `~/.config/mise/ssh/config`.

You can also write a source as a string, such as
`"~/.config/nvim" = "dotfiles/nvim"`. This uses `dotfiles.default_mode`.
When `add` writes an entry, it leaves out the source if it can infer it, and
leaves out the built-in `symlink` mode. A mode you select with `--mode` is
always written explicitly.

### Inline content

Use `content` to declare a literal whole file inline instead of keeping a
separate source file. On Unix, the resulting file has permissions `0600`,
so only its owner can read and write it:

```toml
[dotfiles]
"~/.config/example.conf" = { content = "enabled = true\n" }
```

Use `content` on its own. It cannot be combined with `source`, `mode`,
`exclude`, `manifest`, or the edit options `block`, `line`, `template`, and
`comment`.

### Matching multiple source files

Source paths may contain glob wildcards like `*`, `**`, `?`, or `[ab]`.
When a wildcard source matches multiple paths, the target path must contain
matching wildcards so each source expands to a unique target:

```toml
[dotfiles]
"~/.config/*.toml" = "dotfiles/config/*.toml"
"~/.local/share/app/**/*.json" = { source = "dotfiles/app/**/*.json", mode = "copy" }
"~/.config/app?.toml" = "dotfiles/config/app?.toml"
"~/.config/theme-[ab].toml" = "dotfiles/config/theme-[ab].toml"
```

## Excluding files

Modes that walk a source directory — `symlink-each`, and `copy` with a
directory source — take an `exclude` list of glob patterns. This is the way
to point an entry at a directory you don't fully own, such as the one holding
`mise.toml` itself:

```toml
[dotfiles]
"~" = { source = ".", mode = "symlink-each", exclude = ["mise.toml", "*.md", ".git"] }
```

A pattern without `/` matches any single path component, so `"mise.toml"`
skips that file wherever it appears in the tree and `"*.md"` skips every
markdown file. A pattern containing `/` is anchored to the source root:
`"nvim/spell"` skips only that path. Either kind matching a directory skips
everything under it.

For `symlink-each`, excluding a previously managed file removes its recorded link on the
next apply, just as deleting the source would. Directory `copy` is additive: exclusions
prevent future copying but leave existing target files in place.

## Git-tracked directories

Set `manifest = "git"` on a directory-walking entry to manage only files in
Git's index. This supports repositories that use `gitignore *` and opt files
in with `git add -f`, without listing every path again in mise:

```toml
[dotfiles]
"~" = { source = ".", mode = "symlink-each", manifest = "git" }
```

mise runs `git ls-files` from the source directory. Ignored and untracked
files are left alone, while removing a file from the index removes a
mise-owned `symlink-each` link on the next apply. `exclude` can be combined
with the Git manifest for an additional filter. Git manifests require a
directory source and either `symlink-each` or `copy` mode.

When environment-specific configs select different `symlink-each` sources for
the same target, applying the new environment reconciles links recorded for
the previous source. This makes `mise bootstrap -E home` and
`mise bootstrap -E work` usable as profile switches: links unique to the old
profile are removed, shared paths are repointed, and unmanaged neighbors are
preserved.

## Edit entries

Edit entries manage one piece of a file: the `mise activate` block in your
shell rc, an entry in `/etc/hosts`, or a small snippet in a config file.
They are keyed by target path plus an id naming each edit within the file:

```toml
[dotfiles]
"~/.zshrc/activate" = { block = 'eval "$(mise activate zsh)"' }
"~/.zshrc/aliases" = { block = '''
alias ll='ls -l'
alias la='ls -la'
''' }
"/etc/hosts/dev" = { line = "127.0.0.1 dev.local" }
"/etc/zshrc/zdotdir" = { line = 'ZDOTDIR=$HOME/.config/zsh/', position = "prepend" }
"~/.gitconfig/identity" = { source = "snippets/git-identity.tmpl", template = "tera" }
```

For edit entries, `source` is paired with `template = "tera"` to make the
entry unambiguously an edit. A table with only `source` is a whole-file
entry using `dotfiles.default_mode`.

A `block` is delimited by marker comments in the target file, named by the
entry's id:

```sh
# >>> mise:activate >>> managed by mise - do not edit between markers
eval "$(mise activate zsh)"
# <<< mise:activate <<<
```

Applying replaces the content between the markers. If the block is missing,
mise appends it. Everything else in the file stays as it is.

Ids may contain letters, digits, `_`, `-`, and `.`. The marker comment
prefix is inferred from the file extension (`#` for shell/config files,
`--` for Lua, `//` for C-like languages, `;` for INI, `"` for vim) and can
be overridden with `comment = "..."`. Files that can't hold line comments
at all (strict JSON, XML) aren't a fit for blocks — use a whole-file entry
instead.

A `line` inserts the given text if that exact line is missing. By default,
mise appends it; set `position = "prepend"` to insert it at the beginning.
Running apply again leaves an existing match wherever it is. Other bytes,
including line endings, stay unchanged. The value must be a single line;
use a block for multi-line content.

## How configuration is applied {#semantics}

- Entries merge across the [config hierarchy](/configuration.html).
  Whole-file entries merge by target path; edit entries merge by `(path, id)`.
  Tracking uses only system and global configuration.
- `mise bootstrap dotfiles add` applies the entries it captures unless you
  pass `--no-apply`. Use `mise bootstrap dotfiles apply` or
  [`mise bootstrap`](/bootstrap.html) to apply the rest.
- Applying skips targets that already match. Templates may still execute
  while mise checks their output. Copy and template entries overwrite
  changed targets.
- mise warns and skips entries with unknown modes or operations.

## Conflicts

For symlink entries, mise refuses to replace conflicting existing paths: a real file or
directory where a symlink should go, or a directory where a file should go,
is an error listing the conflicting paths. Pass
`mise bootstrap dotfiles apply --force` to replace them.

Replacing a real file or directory with a symlink requires `--force`, even
when its contents and permissions match the source. To adopt an existing
file, use `mise bootstrap dotfiles add`: it moves the file to its source
path before creating the link. When the source is on another filesystem,
mise copies it while preserving symlinks and permissions.

A `copy` or `template` entry overwrites the target's content without
`--force`. Existing symlinks can also be repointed. Inspect the diff before
changing which source a target uses.

Blocks and lines can be applied without `--force`. mise reports an error
if a block's markers are corrupted or an edit's target is a symlink.
For a symlink, point the edit at the real file you want to change.

Removing an entry from config leaves its file, block, or line in place.
To remove them too, run `mise bootstrap dotfiles unapply` before deleting
the entry from your config.

## Unapplying

`mise bootstrap dotfiles unapply` removes configured targets without removing
their `[dotfiles]` entries or source files. It uses the current config,
filesystem, and recorded `symlink-each` state to determine what the entry owns:

- `symlink` targets are removed only while they still point to the configured
  source.
- `symlink-each` removes exact source-to-target links, including dangling links
  for deleted source files. Other links and files under the target survive.
- File copies and rendered templates are removed only while their content still
  matches. Modified targets require `--force`.
- Directory copies are removed file by file. Unmanaged neighbors always
  survive, and directories are removed only when empty.
- Marker-delimited blocks are removed with their markers. Plain line edits have
  no ownership marker and require `--force`.

If you deleted a source file from a copied directory, unapply cannot
identify its old copy. Remove that leftover file yourself. Use `--dry-run`
to preview removals first. Template dry-runs skip rendering and template
function calls.

## Commands

```sh
mise bootstrap dotfiles status            # show tracked files and the state of managed files
mise bootstrap dotfiles status --missing  # exit 1 if anything is out of sync
mise bootstrap dotfiles diff              # show changes needed to apply
mise bootstrap dotfiles diff ~/.zshrc     # show changes for one target

mise bootstrap dotfiles apply                     # apply files and edits
mise bootstrap dotfiles apply --dry-run           # print what would be done
mise bootstrap dotfiles apply --dry-run --verbose # include diff-like details
mise bootstrap dotfiles apply --yes               # skip the confirmation prompt
mise bootstrap dotfiles apply --force             # also replace conflicting files

mise bootstrap dotfiles unapply             # remove identifiable managed targets
mise bootstrap dotfiles unapply --dry-run   # preview removals
mise bootstrap dotfiles unapply --force     # also remove modified/ambiguous targets

mise bootstrap dotfiles track ~/.zshrc     # track a live file where it is
mise bootstrap dotfiles untrack ~/.zshrc   # stop tracking it; the file stays
mise bootstrap dotfiles add ~/.zshrc       # capture a live file into dotfiles.root
mise bootstrap dotfiles add --changed      # capture all changed copy-mode files
mise bootstrap dotfiles edit ~/.zshrc      # edit the managed source or owning config
mise bootstrap dotfiles edit --apply ~/.zshrc

mise bootstrap dotfiles save                    # checkpoint the tracked files now
mise bootstrap dotfiles history                 # browse checkpoints; `history show`, `history diff`
mise bootstrap dotfiles paths                   # list tracked paths and their save settings
```

`mise bootstrap dotfiles status` reports each entry as `tracked`, `applied`,
`missing`, `differs` with a reason, or `source missing`, followed by the
history state: what is tracked, the latest checkpoint, unfinished
operations, and whether edits are saved automatically.

Use `status --missing` in scripts to exit with status 1 when any selected
entry is out of sync. It displays the same list as `status`.

Every `apply`, `add`, `unapply`, and `edit --apply` records a pair of
[history checkpoints](/history.html) — the tracked files before and after the
change. mise also records which paths the operation touched. Run
`mise bootstrap dotfiles history` to browse these checkpoints.

### JSON output

`mise bootstrap dotfiles status --json` uses `source_missing` for the
`source missing` state. Each entry also includes an `origin` object
describing where its configuration came from: the config file, its
`config_root`, any mise environment in the config filename, and the resolved
source path.

Paths are strings when they are valid UTF-8. On Unix, paths containing
non-UTF-8 bytes use `mise:path-bytes:<base64url>`.

## Capturing changes

If you edit a copied dotfile in place and want to store those changes back
in your dotfiles, run `mise bootstrap dotfiles add` again:

```sh
$EDITOR ~/.config/starship.toml
mise bootstrap dotfiles add ~/.config/starship.toml
```

Use `mise bootstrap dotfiles add --changed` to update the sources of all
changed regular files managed in `copy` mode. Each selected file's
configuration must be trusted. The command skips directory copies,
symlinks, templates, and inline content.

For an unmanaged target, `add` creates a `[dotfiles]` entry and seeds the
source under `dotfiles.root`. For an already-managed target, it updates the
existing source from the live target.

## Tracking options

### Saving and encryption {#policies}

| Field      | Default | Meaning                                                                                                                   |
| ---------- | ------- | ------------------------------------------------------------------------------------------------------------------------- |
| `autosave` | `true`  | Let the history watcher save edits automatically. With `false`, save the path with `mise bootstrap dotfiles save <path>`. |
| `encrypt`  | `false` | Encrypt contents before saving them to Git, using `[history.encryption].recipients`. The file you edit stays unencrypted. |

For a file you want to save manually:

```sh
mise bootstrap dotfiles track ~/.config/app/state.json --no-autosave
mise bootstrap dotfiles save ~/.config/app/state.json
```

The first command saves the initial version. Later edits wait for an
explicit save. See [saving](/history.html#saving) for how commands that
modify tracked files save their before and after versions.

Saving and sharing have separate settings. With manual synchronization,
mise continues making local commits. Your next push sends all accumulated
commits to the origin. Sharing applies to the entire history; to keep a
file out of it, leave it untracked. Untracking or excluding a file keeps
its previously committed versions in history.

### Files, directories, and symlinks

Track exact file or directory paths under your home directory or mise
configuration directory. Glob patterns are supported by
[history exclusions](/history.html#explicit-tracking-and-exclusions), but
cannot be used as tracking entries. Tracking a directory includes files
added beneath it later, subject to those exclusions.

Start with individual configuration files so you can choose what to save.
Leave logs, caches, databases, and application session state out of history.
Credential files and `*.local.toml` files are excluded by default; see
[encrypted tracking](/history.html#encrypted-shared-files) to save credentials.

Tracking a symlink saves the link itself. Track its target separately to
save the target's contents. If a parent directory is a symlink, track that
link and use the real directory path to track files beneath it.

Your home directory and mise configuration directory can themselves be
symlinks. mise maps these roots to the corresponding directories on each
machine when sharing history.

Add an explicit tracking entry for each file or directory you want to
save, including the mise configuration directory or `dotfiles.root`.
Track entries cannot contain `source`, `content`, `exclude`, or `manifest`.
`mise bootstrap dotfiles paths` reports such combinations as invalid and
leaves them out of history. The `track` command exits non-zero if the
entry it writes is not active.

### Stop tracking a file

```sh
mise bootstrap dotfiles untrack ~/.zshrc
```

The file stays in place. Its earlier checkpoints remain in Git, but future
checkpoints leave it out.

### Different contents on different machines {#variants}

A **variant** lets a tracked file have different contents on different
machines, using the same path on each one. For example, keep separate
versions of `~/.zshrc` for macOS and Linux:

```toml
[dotfiles]
"~/.zshrc" = { mode = "track", variants = [{ os = "macos" }, { os = "linux" }] }
```

Add this to your global configuration, or use
`mise bootstrap dotfiles track ~/.zshrc --os macos` to add one variant.
The `os` selector accepts an optional `/arch`, as in bootstrap packages.
Use `profile` to select a [mise environment](/configuration/environments.html):

```toml
[dotfiles]
"~/.gitconfig-work" = { mode = "track", variants = [{ profile = "work" }, { default = true }] }
```

When several variants match, mise scores each one: `profile` adds two
points, `os` adds one, and an architecture adds one more. The highest
score wins. A profile-only variant therefore ties with an OS-and-architecture
variant. If the highest score is tied, mise reports the ambiguity and
skips the path until you fix it.

When nothing matches, mise uses the variant marked `default = true`.
Without a default, it skips saving and applying the path on that machine.
Checkpoints preserve the versions saved by other machines.

### Tracking files that mise also manages {#ownership}

You can save the history of a file that mise copies, links, templates, or
edits. For example, tracking `~/.zshrc` also saves changes made by a managed
activation block or `mise bootstrap mise-shell-activate`.

When you track an already-managed file, mise keeps its existing entry and
adds the tracking entry to `conf.d/dotfiles-tracking.toml`. Untracking
removes only that tracking entry. A tracked directory can also contain
managed files or template sources; their copy, link, or template settings
continue to apply. Conflicting entries that try to create the same target
are still rejected.

Tracking entries belong in system or global configuration. mise warns and
ignores `mode = "track"` in project configuration. To turn off an entry
inherited from another configuration file, set `enabled = false` in a
later configuration layer. `untrack` does this for system entries.

## Self-managing mise config

You can manage the mise config and the dotfiles root as dotfiles too:

```toml
[settings]
dotfiles.root = "~/.dotfiles"

[dotfiles]
"~/.dotfiles" = "~/src/dotfiles"
"~/.config/mise/config.toml" = "~/src/dotfiles/mise/config.toml"
```

This is a bootstrap pattern: clone the real repo (for example
`~/src/dotfiles`) before the first `mise bootstrap dotfiles apply` or
`mise bootstrap`.
Use the real repo path for sources needed during the first run; `~/.dotfiles`
does not exist until mise creates that symlink.
Replacing `~/.config/mise/config.toml` affects future mise invocations, so
make sure the source contains a valid config before applying it.

## Root-owned files

Dotfiles write as the current user — there is no sudo here. Managing
`/etc/hosts` works when running as root (containers, CI); otherwise mise
fails with an ordinary permission error.

## Windows

`symlink` creates a real file symlink on Windows when it can. Windows allows that
without elevation once Developer Mode is on — the same privilege
[`windows_shim_mode`](/configuration/settings.html#windows_shim_mode) relies on for
its `symlink` option — and mise falls back to copying the file when the privilege
is not available, so entries keep applying either way.
`mise bootstrap dotfiles status` reads whichever form is on disk.

`symlink-each` still copies files on Windows. Directory symlinks use junctions.

## Command compatibility

> [!WARNING]
> The top-level `mise dotfiles` command is deprecated and hidden from help. It
> will begin warning in mise 2027.2.0 and be removed in mise 2028.2.0. Use
> `mise bootstrap dotfiles` instead.
