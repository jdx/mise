# Bootstrap <Badge type="warning" text="experimental" />

packages, git repos, dotfiles, mise shell activation, macOS defaults, macOS
LaunchAgents, Linux systemd user services, the user's login shell, tools, and
any final project-specific task. You can also add hooks that run at named points
in the bootstrap sequence.

Use bootstrap for things that are needed before a project or workstation is
ready, but that do not belong in `[tools]`: native libraries, Homebrew
formulae, dotfile repositories, shell rc files, editor config, macOS
preferences, user services, and one-time machine setup.

## How it runs

`mise bootstrap` runs these steps in order:

1. `mise bootstrap packages apply` installs missing `[bootstrap.packages]`.
2. `mise bootstrap repos apply` clones or updates `[bootstrap.repos]`.
3. `mise bootstrap dotfiles apply` applies `[dotfiles]`.
4. `mise bootstrap mise-shell-activate apply` configures shell activation from
   `[bootstrap.mise_shell_activate]`.
5. `mise bootstrap macos defaults apply` writes `[bootstrap.macos.defaults]`.
6. `mise bootstrap macos launchd-agents apply` writes and loads
   `[bootstrap.macos.launchd.agents]`.
7. `mise bootstrap linux systemd-units apply` converges
   `[bootstrap.linux.systemd.units]`
   by writing unit files, enabling/disabling them, and starting/stopping them
   as configured.
8. `mise bootstrap user apply` applies `[bootstrap.user]`.
9. `mise install` installs missing `[tools]`.
10. `mise run bootstrap` runs a task named `bootstrap`, if one exists.
11. `[bootstrap.hooks.final]` runs after the bootstrap task, if configured.

Use `mise bootstrap --skip <part>` to skip specific parts. Supported parts are
`packages`, `repos`, `dotfiles`, `mise-shell-activate`, `macos-defaults`,
`macos-launchd-agents`, `linux-systemd-units`, `user`, `tools`, `task`, and
`final-hook`. The old shorter names `shell`, `defaults`, `launchd`, and
`systemd` are still accepted as aliases. The flag can be repeated or
comma-separated, for example `mise bootstrap --skip tools,task`.

Use `mise bootstrap --only <part>` to run only specific parts. It supports the
same part names and can be repeated or comma-separated, for example
`mise bootstrap --only dotfiles,tools`. `--only` and `--skip` are mutually
exclusive.

Hook phases can also run before and after the built-in steps:
`pre-packages`, `post-packages`, `pre-repos`, `post-repos`, `pre-dotfiles`,
`post-dotfiles`, `pre-defaults`, `post-defaults`, `pre-user`, `post-user`,
`pre-tools`, and `post-tools`.

The declarative steps converge: if a package is already installed, a repo is
already at the requested ref, a dotfile already matches, or a default is already
set, mise skips it. The `bootstrap` task runs every time, so keep it idempotent.

## Example

```toml
[bootstrap.packages]
"apk:build-base" = "latest"
"apt:build-essential" = "latest"
"brew:postgresql@17" = "latest"

[bootstrap.repos]
"~/src/dotfiles" = { url = "git@github.com:jdx/dotfiles.git", ref = "main" }

[dotfiles]
"~/.gitconfig" = { mode = "symlink" }
"~/.config/nvim" = { mode = "symlink" }

[bootstrap.mise_shell_activate]
zprofile = "shims"
zshrc = "activate"
fish = "activate"

[bootstrap.macos.dock]
autohide = true
orientation = "left"
tilesize = 48

[bootstrap.macos.finder]
show_pathbar = true

[bootstrap.macos.keyboard]
key_repeat = 2
initial_key_repeat = 15

[bootstrap.macos.trackpad]
tap_to_click = true

[bootstrap.macos.defaults]
"com.apple.finder" = { AppleShowAllFiles = true }

[bootstrap.macos.launchd.agents.my-sync]
program = "~/.local/bin/my-sync"
args = ["--watch"]
run_at_load = true

[bootstrap.linux.systemd.units.my-sync]
description = "sync files"
exec_start = "~/.local/bin/my-sync --watch"
restart = "on-failure"

[bootstrap.user]
login_shell = "/bin/zsh"

[bootstrap.hooks.pre-packages]
run = "softwareupdate --install-rosetta --agree-to-license"

[bootstrap.hooks.post-defaults]
run = "killall Dock || true"

[tools]
node = "lts"
python = "3.12"

[tasks.bootstrap]
run = "gh auth status || gh auth login"
```

Then run:

```sh
mise bootstrap --yes
```

For a dry run:

```sh
mise bootstrap --dry-run
```

When `mise bootstrap` applies or would apply something that needs user
follow-up, it prints a final `bootstrap: follow-up` section after a successful
run. Dry runs use `bootstrap: follow-up if applied`. If a later bootstrap phase
fails after earlier phases already produced follow-up items, mise prints those
items before returning the error. The section is omitted when there is nothing
actionable to report.

By default, bootstrap refuses dotfile conflicts rather than replacing local
files. Use `mise bootstrap --force-dotfiles` when you explicitly want the
dotfiles phase to replace conflicting whole-file dotfile targets.

## Inspecting State

Use `mise bootstrap status` to inspect the declarative bootstrap state in one
place:

```sh
mise bootstrap status
mise bootstrap status --json
mise bootstrap status --missing
mise bootstrap packages status
mise bootstrap repos status
mise bootstrap dotfiles status
mise bootstrap dotfiles apply --dry-run
mise bootstrap dotfiles apply --dry-run --verbose
mise bootstrap mise-shell-activate status
mise bootstrap macos defaults status
mise bootstrap macos launchd-agents status
mise bootstrap linux systemd-units status
mise bootstrap user status
```

`mise bootstrap status --missing` checks the whole declarative bootstrap
surface in one command. The narrower `mise bootstrap packages status
--missing` and `mise bootstrap dotfiles status --missing` commands are useful when you
only want to check one part without installing anything.

## What Goes Where

| Config                             | Use for                                                       |
| ---------------------------------- | ------------------------------------------------------------- |
| `[bootstrap.packages]`             | OS packages from apk, apt, dnf, pacman, or brew               |
| `[bootstrap.repos]`                | Git repos cloned before dotfiles are applied                  |
| `[dotfiles]`                       | Whole-file dotfiles and small managed edits to existing files |
| `[bootstrap.mise_shell_activate]`  | mise activation snippets in shell startup files               |
| `[bootstrap.macos.*]`              | Curated macOS preferences for Dock/Finder/keyboard/trackpad   |
| `[bootstrap.macos.defaults]`       | macOS user preferences written through `defaults write`       |
| `[bootstrap.macos.launchd.agents]` | macOS user LaunchAgents written and loaded with `launchctl`   |
| `[bootstrap.linux.systemd.units]`  | Linux systemd user services managed with `systemctl --user`   |
| `[bootstrap.user]`                 | Current-user settings such as `login_shell`                   |
| `[bootstrap.hooks]`                | Commands that run at named bootstrap phases                   |
| `[tools]`                          | Versioned dev tools managed by mise                           |
| `[tasks.bootstrap]`                | Anything custom that should run after tools are installed     |

Use declarative sections when mise can inspect and converge the state. Use
`[tasks.bootstrap]` for imperative setup that does not fit those sections,
such as running an auth flow, seeding local data, or other one-off project
setup.

## Hooks

Hooks run only during explicit `mise bootstrap` invocations. A hook can be
specified as a command string, an array of command strings, or a table with a
`run` field. They use the same default inline shell setting as tasks, stop the
bootstrap if they fail, and print the command instead of running it during
`mise bootstrap --dry-run`. Hooks run in the current process environment; use
`mise exec -- ...` inside a hook, or use `[tasks.bootstrap]`, when the command
needs tools from `[tools]` on PATH.

```toml
[bootstrap.hooks.pre-packages]
run = "softwareupdate --install-rosetta --agree-to-license"

[bootstrap.hooks.post-tools]
run = [
  "mise exec -- corepack enable",
  "mise exec -- rustup component add rustfmt clippy",
]

[bootstrap.hooks.final]
run = "gh auth status || gh auth login"
```

As shorthand, a hook phase can also be set directly:

```toml
[bootstrap.hooks]
post-defaults = "killall Dock || true"
```

Hooks merge across the config hierarchy from global to local, so shared config
can define broad machine setup while a project adds its own phase commands.

## Common Workflows

### New Machine

```sh
mise trust
mise bootstrap --yes
```

### Add A Package

```sh
mise bootstrap packages use apk:zlib-dev apt:libssl-dev
```

This writes `[bootstrap.packages]` and installs what is missing.

### Capture An Edited Dotfile

```sh
$EDITOR ~/.zshrc
mise dotfiles add ~/.zshrc
```

`mise dotfiles add` stores the live file under `dotfiles.root` and writes an
explicit `[dotfiles]` entry with `mode`.

### Edit A Managed Dotfile

```sh
mise dotfiles edit ~/.zshrc
mise dotfiles apply ~/.zshrc
```

For symlinked dotfiles, `edit` opens the managed source, so it works with the
default `symlink` mode.

## Advanced: Self-Managing Config

You can manage the dotfiles repository and the mise global config as
dotfiles:

```toml
[settings]
dotfiles.root = "~/.dotfiles"

[dotfiles]
"~/.dotfiles" = "~/src/dotfiles"
"~/.config/mise/config.toml" = "~/src/dotfiles/mise/config.toml"
```

The repo/source must exist before the first apply. Use the real repo path for
sources needed during the first run; `~/.dotfiles` does not exist until mise
creates that symlink. Replacing the active global config affects future mise
invocations, so use this pattern carefully.
