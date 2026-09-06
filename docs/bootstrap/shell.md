# Shell Activation

mise can declaratively add [shell activation](/getting-started.html#activate-mise)
snippets for bash, zsh, and fish with `[bootstrap.mise_shell_activate]`,
applied by `mise bootstrap mise-shell-activate apply` or as part of
[`mise bootstrap`](/bootstrap.html). Each key names a shell startup file and
each value picks a mode:

```toml
[bootstrap.mise_shell_activate]
zprofile = "shims"
zshrc = "activate"
bash_profile = "shims"
bashrc = "activate"
fish = "activate"
```

Configure only the shells and startup files you use. `"activate"` enables
interactive environment updates; `"shims"` makes installed tool commands
available without a prompt hook. See [shims](/dev-tools/shims.html) for that mode's
limits. The `mise` executable must already be on the startup file's `PATH`.

The inline table form makes enablement and mode explicit:

```toml
[bootstrap.mise_shell_activate]
zprofile = {enabled = true, mode = "shims"}
zshrc = {enabled = true, mode = "activate"}
```

Shell keys are shortcuts. For example, `zsh = true` expands to
`zprofile = "shims"` and `zshrc = "activate"`.

Any target can use either `"activate"` or `"shims"`. Boolean `true` enables the
target with its default mode, and `false` disables it.

`mise bootstrap mise-shell-activate apply` writes marker-delimited blocks to the shell rc
file:

| Target         | Shell | Default mode | Target file                  | Block                                  |
| -------------- | ----- | ------------ | ---------------------------- | -------------------------------------- |
| `bash_profile` | bash  | `shims`      | `~/.bash_profile`            | `eval "$(mise activate bash --shims)"` |
| `bashrc`       | bash  | `activate`   | `~/.bashrc`                  | `eval "$(mise activate bash)"`         |
| `zprofile`     | zsh   | `shims`      | `~/.zprofile`                | `eval "$(mise activate zsh --shims)"`  |
| `zshrc`        | zsh   | `activate`   | `~/.zshrc`                   | `eval "$(mise activate zsh)"`          |
| `zshenv`       | zsh   | `shims`      | `~/.zshenv`                  | `eval "$(mise activate zsh --shims)"`  |
| `fish`         | fish  | `activate`   | `~/.config/fish/config.fish` | `mise activate fish \| source`         |

The markers are the same edit markers used by [Dotfiles](/dotfiles.html):

```sh
# >>> mise:activate >>> managed by mise - do not edit between markers
eval "$(mise activate zsh)"
# <<< mise:activate <<<
```

## Semantics

`[bootstrap.mise_shell_activate]` follows the same manual, idempotent model as
other bootstrap sections:

- **Per-target override** — a project config can override a global setting for
  one startup file with `zshrc = false` without changing `zprofile`.
- **Manual application only** — mise never edits shell rc files implicitly.
  Only `mise bootstrap mise-shell-activate apply` and `mise bootstrap` apply this section.
- **Marker-owned edits** — mise only owns the block between its markers. Other
  content in the rc file is left untouched.
- **Shims stay out of `zshenv` by default** — `zshenv` is supported when
  configured explicitly, but shell shortcuts do not write it because zsh reads
  it for every invocation, including scripts.
- **Explicit dotfiles win** — if `[dotfiles]` already manages the same rc file
  as a whole file, or defines an edit for the same target/id such as
  `"~/.zshrc/activate"`, mise skips the generated shell activation entry for
  that shell.

For fully managed rc files or custom activation blocks, use `[dotfiles]`
directly instead. Review any existing unmarked activation lines to avoid running
the hook twice. After applying, open a new shell and check `mise doctor`; editing
a startup file does not change the shell process already running.

## Commands

```sh
mise bootstrap mise-shell-activate status            # shows activation block state
mise bootstrap mise-shell-activate status --json     # machine-readable
mise bootstrap mise-shell-activate status --missing  # exit 1 if anything is out of sync

mise bootstrap mise-shell-activate apply           # writes missing/different blocks
mise bootstrap mise-shell-activate apply --dry-run # print the edits instead
mise bootstrap mise-shell-activate apply --yes     # skip the confirmation prompt
```

JSON status entries include `target`, `shell`, `path`, `mode`, and `state`.
`state` is `"missing" | "applied" | "differs" | "source_missing"`. Entries
with `state = "differs"` also include a `reason` field.
