# System Files <Badge type="warning" text="experimental" />

mise can place config files (dotfiles) at machine-global paths via the
`[system.files]` section of `mise.toml`:

```toml
[system.files]
"~/.gitconfig" = "dotfiles/gitconfig"                                # symlink (default)
"~/.config/starship.toml" = { source = "dotfiles/starship.toml", mode = "copy" }
"~/.ssh/config" = { source = "dotfiles/ssh_config.tmpl", mode = "template" }
"~/.config/nvim" = "dotfiles/nvim"                                   # symlink the directory itself
"~/.local/bin" = { source = "dotfiles/bin", mode = "symlink-each" }  # symlink each file within
"~/.config/*.toml" = { source = "dotfiles/config/*.toml", mode = "copy" }
```

Each entry is keyed by the target path — absolute or starting with `~/` —
and points at a source file or directory. Relative sources resolve against
the directory of the config file that declares the entry, so a global
`~/.config/mise/config.toml` can manage dotfiles kept next to it, and a
project config can ship machine setup from the repo.

To manage one piece of a file something else owns (a line in `.zshrc`, an
entry in `/etc/hosts`) rather than the whole file, see
[System Edits](/system-edits.html).

Source paths may contain glob wildcards like `*`, `**`, `?`, or `[ab]`.
When a wildcard source matches multiple paths, the target path must contain
matching wildcards so each source expands to a unique target:

```toml
[system.files]
"~/.config/*.toml" = "dotfiles/config/*.toml"
"~/.local/share/app/**/*.json" = { source = "dotfiles/app/**/*.json", mode = "copy" }
"~/.config/app?.toml" = "dotfiles/config/app?.toml"
"~/.config/theme-[ab].toml" = "dotfiles/config/theme-[ab].toml"
```

## Modes

| Mode           | Behavior                                                                                                                                                                                                                                               |
| -------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `symlink`      | Symlink the target to the source. Works for files and directories — a directory source gets one link for the whole directory. This is the default.                                                                                                     |
| `symlink-each` | Source must be a directory: recreate its directory structure under the target and symlink each file individually, so the target directory (say, `~/.config`) can also hold files mise doesn't manage.                                                  |
| `copy`         | Copy the source file (or directory, recursively). Use when the target must be a real file — e.g. tools that rewrite their config in place. Directory copies are additive: matching files are overwritten, files mise doesn't manage are left in place. |
| `template`     | Render the source through the [mise template engine](/templates.html) and write the result. Permissions are taken from the source file (and repaired if they drift).                                                                                   |

Templates get the same context as other mise templates (`env`, `vars`,
`exec()`, etc.), which is the main reason to use them: one source file,
per-machine output.

Detecting whether a template's output has drifted requires rendering it, so
`mise system status` (and a real install) evaluates templates — including
any `exec()` calls — from your trusted config, just like `[env]` templates.
`--dry-run` is the exception: it promises to execute nothing, so it skips
template rendering and lists those entries as `(if changed)`.

## Semantics

Files follow the same rules as [system packages](/system-packages/):

- **Declarative and additive** — entries merge across the
  [config hierarchy](/configuration.html) (global → project) as a union of
  target keys; a more local config overrides an entry for the same target.
- **Manual application only** — nothing is written implicitly. Only
  `mise system install` (or [`mise bootstrap`](/cli/bootstrap.html)) applies
  files.
- **Idempotent** — entries already in their desired state are skipped;
  re-running is always safe.
- **Unknown modes are ignored with a warning** so configs using modes from
  newer mise versions still parse.

## Conflicts

mise refuses to _replace_ existing files it doesn't manage: a real file or
directory where a symlink should go, or a directory where a file should go,
is an error listing the conflicting paths. Pass
`mise system install --force` to replace them.

Content updates are not conflicts: a `copy` or `template` entry overwrites
the target file's content without `--force` — that is the declared intent of
those modes. Symlinks are re-pointed freely, since a symlink is never data.

## Commands

```sh
mise system status            # shows file state: applied/missing/differs
mise system status --missing  # exit 1 if anything is out of sync (CI check)

mise system install           # install packages, then apply files (prompts first)
mise system install --dry-run # print what would be done
mise system install --yes     # skip the confirmation prompt
mise system install --force   # also replace conflicting files
```

`mise system status` reports each entry as `applied`, `missing`, `differs`
(with a reason: re-pointed symlink, changed content, type conflict), or
`source missing`.

## Windows

File symlinks require elevation on Windows, so `symlink` and `symlink-each`
fall back to copying for files there; directory symlinks use junctions.
