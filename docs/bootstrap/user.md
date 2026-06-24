# User Login Shell <Badge type="warning" text="experimental" />

mise can declare the current user's login shell in `[bootstrap.user]` and
apply it with `mise bootstrap user apply` or
[`mise bootstrap`](/cli/bootstrap.html):

```toml
[bootstrap.user]
login_shell = "/bin/zsh"
```

When the configured shell is not listed in `/etc/shells`, mise appends it
first. When the configured shell differs from the user's account entry, mise
runs:

```sh
chsh -s /bin/zsh
```

Top-level `mise bootstrap` also includes a final follow-up reminder to start a
new login session when it changes or would change the login shell.

## Semantics

`[bootstrap.user].login_shell` follows the same manual, idempotent model as
[bootstrap packages](/bootstrap/packages/):

- **Most local wins** - a project config can override a global
  `login_shell`; unlike package/file lists, there is only one desired value.
- **Manual application only** - mise never changes your login shell
  implicitly. Only [`mise bootstrap`](/cli/bootstrap.html) applies it.
- **Listed shell** - the shell must appear in `/etc/shells` before `chsh`
  accepts it on many platforms. mise adds the configured path to that file
  when it is missing.
- **Unix-only** - on non-Unix platforms, or when `chsh` is not available,
  `mise bootstrap user status` reports the entry as skipped and bootstrap
  ignores it.
- **Absolute path required** - relative shell names are skipped with a
  warning. Use the full path, such as `/bin/zsh` or `/opt/homebrew/bin/fish`.

`/etc/shells` is usually root-owned. If the file is not writable, mise uses
the same non-interactive sudo behavior as system packages: it can prompt in an
interactive terminal, uses passwordless sudo in non-interactive contexts, and
honors `system_packages.sudo = false`.

When `mise` itself is started under `sudo`, login shell status and `chsh`
target `SUDO_USER` rather than root. Plain root sessions, such as containers,
still target root.

## Commands

```sh
mise bootstrap user status            # shows login shell state
mise bootstrap user status --missing  # exit 1 if the shell differs or is not listed

mise bootstrap user apply           # updates /etc/shells and runs chsh -s
mise bootstrap user apply --dry-run # print the commands instead
mise bootstrap user apply --yes     # skip the confirmation prompt
```
