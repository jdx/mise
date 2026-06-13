# System Edits <Badge type="warning" text="experimental" />

Where [System Files](/system-files.html) manages whole files, `[[system.edits]]`
manages one small piece of a file something else owns — the `mise activate`
line in your shell rc, an entry in `/etc/hosts`:

```toml
[[system.edits]]
path = "~/.zshrc"
id = "activate"                        # marker identity, default "mise"
block = 'eval "$(mise activate zsh)"'

[[system.edits]]
path = "/etc/hosts"
line = "127.0.0.1 dev.local"
```

## Blocks

A `block` is delimited by marker comments in the target file:

```sh
# >>> mise:activate >>> managed by mise — do not edit between markers
eval "$(mise activate zsh)"
# <<< mise:activate <<<
```

The markers are the ownership record, stored in the file itself, so the
design stays stateless: applying replaces only what's between them (or
appends the block if absent), and everything else in the file is untouched.
Content can come from three places:

```toml
[[system.edits]]
path = "~/.zshrc"
block = "..."                  # inline

[[system.edits]]
path = "~/.zshrc"
id = "aliases"
source = "snippets/aliases.sh" # from a file, relative to this config file

[[system.edits]]
path = "~/.gitconfig"
id = "identity"
source = "snippets/git-identity.tmpl"
template = true                # rendered with the mise template engine
```

`id` distinguishes multiple blocks in one file and appears in the markers;
it defaults to `mise`. The marker comment prefix is inferred from the file
extension (`#` for shell/config files, `--` for Lua, `//` for C-like
languages, `;` for INI, `"` for vim) and can be overridden with
`comment = "..."`. Files that can't hold line comments at all (strict JSON,
XML) aren't a fit for blocks — use [System Files](/system-files.html) to own
the whole file instead.

Detecting whether a template block has drifted requires rendering it, so
`mise system status` (and a real install) evaluates templates — including
any `exec()` calls — from your trusted config, just like `[env]` templates.
`--dry-run` is the exception: it promises to execute nothing, so it skips
template rendering and lists those entries as `(if changed)`.

## Lines

A `line` ensures an exact line exists somewhere in the file, appending it at
the end if absent. It never modifies or removes other lines, which is what
makes it safely idempotent — use it for files where a three-line marker
block is overkill or comments aren't tolerated.

## Semantics

Edits follow the same rules as the rest of [`[system]`](/system-packages/):

- **Declarative and additive** — entries merge across the
  [config hierarchy](/configuration.html) (global → project) as a union,
  keyed by `(path, id)` for blocks and `(path, line)` for lines; a more
  local config overrides a block with the same id.
- **Manual application only** — nothing is written implicitly. Only
  `mise system install` (or [`mise bootstrap`](/cli/bootstrap.html)) applies
  edits.
- **Idempotent** — entries already in their desired state are skipped;
  re-running is always safe.
- **Surgical** — edits never create conflicts with existing content and
  never need `--force`: a block owns only what's between its markers, a
  line only ever appends. Two cases are refused with an error instead of
  guessed at: corrupted markers (a begin without an end, or duplicates) and
  targets that are symlinks — an edit through a symlink would modify
  whatever the link points at (often a `[system.files]` source), so point
  the edit at the real file instead.

Removing an entry from config leaves its block in the file (mise keeps no
state database); delete the marker block by hand. The markers name mise and
the id, so it's always clear where a block came from.

## Commands

```sh
mise system status            # shows edit state: applied/missing/differs
mise system status --missing  # exit 1 if anything is missing (CI check)

mise system install           # packages, then files, then edits (prompts first)
mise system install --dry-run # print what would be done
mise system install --yes     # skip the confirmation prompt
```

`mise system status` reports each edit as `applied`, `missing` (no markers
or line yet), or `differs` (block content changed, corrupted markers, or a
symlink target).

## Root-owned files

Edits write as the current user — there is no sudo here. Editing
`/etc/hosts` works when running as root (containers, CI); otherwise mise
fails with an ordinary permission error.
