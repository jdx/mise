---
description: "Manage one block, one line, or a few keys of a file that something else owns, and leave the rest of the file alone."
socialDescription: "Manage one block, line, or set of keys in a file that something else owns."
---

# Edit part of a file

An edit entry manages one piece of a file and leaves the rest alone: a block
between marker comments, a single line, or a few keys of a JSON, TOML, or YAML
file. Use it for a file that you or an application also edit by hand.

Edit entries live in `[dotfiles]`. Each key is the target path followed by an
id that names the edit within that file:

```toml
[dotfiles]
"~/.zshrc/aliases" = { block = '''
alias ll='ls -l'
alias la='ls -la'
''' }
"~/.ssh/config/include" = { line = "Include ~/.ssh/config.d/*", position = "prepend" }
"~/.gitconfig/identity" = { source = "snippets/git-identity.tmpl", template = "tera" }
```

`"~/.zshrc/aliases"` edits `~/.zshrc` with the id `aliases`. Ids can contain
letters, digits, `_`, `-`, and `.`. `mise dot apply` applies edits along with
the rest of `[dotfiles]`, and `mise dot status` and `mise dot diff` report
them.

To add `mise activate` to your shell's startup file, use
[shell activation](/bootstrap/shell.html) instead of writing the block
yourself.

## Blocks {#blocks}

A `block` sits between two marker comments named by the entry's id. The
`aliases` entry above writes:

```sh
# >>> mise:aliases >>> managed by mise — do not edit between markers
alias ll='ls -l'
alias la='ls -la'
# <<< mise:aliases <<<
```

Applying replaces whatever is between the markers. If the block is missing,
mise appends it to the file. Everything else in the file stays as it is.

mise picks the marker comment from the file extension: `--` for Lua, `"` for
`.vim` files, `;;` for Lisp, `;` for INI, `//` for C-like languages such as
JavaScript, Go, and Rust, and `#` for everything else, including files without
an extension such as `.zshrc`. Set `comment = "..."` to use another prefix.
`~/.vimrc` has no extension, so it gets `#`, which Vim does not read as a
comment; set `comment = '"'` on edits to `~/.vimrc`.

A file that cannot hold line comments, such as JSON, cannot take a block. To
set keys in a JSON, TOML, or YAML file, use a [merge entry](#merge);
otherwise [manage the whole file](/dotfiles/managed.html).

### Block content from a file {#block-content-from-a-file}

To keep a block's content in its own file instead of inline, set `source` and
`template = "tera"`, as the `identity` entry above does. mise renders the file
with [Tera](/templates.html) before it writes the block, and content without
Tera syntax renders unchanged. A relative `source` starts from the directory of
the config file that declares the entry. A table with only `source` is a
[whole-file entry](/dotfiles/managed.html#whole-file-entries), not an edit.

## Lines {#lines}

A `line` entry adds the given text if that exact line is missing. By default
mise appends it; set `position = "prepend"` to insert it at the beginning of the
file. Once the line is there, apply leaves it wherever it is. Other bytes,
including line endings, stay unchanged. The value must be a single line; use a
block for several.

## Set keys in a config file {#merge}

Some applications write their own state into the file that holds the settings
you care about: Codex rewrites `~/.codex/config.toml`, Claude Code rewrites
`~/.claude/settings.json`, and other tools rewrite a YAML file on every change.
A `symlink` or `copy` entry fights them for the whole file, and a block does
not survive an application that rewrites the file.

A `merge` entry owns only the keys in its source. Everything else in the file
stays the application's:

```toml
[dotfiles]
"~/.codex/config.toml/shared" = { source = "codex/shared.toml", merge = true }
"~/.claude/settings.json/shared" = { source = "claude/shared.json", merge = true }
"~/.omp/agent/config.yml/shared" = { source = "omp/shared.yml", merge = true }
```

The source holds the keys you want, in the same format as the target, which
mise infers from the target's `.json`, `.toml`, `.yaml`, or `.yml` extension.
With `codex/shared.toml` containing:

```toml
model = "gpt-5"

[tui]
theme = "light"
```

applying sets `model` and `tui.theme` and leaves the rest of
`~/.codex/config.toml` as it is, including `[plugins.*]` tables and comments
the application wrote. A missing target is created from the source.

- A table or object that exists on both sides merges recursively. Any other
  value, including an array, is replaced by the source's value.
- Keys that only the target has are never changed, and keys you remove from
  the source are not removed from the target.
- `mise dot status` and `mise dot diff` look only at the keys in the source, so
  keys the application adds never show up as drift.
- TOML and YAML are edited in place, so comments, key order, and the
  formatting of untouched keys stay. JSON has no comments; mise keeps its key
  order and indentation, and rewrites the file only when an owned key differs.
  A target that is not valid JSON, TOML, or YAML, including JSON with
  comments, is reported and never overwritten.
- `template = "tera"` renders the source first, as for blocks.
- Like a `symlink` entry, a merge entry without `source` uses the target's path
  under [`dotfiles.root`](/configuration/settings.html#dotfiles.root), so
  `"~/.codex/config.toml/shared" = { merge = true }` reads
  `<dotfiles.root>/.codex/config.toml`.
- To switch an entry from `symlink` to `merge`, point the merge at the same
  source. If the target is a link to that source, `mise dot apply` replaces the
  link with a regular copy before merging, so the application's keys survive
  when you later trim the source down to the keys you own. A target that links
  anywhere else is refused.

Two merge entries that set the same key of one file to different values are
refused instead of fighting over it. An entry with `template = "tera"` is
rendered only when it is applied, so mise finds a conflict with it when both
entries are applied in the same run, not when one is applied alone through a
target filter. mise does not compare a merge with a block or line edit of the
same file, so do not let both own one key.

### Defaults the application may change {#merge-missing}

Some keys are worth shipping as a default but belong to the application once
it has picked a value, such as the model a `/model` command writes. Use
`merge = "missing"` for those. It sets only the keys the target has no value
for, next to a regular `merge = true` entry for the keys you enforce:

```toml
[dotfiles]
"~/.codex/config.toml/shared" = { merge = true }
"~/.codex/config.toml/defaults" = { source = "codex/defaults.toml", merge = "missing" }
```

- A key the target already has keeps its value, even a different one. A key
  the application removes is filled in again on the next apply.
- A table that exists on both sides is compared key by key, so a default
  inside it is added without touching its siblings. A value that is not a
  table counts as present, so a source table under it is skipped.
- `mise dot status` and `mise dot diff` report only missing keys, never a
  differing value.
- A `missing` entry never conflicts with another entry for the same key: the
  other entry's value wins whichever applies first.

## Conflicts and removal {#conflicts}

Edits apply without `--force`. mise reports an error when a block's markers
are damaged, or when the target is a symlink; point the edit at the real file
you want to change instead. Editing a root-owned file, such as `/etc/hosts`,
requires running mise as root; see
[Files outside your home directory](/dotfiles/managed.html#root-owned-files).

Deleting an edit entry from your config leaves its block, line, or keys in the
file. [`mise dot unapply`](/cli/dotfiles/unapply.html) removes a block
together with its markers. A plain line has no marker to prove mise added it,
so removing one needs `mise dot unapply --force`. Merged keys stay, because
the application may have changed them since.

Edit entries merge across config files by target path and id, so a more local
config replaces an edit with the same id. The
[reference](/dotfiles/reference.html#edit-keys) lists every edit key.
