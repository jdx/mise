# Dotfiles

> [!WARNING]
> The top-level `mise dotfiles` command is deprecated and hidden from help. It
> will begin warning in mise 2027.2.0 and be removed in mise 2028.2.0. Use
> `mise bootstrap dotfiles` instead.

mise can manage dotfiles from the `[dotfiles]` section of `mise.toml`.
Entries can either own a whole file or directory, or manage one small piece
of a file something else owns.

```toml
[settings]
dotfiles.root = "~/.dotfiles"
dotfiles.default_mode = "symlink"

[dotfiles]
"~/.zshrc" = {}                                                       # ~/.dotfiles/.zshrc
"~/.gitconfig" = "dotfiles/gitconfig"                                # explicit source
"~/.config/alacritty.toml" = { mode = "copy" }                       # ~/.dotfiles/.config/alacritty.toml
"~/.config/starship.toml" = { source = "dotfiles/starship.toml", mode = "copy" }
"~/.ssh/config" = { source = "dotfiles/ssh_config.tmpl", mode = "template" }
"~/.config/nvim" = "dotfiles/nvim"                                   # symlink the directory itself
"~/.local/bin" = { source = "dotfiles/bin", mode = "symlink-each" }  # symlink each file within
"~/hosts/dev" = { line = "127.0.0.1 dev.local" }                     # edit one line in ~/hosts
```

New entries are captured and applied by `mise bootstrap dotfiles add`; pass
`--no-apply` to only capture them. Existing entries can be applied explicitly
with `mise bootstrap dotfiles apply` or as part of
[`mise bootstrap`](/bootstrap.html). They are never applied implicitly by
`mise install` or `mise bootstrap packages`.
The nested apply command runs the configured `pre-dotfiles` and
`post-dotfiles` bootstrap hooks.

## Whole-file entries

Whole-file entries are keyed by the target path — absolute or starting with
`~/` — and may point at a source file or directory. If `source` is omitted,
mise mirrors the home-relative target path under `dotfiles.root`: `~/.zshrc`
uses `~/.dotfiles/.zshrc`, and `~/.config/foo.toml` uses
`~/.dotfiles/.config/foo.toml`. Targets outside `$HOME` must specify
`source`.

String entries are shorthand for an explicit source with
`dotfiles.default_mode`. `mise bootstrap dotfiles add` omits an implied source
and the built-in `symlink` mode, while preserving a mode explicitly selected
with `--mode`:

```toml
[dotfiles]
"~/.zshrc" = {}
"~/.ssh/config" = { source = "ssh/config", mode = "copy" }
```

Relative explicit sources resolve against the directory of the config file
that declares the entry, so a global `~/.config/mise/config.toml` can manage
dotfiles kept next to it, and a project config can ship machine setup from
the repo.

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

Excluding a file mise already applied removes what it left behind on the next
apply, the same as deleting the source would.

## Modes

| Mode           | Behavior                                                                                                                                                                                                                                                                                                                              |
| -------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `symlink`      | Symlink the target to the source. Works for files and directories — a directory source gets one link for the whole directory. This is the default.                                                                                                                                                                                    |
| `symlink-each` | Source must be a directory: recreate its directory structure under the target and symlink each file individually, so the target directory (say, `~/.config`) can also hold files mise doesn't manage. Deleting a source file removes the link it left behind on the next apply; files and links mise didn't create are never touched. |
| `copy`         | Copy the source file (or directory, recursively). Use when the target must be a real file — e.g. tools that rewrite their config in place. Directory copies are additive: matching files are overwritten, files mise doesn't manage are left in place. Copies are never pruned, so removing a source file leaves the copy behind.     |
| `template`     | Render the source through the [mise template engine](/templates.html) and write the result. Permissions are taken from the source file (and repaired if they drift).                                                                                                                                                                  |

Templates get the same context as other mise templates (`env`, `vars`,
`exec()`, etc.), which is the main reason to use them: one source file,
per-machine output.

Detecting whether a template's output has drifted requires rendering it, so
`mise bootstrap dotfiles status` and a real apply evaluate templates — including any
`exec()` calls — from your trusted config, just like `[env]` templates.
`--dry-run` is the exception: it promises to execute nothing, so it skips
template rendering and lists those entries as `(if changed)`.

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
- **Manual application only** — nothing is written implicitly. Only
  `mise bootstrap dotfiles apply` or [`mise bootstrap`](/bootstrap.html) applies
  dotfiles.
- **Idempotent** — entries already in their desired state are skipped;
  re-running is always safe.
- **Unknown modes and operations are ignored with a warning** so configs
  using features from newer mise versions still parse.

## Conflicts

mise refuses to _replace_ existing files it doesn't manage: a real file or
directory where a symlink should go, or a directory where a file should go,
is an error listing the conflicting paths. Pass
`mise bootstrap dotfiles apply --force` to replace them.

For symlink entries, an existing regular file with identical content to the
source is converged without `--force` by replacing it with the requested
symlink. If the content differs, mise still treats it as a conflict.

Content updates are not conflicts: a `copy` or `template` entry overwrites
the target file's content without `--force` — that is the declared intent of
those modes. Symlinks are re-pointed freely, since a symlink is never data.

Edit entries never need `--force`: a block owns only what's between its
markers, and a line only ever appends. Two cases are refused with an error
instead of guessed at: corrupted markers and targets that are symlinks. An
edit through a symlink would modify whatever the link points at, often a
`[dotfiles]` source, so point the edit at the real file instead.

Removing an entry from config leaves its file, block, or line in place
because mise keeps no state database. Run `mise bootstrap dotfiles unapply`
before removing the entry when you want mise to clean up its observable
footprint.

## Unapplying

`mise bootstrap dotfiles unapply` removes configured targets without removing
their `[dotfiles]` entries or source files. It uses the current config and
filesystem to determine what the entry owns:

- `symlink` targets are removed only while they still point to the configured
  source.
- `symlink-each` removes exact source-to-target links, including dangling links
  for deleted source files. Other links and files under the target survive.
- file copies and rendered templates are removed only while their content still
  matches. Modified targets require `--force`.
- directory copies are removed file by file. Unmanaged neighbors always
  survive, and directories are removed only when empty.
- marker-delimited blocks are removed with their markers. Plain line edits have
  no ownership marker and require `--force`.

Unapply is deliberately conservative because dotfiles do not have an apply
manifest. In particular, a copied file whose source was deleted can no longer
be identified inside an additive directory copy. Remove such leftovers by
hand. Use `--dry-run` to inspect the identifiable removals first; template
dry-runs do not render or execute template functions.

## Commands

```sh
mise bootstrap dotfiles status            # shows applied/missing/differs/source missing
mise bootstrap dotfiles status --missing  # exit 1 if anything is out of sync

mise bootstrap dotfiles apply                     # apply files and edits
mise bootstrap dotfiles apply --dry-run           # print what would be done
mise bootstrap dotfiles apply --dry-run --verbose # include diff-like details
mise bootstrap dotfiles apply --yes               # skip the confirmation prompt
mise bootstrap dotfiles apply --force             # also replace conflicting files

mise bootstrap dotfiles unapply             # remove identifiable managed targets
mise bootstrap dotfiles unapply --dry-run   # preview removals
mise bootstrap dotfiles unapply --force     # also remove modified/ambiguous targets

mise bootstrap dotfiles add ~/.zshrc       # capture a live file into dotfiles.root
mise bootstrap dotfiles edit ~/.zshrc      # edit the managed source or owning config
mise bootstrap dotfiles edit --apply ~/.zshrc
```

`mise bootstrap dotfiles status` reports each entry as `applied`, `missing`,
`differs` with a reason, or `source missing`.

## Capturing changes

If you edit a copied dotfile in place and want to store those changes back
in your dotfiles, run `mise bootstrap dotfiles add` again:

```sh
$EDITOR ~/.config/starship.toml
mise bootstrap dotfiles add ~/.config/starship.toml
```

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

File symlinks require elevation on Windows, so `symlink` and `symlink-each`
fall back to copying for files there; directory symlinks use junctions.
