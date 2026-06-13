# Bootstrap <Badge type="warning" text="experimental" />

`mise bootstrap` sets up the machine-level pieces around a mise config: OS
packages, dotfiles, macOS defaults, macOS LaunchAgents, the user's login
shell, tools, and any final project-specific task. You can also add hooks that
run at named points in the bootstrap sequence.

Use bootstrap for things that are needed before a project or workstation is
ready, but that do not belong in `[tools]`: native libraries, Homebrew
formulae, shell rc files, editor config, macOS preferences, and one-time
machine setup.

## How it runs

`mise bootstrap` runs these steps in order:

1. `mise bootstrap packages install` installs missing `[bootstrap.packages]`.
2. `mise dotfiles apply` applies `[dotfiles]`.
3. `mise bootstrap macos-defaults apply` writes `[bootstrap.macos.defaults]`.
4. `mise bootstrap launchd apply` writes and loads `[bootstrap.macos.launchd.agents]`.
5. `mise bootstrap user apply` applies `[bootstrap.user]`.
6. `mise install` installs missing `[tools]`.
7. `mise run bootstrap` runs a task named `bootstrap`, if one exists.
8. `[bootstrap.hooks.final]` runs after the bootstrap task, if configured.

Hook phases can also run before and after the built-in steps:
`pre-packages`, `post-packages`, `pre-dotfiles`, `post-dotfiles`,
`pre-defaults`, `post-defaults`, `pre-user`, `post-user`, `pre-tools`, and
`post-tools`.

The declarative steps converge: if a package is already installed, a dotfile
already matches, or a default is already set, mise skips it. The `bootstrap`
task runs every time, so keep it idempotent.

## Example

```toml
[bootstrap.packages]
"apt:build-essential" = "latest"
"brew:postgresql@17" = "latest"

[dotfiles]
"~/.gitconfig" = { mode = "symlink" }
"~/.config/nvim" = { mode = "symlink" }
"~/.zshrc/activate" = { block = 'eval "$(mise activate zsh)"' }

[bootstrap.macos.defaults]
"com.apple.dock" = { autohide = true }

[bootstrap.macos.launchd.agents.my-sync]
program = "~/.local/bin/my-sync"
args = ["--watch"]
run_at_load = true

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

## Inspecting State

Use the narrower commands when you want to inspect one part of the bootstrap
state:

```sh
mise bootstrap packages status
mise dotfiles status
mise dotfiles apply --dry-run
mise dotfiles apply --dry-run --verbose
mise bootstrap macos-defaults status
mise bootstrap launchd status
mise bootstrap user status
```

`mise bootstrap packages status --missing` and `mise dotfiles status
--missing` are useful CI checks when a repo expects machine setup to be in
place but should not install anything during that check.

## What Goes Where

| Config                             | Use for                                                       |
| ---------------------------------- | ------------------------------------------------------------- |
| `[bootstrap.packages]`             | OS packages from apt, dnf, pacman, or brew                    |
| `[dotfiles]`                       | Whole-file dotfiles and small managed edits to existing files |
| `[bootstrap.macos.defaults]`       | macOS user preferences written through `defaults write`       |
| `[bootstrap.macos.launchd.agents]` | macOS user LaunchAgents written and loaded with `launchctl`   |
| `[bootstrap.user]`                 | Current-user settings such as `login_shell`                   |
| `[bootstrap.hooks]`                | Commands that run at named bootstrap phases                   |
| `[tools]`                          | Versioned dev tools managed by mise                           |
| `[tasks.bootstrap]`                | Anything custom that should run after tools are installed     |

Use declarative sections when mise can inspect and converge the state. Use
`[tasks.bootstrap]` for imperative setup that does not fit those sections,
such as cloning a private repository, running an auth flow, or seeding local
data.

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
mise bootstrap packages use apt:libssl-dev
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
"~/.config/mise/config.toml" = "~/.dotfiles/mise/config.toml"
```

The repo/source must exist before the first apply. Replacing the active
global config affects future mise invocations, so use this pattern carefully.
