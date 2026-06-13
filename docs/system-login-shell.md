# System Login Shell <Badge type="warning" text="experimental" />

mise can declare the current user's login shell in the `[system]` section of
`mise.toml` and apply it with `mise system install`:

```toml
[system]
login_shell = "/bin/zsh"
```

When the configured shell differs from the user's account entry, mise runs:

```sh
chsh -s /bin/zsh
```

## Semantics

`[system].login_shell` follows the same manual, idempotent model as
[system packages](/system-packages/):

- **Most local wins** - a project config can override a global
  `login_shell`; unlike package/file lists, there is only one desired value.
- **Manual application only** - mise never changes your login shell
  implicitly. Only `mise system install` or [`mise bootstrap`](/cli/bootstrap.html)
  applies it.
- **Unix-only** - on non-Unix platforms, or when `chsh` is not available,
  `mise system status` reports the entry as skipped and install ignores it.
- **Absolute path required** - relative shell names are skipped with a
  warning. Use the full path, such as `/bin/zsh` or `/opt/homebrew/bin/fish`.

mise does not edit `/etc/shells`. If your platform requires the shell to be
listed there, add it yourself before applying this setting; any `chsh` error
is surfaced as-is.

## Commands

```sh
mise system status            # shows login shell state
mise system status --missing  # exit 1 if the configured shell differs

mise system install           # runs chsh -s when needed
mise system install --dry-run # print the chsh command instead
mise system install --yes     # skip the confirmation prompt
```

Explicit package arguments and `--manager` scope `mise system install` to
packages only, so `login_shell` is applied by the bare converge-everything
form.
