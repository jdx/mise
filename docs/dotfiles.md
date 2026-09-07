# Dotfiles

`[dotfiles]` declares how each of your configuration files is managed. The
recommended way to adopt a file you already edit in place is to **track** it:
the file stays where it is, nothing is copied or linked, and
[history](/history.html) saves a checkpoint of it right away and after every
change it sees.

```sh
mise bootstrap dotfiles track ~/.zshrc ~/.config/hypr
mise bootstrap                                  # packages, tools, shell activation, …
mise bootstrap dotfiles status                             # what is protected, and how
```

```toml
[dotfiles]
"~/.zshrc" = { mode = "track" }
"~/.config/hypr" = { mode = "track" }
"~/.config/app/state.json" = { mode = "track", autosave = false }   # saved only on request
"~/.ssh/config" = { mode = "track", share = false }                 # never shared
```

Templates, symlinks, copies, inline content, and managed edits are the
complementary techniques for files mise generates or places for you. They keep
working exactly as before, and the sources they reference are tracked
automatically:

```toml
[dotfiles]
"~/.gitconfig" = { source = "templates/gitconfig.tera", mode = "template" }
"~/.config/alacritty.toml" = { mode = "copy" }                       # ~/.dotfiles/.config/alacritty.toml
"~/.config/nvim" = "dotfiles/nvim"                                   # symlink the directory itself
"~/.local/bin" = { source = "dotfiles/bin", mode = "symlink-each" }  # symlink each file within
"~/.config/tool.conf" = { content = "enabled = true\n" }              # inline whole-file content
"~/.zshrc/activate" = { block = 'eval "$(mise activate zsh)"' }      # a managed block in a tracked file
"~/hosts/dev" = { line = "127.0.0.1 dev.local" }                     # edit one line in ~/hosts
```

Source-managed entries are captured and applied by `mise bootstrap dotfiles add`
(pass `--no-apply` to only capture them) and applied explicitly with
`mise bootstrap dotfiles apply` or as part of [`mise bootstrap`](/bootstrap.html).
They are never applied implicitly by `mise install` or `mise bootstrap packages`.
The nested apply command runs the configured `pre-dotfiles` and
`post-dotfiles` bootstrap hooks.

## Command compatibility

> [!WARNING]
> The top-level `mise dotfiles` command is deprecated and hidden from help. It
> will begin warning in mise 2027.2.0 and be removed in mise 2028.2.0. Use
> `mise bootstrap dotfiles` instead.

## Tracking files in place

`mise bootstrap dotfiles track <path>…` writes a `mode = "track"` entry for
each path (a file or a directory) into the global `config.toml`, saves a
baseline checkpoint of it immediately, and reports whether anything saves
later edits automatically. Tracking never infers a source under
`dotfiles.root`, never moves the file, and never replaces it with a symlink.
`mise bootstrap dotfiles untrack <path>` removes the declaration and stops
future captures; the file and its existing checkpoints stay exactly as they
are, and nothing re-enrolls the path later.

A track entry takes no `source`, `content`, `exclude`, or `manifest`: a
declaration combining them is reported by `mise bootstrap dotfiles paths` as invalid and
never counted as protection, and `track` exits non-zero when the entry it
wrote is not active.

### Policies

| Field      | Default | Meaning                                                                                                                                                     |
| ---------- | ------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `autosave` | `true`  | Save edits automatically. `false` makes a manual-save file: `mise bootstrap dotfiles save <path>`, or an operation that names it, promotes what is on disk. |
| `share`    | `true`  | Publish the saved version to the shared setup branch of the connected repository.                                                                           |
| `encrypt`  | `false` | Encrypt shared contents for `[history.encryption].recipients`; local files remain plaintext.                                                                |
| `backup`   | `true`  | Include the file in this machine's remote backups (plaintext, or encrypted when the connection was made with `--encrypt-backups`).                          |

`mise bootstrap dotfiles track --no-autosave|--no-share|--no-backup` sets them.
`--private` sets both `share = false` and `backup = false` to keep contents local;
the path and declaration can still be shared, and earlier uploads are not erased.
`--local` writes the entry to `config.local.toml` (this machine only). A local
override of an inherited entry restates the effective entry with only the
changed field, so its variants and other policies are kept.

On source-managed entries the same fields govern the destination's own local
history: rendered and inline output is recoverable but not backed up by
default (`backup = false`), copies are (`backup = true`), and output is never
shared. The referenced source is governed by the entry covering it — the
global config directory or `dotfiles.root` for the usual layouts.

### Variants

A tracked file can hold different contents on different machines while
keeping the same live path. Variants use the same selectors as bootstrap
packages (`os`, with an optional `/arch`) and mise environments (`profile`):

```toml
[dotfiles]
"~/.zshrc" = { mode = "track", variants = [{ os = "macos" }, { os = "linux" }] }
"~/.gitconfig-work" = { mode = "track", variants = [{ profile = "work" }, { default = true }] }
```

`mise bootstrap dotfiles track ~/.zshrc --os macos` adds a variant. The most
specific matching variant wins (`profile` over an arch-qualified `os` over an
`os` alone); two matching equally are reported as ambiguous and the path is
left out until fixed. A machine matching no variant keeps local protection but
has no shared stream unless a variant is marked `default = true`.

### Ownership

One declaration owns a destination. Tracking coexists with managed edits and
with shell activation: `mise bootstrap mise-shell-activate` keeps editing a
tracked `~/.zshrc`, the edit is journaled, and the file stays tracked. A track
entry and a source-managed entry for the same destination are a duplicate
target (the later config layer wins; composed roots report both). `add` refuses
a tracked destination, `track` refuses a source-managed one, and `unapply`
never deletes a tracked file.

`enabled = false` on a later layer switches an inherited declaration off on
this machine, which is what `untrack` writes for entries declared by the
system configuration. Tracking is enrolled from the system and global
configuration only: a `mode = "track"` entry in a project config is ignored
with a warning, so no project can enroll files in your history.

`*.local.toml` files are private by default wherever they are found, and
credential stores under the config directory are neither shared nor backed
up; a per-file declaration is the only override.

`mise bootstrap dotfiles status` reports tracked entries as `tracked` and
source-managed ones as `applied`, `missing`, `differs`, or `source missing`
— deployment drift. History and sync state live in `mise bootstrap dotfiles status`.

## Start with one managed file

Create `dotfiles/example.conf` next to your `mise.toml`, then add an entry for an unused target:

```toml
[dotfiles]
"~/.config/mise-dotfiles-example.conf" = {
    source = "dotfiles/example.conf",
    mode = "copy",
}
```

Inspect the declaration, preview its application, and then apply it:

```sh
mise bootstrap dotfiles status
mise bootstrap dotfiles apply --dry-run
mise bootstrap dotfiles apply
mise bootstrap dotfiles status --missing
```

The last command exits successfully only when all selected entries match their sources.
Editing a copy's target does not update the source: a later apply overwrites it. Use
[`add` to capture changes](#capturing-changes), or edit the managed source with
`mise bootstrap dotfiles edit <target>`.

To adopt an existing file as a managed source, use `mise bootstrap dotfiles add <target>`.
This captures the live file into `dotfiles.root`, writes a declaration, and applies it.
Start with `--no-apply` when you want to review the captured source and configuration
first. To keep the file where it is instead, [track it](#tracking-files-in-place).
The nested apply command runs the configured `pre-dotfiles` and
`post-dotfiles` bootstrap hooks.

## Whole-file entries

Whole-file entries are keyed by the target path — absolute or starting with
`~/` — and may point at a source file or directory. If `source` is omitted,
mise mirrors the home-relative target path under `dotfiles.root`: `~/.zshrc`
uses `~/.dotfiles/.zshrc`, and `~/.config/foo.toml` uses
`~/.dotfiles/.config/foo.toml`. Targets outside `$HOME` must specify `source`
or inline `content`.

String entries are shorthand for an explicit source with
`dotfiles.default_mode`. `mise bootstrap dotfiles add` omits an implied source
and the built-in `symlink` mode, while preserving a mode explicitly selected
with `--mode`:

```toml
[dotfiles]
"~/.zshrc" = { mode = "symlink" }
"~/.ssh/config" = { source = "ssh/config", mode = "copy" }
```

Relative explicit sources resolve against the directory of the config file
that declares the entry, so a global `~/.config/mise/config.toml` can manage
dotfiles kept next to it, and a project config can ship machine setup from
the repo.

`mise bootstrap dotfiles status --json` includes an `origin` object for every
entry. It reports the declaring config, its `config_root`, any configuration
environment encoded by that config filename, and the resolved source path.
Paths are ordinary strings when they are valid UTF-8. On Unix, a path containing
non-UTF-8 bytes uses `mise:path-bytes:<base64url>` so provenance remains lossless.
This makes layered dotfile declarations inspectable without reconstructing
their precedence by hand.

Use `content` to declare a literal whole file inline instead of keeping a
separate source file. Inline content is written as a private regular file
(`0600` on Unix). Use `content` on its own, without `source`, `mode`, `exclude`, `manifest`,
or the edit options `block`, `line`, `template`, and `comment`:

```toml
[dotfiles]
"~/.config/example.conf" = { content = "enabled = true\n" }
```

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

## Modes

| Mode           | Target behavior                                                      | Use when                                                       |
| -------------- | -------------------------------------------------------------------- | -------------------------------------------------------------- |
| `symlink`      | One link to a file or entire directory; the default.                 | Edits to the target should edit the source.                    |
| `symlink-each` | Recreate directories and link individual files.                      | A target directory also contains unmanaged neighbors.          |
| `copy`         | Copy a file or directory; overwrite matching files.                  | The application needs a regular file or writes its own config. |
| `template`     | Render a source file through the [template engine](/templates.html). | The output depends on machine-specific variables.              |

`symlink-each` requires a directory source. It records managed links under
`$MISE_STATE_DIR/dotfiles`, so subsequent applies can remove links for deleted or excluded
sources without recursively scanning a shared target. Unmanaged neighbors are preserved.
Keep that state directory when you want mise to reconcile previously applied profiles.

Directory copies are additive and **never pruned**: deleting or excluding a source leaves
its old copy behind. Review and remove those leftovers yourself. Templates use the source
file's permissions and repair permission drift when applied. See [Windows](#windows) for
platform-specific link behavior.

Templates get the same context as other mise templates (`env`, `vars`,
`exec()`, etc.), which is the main reason to use them: one source file,
per-machine output.

Detecting whether a template's output has drifted requires rendering it, so
`mise bootstrap dotfiles status`, `mise bootstrap dotfiles diff`, and a real
apply evaluate templates — including any `exec()` calls — from your trusted
config, just like `[env]` templates. Dotfile `--dry-run` skips rendering the dotfile
templates and lists those entries as `(if changed)`. It does not suppress unrelated
configuration evaluation, so this is a preview of dotfile writes, not a sandbox for
untrusted configuration.

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

The markers are the ownership record, stored in the file itself, so the
design stays stateless: applying replaces only what's between them or
appends the block if absent, and everything else in the file is untouched.

Ids may contain letters, digits, `_`, `-`, and `.`. The marker comment
prefix is inferred from the file extension (`#` for shell/config files,
`--` for Lua, `//` for C-like languages, `;` for INI, `"` for vim) and can
be overridden with `comment = "..."`. Files that can't hold line comments
at all (strict JSON, XML) aren't a fit for blocks — use a whole-file entry
instead.

A `line` ensures an exact line exists somewhere in the file, appending it at
the end if absent. It never modifies or removes other lines, which is what
makes it safely idempotent. The value must be a single line; use a block for
multi-line content.

## Semantics

- **Declarative and additive** — entries merge across the
  [config hierarchy](/configuration.html) (global → project). Whole-file
  entries merge by target path; edit entries merge by `(path, id)`.
- **Explicit application** — `mise bootstrap dotfiles add` applies the entries
  it captures unless `--no-apply` is set. Entries not captured by `add` are
  applied by `mise bootstrap dotfiles apply` or [`mise bootstrap`](/bootstrap.html).
- **Skip unchanged output** — entries already in their desired state are skipped.
  Templates may still execute while checking their output, and copy/template entries
  overwrite changed targets on apply.
- **Unknown modes and operations are ignored with a warning** so configs
  using features from newer mise versions still parse.

## Conflicts

For symlink entries, mise refuses to replace conflicting existing paths: a real file or
directory where a symlink should go, or a directory where a file should go,
is an error listing the conflicting paths. Pass
`mise bootstrap dotfiles apply --force` to replace them.

Real files and directories always require `--force` during a standalone
symlink apply, even when their visible content and permissions match. Portable
filesystem APIs cannot compare ownership, ACLs, extended attributes, flags,
and security labels. `mise bootstrap dotfiles add` avoids that destructive
comparison by moving each captured real path to its source before creating the
symlink; cross-filesystem moves fall back to a symlink- and
permission-preserving copy.

Content updates in writing modes are not conflicts: a `copy` or `template` entry overwrites
the target file's content without `--force` — that is the declared intent of
those modes. Existing symlinks can be repointed; inspect the diff before changing which source a target uses.

Edit entries never need `--force`: a block owns only what's between its
markers, and a line only ever appends. Two cases are refused with an error
instead of guessed at: corrupted markers and targets that are symlinks. An
edit through a symlink would modify whatever the link points at, often a
`[dotfiles]` source, so point the edit at the real file instead.

Removing an entry from config leaves its file, block, or line in place
because the active config still defines which state belongs to an entry. Run
`mise bootstrap dotfiles unapply` before removing the entry when you want mise
to clean up its observable footprint.

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

Unapply is deliberately conservative because `copy` and `template` entries have
no apply manifest. In particular, a copied file whose source was deleted can no
longer be identified inside an additive directory copy. Remove such leftovers
by hand. Use `--dry-run` to inspect the identifiable removals first; template
dry-runs do not render or execute template functions.

## Commands

```sh
mise bootstrap dotfiles status            # shows applied/missing/differs/source missing
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
mise bootstrap dotfiles paths                   # what history tracks, under which policies
```

`mise bootstrap dotfiles status` reports each entry as `tracked`, `applied`,
`missing`, `differs` with a reason, or `source missing`, followed by the
history state: what is tracked, the latest checkpoint, unfinished
operations, and whether edits are saved automatically. JSON uses
`source_missing` for the last state and includes
[origin information](#whole-file-entries). `--missing` changes the exit status
when any selected entry is out of sync; it does not filter the displayed list.

Every `apply`, `add`, `unapply`, and `edit --apply` records a pair of
[history checkpoints](/history.html) — the tracked files before and after the
change, with a journal of every path it touched — and `mise bootstrap dotfiles history` lists
them.

## Capturing changes

If you edit a copied dotfile in place and want to store those changes back
in your dotfiles, run `mise bootstrap dotfiles add` again:

```sh
$EDITOR ~/.config/starship.toml
mise bootstrap dotfiles add ~/.config/starship.toml
```

Use `mise bootstrap dotfiles add --changed` to update every changed regular file
managed in `copy` mode at once. Directory copies are excluded because reversing
an additive copy could delete source files intentionally excluded from the live
tree. Symlinks already edit their source directly, and templates and inline
content cannot be reverse-rendered, so they are not included. Bulk capture also
requires the configuration declaring each selected file to be trusted.

For an unmanaged target, `add` creates a `[dotfiles]` entry and seeds the
source under `dotfiles.root`. For an already-managed target, it updates the
existing source from the live target.

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
