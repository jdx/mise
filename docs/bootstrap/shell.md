---
description: "Write mise activation into bash, zsh, and fish startup files, and set your login shell, as part of mise bootstrap."
socialDescription: "Write mise activation into shell startup files and set your login shell."
---

# Shell activation and login shell

`[bootstrap.mise_shell_activate]` writes mise activation into your shell
startup files, and `[bootstrap.user]` sets your login shell. `mise bootstrap`
applies both, so a new machine's shell is ready after one run.

## Activate mise in your shell

For zsh, one key is enough:

```toml
[bootstrap.mise_shell_activate]
zsh = true
```

`zsh = true` adds shims to `~/.zprofile` and activation to `~/.zshrc`. Use
`bash = true` for `~/.bash_profile` and `~/.bashrc`, or `fish = true` for
`~/.config/fish/config.fish`. Apply the section on its own, or as part of a full
run:

```sh
mise bootstrap mise-shell-activate apply
mise bootstrap
```

The blocks are the same lines [Shell setup](/shell-setup.html) shows for adding
activation by hand.

### Put mise on PATH first

The blocks call `mise` by name, so `mise` must already be on `PATH` when the
startup file runs. A package manager install, such as Homebrew, takes care of
this. For the [mise.run](/installing-mise.html) install in `~/.local/bin`, put
that directory on `PATH` before the blocks run, for example with a
[`[dotfiles]` line entry](/dotfiles/edits.html) in `~/.zshenv`, which zsh reads
first:

```toml
[dotfiles]
"~/.zshenv/local-bin" = { line = 'export PATH="$HOME/.local/bin:$PATH"' }
```

For Bash, add the same line to `~/.bash_profile` and `~/.bashrc` with
`position = "prepend"`, so it comes before the blocks. For fish, prepend a line
that adds `~/.local/bin` to `PATH` in `~/.config/fish/config.fish`.
`mise bootstrap` applies `[dotfiles]` before shell activation, so one run writes
both.

### Why login files get shims

Activation applies the environment when the activation line runs, then updates
it from hooks at each prompt and directory change. An editor or other program
reads your login profile once, when it starts, and keeps that environment, so
activation there would fix the tool versions of the directory the login shell
started in. Shims choose the version each time a tool runs, from the directory
it runs in. That is why the login files, `~/.zprofile` and `~/.bash_profile`,
get shims, while `~/.zshrc` and `~/.bashrc`, which interactive shells read, get
full activation. fish has one
startup file, `config.fish`, and it gets full activation. See
[Shims](/dev-tools/shims.html#overview) for how the two compare.

Bash login shells read `~/.bash_profile` but not `~/.bashrc`. If your
`~/.bash_profile` does not source `~/.bashrc`, an interactive login shell gets
shims but not activation.

## Choose files and modes

Each key names a shell or a startup file. A shell key is a shortcut for that
shell's default files, and a file key sets one file:

| Key            | Shell | File                         | Default mode | Block in the default mode              |
| -------------- | ----- | ---------------------------- | ------------ | -------------------------------------- |
| `bash_profile` | bash  | `~/.bash_profile`            | `shims`      | `eval "$(mise activate bash --shims)"` |
| `bashrc`       | bash  | `~/.bashrc`                  | `activate`   | `eval "$(mise activate bash)"`         |
| `zprofile`     | zsh   | `~/.zprofile`                | `shims`      | `eval "$(mise activate zsh --shims)"`  |
| `zshrc`        | zsh   | `~/.zshrc`                   | `activate`   | `eval "$(mise activate zsh)"`          |
| `zshenv`       | zsh   | `~/.zshenv`                  | `shims`      | `eval "$(mise activate zsh --shims)"`  |
| `fish`         | fish  | `~/.config/fish/config.fish` | `activate`   | `mise activate fish \| source`         |

The other mode adds or removes `--shims`. The `bash` and `zsh` shortcuts cover
the login and interactive startup files above. They never write `~/.zshenv`, which zsh reads
for every script; set `zshenv` explicitly if you want it.

Every key takes one of these values:

| Value                                | Effect                                                |
| ------------------------------------ | ----------------------------------------------------- |
| `true`                               | Enable with the default mode                          |
| `false`                              | Disable, even when a broader config enables it        |
| `"activate"` or `"shims"`            | Enable with that mode; on a shell key, for every file |
| `{ enabled = true, mode = "shims" }` | The same settings as a table; `enabled` is required   |

For example, this puts shims in both zsh files:

```toml
[bootstrap.mise_shell_activate]
zsh = "shims"
```

Within one config file, shell keys apply before file keys, so `zsh = true` with
`zshrc = false` writes only `~/.zprofile`. Across config files, the most local
value for each file wins, so a project config can turn off one file that your
global config enables without changing the others.

These files are fixed paths in your home directory and ignore `ZDOTDIR`. If you
set `ZDOTDIR`, set `zsh = false` and add the blocks with
[`[dotfiles]` edits](/dotfiles/edits.html) on the files in that directory:

```toml
[dotfiles]
"~/.config/zsh/.zprofile/activate" = { block = 'eval "$(mise activate zsh --shims)"' }
"~/.config/zsh/.zshrc/activate" = { block = 'eval "$(mise activate zsh)"' }
```

Only bash, zsh, and fish are supported here. For PowerShell, Nushell, or another
shell, add its `mise activate` line from [Shell setup](/shell-setup.html) with a
`[dotfiles]` edit.

## Existing startup files

mise owns only the lines between its markers and leaves the rest of the file
alone. The markers are the same ones [`[dotfiles]` edits](/dotfiles/edits.html)
use:

```sh
# >>> mise:activate >>> managed by mise — do not edit between markers
eval "$(mise activate zsh)"
# <<< mise:activate <<<
```

If the file already has an `eval "$(mise activate zsh)"` line outside the
markers, delete it, or activation runs twice. After applying, open a new shell
and run `mise doctor`: editing a startup file does not change a shell that is
already running. mise edits these files only when you run `mise bootstrap` or
`mise bootstrap mise-shell-activate apply`.

mise skips its own block for a file, without a warning, when `[dotfiles]`
already covers it:

- An entry that deploys the whole file to the same path.
- An edit with the id `activate` on the same path, such as
  `"~/.zshrc/activate"`.

A file tracked with `mode = "track"` or `mode = "track-local"` still gets the
block, because tracking records the file's history without writing it. To write
a custom block, use a `[dotfiles]` edit like the `ZDOTDIR` example above.

## Preview and apply

```sh
mise bootstrap mise-shell-activate status            # show each block's state
mise bootstrap mise-shell-activate status --missing  # exit 1 if a block is missing or differs
mise bootstrap mise-shell-activate apply --dry-run   # print the edits
mise bootstrap mise-shell-activate apply --yes       # apply without a prompt
```

`mise bootstrap shell` is a shorter alias. `status --json` returns one entry per
file with `target`, `shell`, `path`, `mode`, and `state`. The state is
`missing`, `applied`, or `differs`, and a `differs` entry also has a `reason`.

## Set your login shell {#set-your-login-shell}

`[bootstrap.user]` sets the current user's login shell. This installs fish with
Homebrew and makes it the login shell on an Apple Silicon Mac:

```toml
[bootstrap.packages]
"brew:fish" = "latest"

[bootstrap.user]
login_shell = "/opt/homebrew/bin/fish"
```

`mise bootstrap` installs packages before it changes the login shell, so one run
does both. When you run `mise bootstrap user apply` on its own, install the
shell first. The path must be absolute, such as `/bin/zsh`; mise ignores a
relative name with a warning.

Many systems accept only shells listed in `/etc/shells`, so mise adds the path
there when it is missing. Then, if the account's shell differs, mise runs
`chsh -s /opt/homebrew/bin/fish`, which may ask for your password.

`/etc/shells` is usually owned by root. When mise cannot write it directly, it
uses sudo, which can prompt for a password in an interactive terminal; otherwise
mise needs passwordless sudo, and it never elevates when
[`system_packages.sudo`](/configuration/settings.html#system_packages.sudo) is
`false`.

This changes only the account's login shell. It does not affect the shell you
are in, so `mise bootstrap` ends with a follow-up reminding you to start a new
login session. It also does not set up [activation](#activate-mise-in-your-shell)
in the new shell.

A project config can override a global `login_shell`; the most local value
wins. mise changes the login shell only when you run `mise bootstrap` or
`mise bootstrap user apply`. When mise itself runs under `sudo`, it checks and
changes the shell of `SUDO_USER` rather than root, while a plain root session,
such as a container, changes root's shell. On Windows, or where `chsh` is
missing, mise skips the setting and `status` reports it as skipped.

```sh
mise bootstrap user status            # show the login shell state
mise bootstrap user status --missing  # exit 1 if the shell differs or is not listed
mise bootstrap user apply --dry-run   # print the commands instead
mise bootstrap user apply --yes       # apply without a prompt
```

## See also

- [Bootstrap](/bootstrap.html#how-it-runs) for where these parts fall in the
  run order.
- [Shell setup](/shell-setup.html) for activating mise by hand and for shell
  completions.
- [Edit part of a file](/dotfiles/edits.html) for custom blocks and lines.
