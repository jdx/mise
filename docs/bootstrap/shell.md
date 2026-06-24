# Shell Activation <Badge type="warning" text="experimental" />

mise can declaratively add shell activation snippets for bash, zsh, and fish
with `[bootstrap.mise_shell_activate]`:

```toml
[bootstrap.mise_shell_activate]
zsh = true
fish = true
bash = false
```

Use table form when you want a shape that can accept future options:

```toml
[bootstrap.mise_shell_activate.zsh]
enabled = true
```

`mise bootstrap shell apply` writes marker-delimited blocks to the shell rc
file:

| Shell | Target file                  | Block                          |
| ----- | ---------------------------- | ------------------------------ |
| bash  | `~/.bashrc`                  | `eval "$(mise activate bash)"` |
| zsh   | `~/.zshrc`                   | `eval "$(mise activate zsh)"`  |
| fish  | `~/.config/fish/config.fish` | `mise activate fish \| source` |

The markers are the same edit markers used by [Dotfiles](/dotfiles.html):

```sh
# >>> mise:activate >>> managed by mise - do not edit between markers
eval "$(mise activate zsh)"
# <<< mise:activate <<<
```

## Semantics

`[bootstrap.mise_shell_activate]` follows the same manual, idempotent model as
other bootstrap sections:

- **Per-shell override** - a project config can override a global setting for
  one shell with `zsh = false` without changing the other shells.
- **Manual application only** - mise never edits shell rc files implicitly.
  Only `mise bootstrap shell apply` and `mise bootstrap` apply this section.
- **Marker-owned edits** - mise only owns the block between its markers. Other
  content in the rc file is left untouched.
- **Explicit dotfiles win** - if `[dotfiles]` already manages the same rc file
  as a whole file, or defines an edit for the same target/id such as
  `"~/.zshrc/activate"`, mise skips the generated shell activation entry for
  that shell.

For fully managed rc files or custom activation blocks, use `[dotfiles]`
directly instead.

## Commands

```sh
mise bootstrap shell status            # shows activation block state
mise bootstrap shell status --json     # machine-readable
mise bootstrap shell status --missing  # exit 1 if anything is out of sync

mise bootstrap shell apply           # writes missing/different blocks
mise bootstrap shell apply --dry-run # print the edits instead
mise bootstrap shell apply --yes     # skip the confirmation prompt
```

JSON status entries use `state = "missing" | "applied" | "differs" |
"source_missing"`. Entries with `state = "differs"` also include a `reason`
field.
