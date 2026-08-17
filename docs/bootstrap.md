# Bootstrap

`mise bootstrap` sets up a machine for the current config in one command: Linux
users and groups, OS packages, privileged files and directories, system services,
Linux host firewall policy, Docker Compose projects, git repos, dotfiles, mise shell
activation, macOS defaults, macOS LaunchAgents, Linux systemd user services,
the user's login shell, tools, and any final project-specific task. It can
consume declared secret inputs without storing their values in mise config. You
can also add hooks that run at named points in the bootstrap sequence.

The same configuration can be applied to named inventory hosts or ad-hoc SSH
destinations with [`mise bootstrap remote`](/bootstrap/remote.html).

Use bootstrap for things that are needed before a project or workstation is
ready, but that do not belong in `[tools]`: native libraries, Homebrew
formulae, dotfile repositories, shell rc files, editor config, macOS
preferences, user services, and one-time machine setup.

## Composing configuration roots

`[bootstrap].config_roots` composes declarative resources from independent
configuration roots into the current bootstrap operation:

```toml
[bootstrap]
config_roots = ["bundles/*"]
```

Entries are relative to the declaring config root and may use single-level `*`
globs. Each matched directory is loaded with the normal active configuration
environments. Relative resource sources and template `config_root` values remain
relative to the config that declared them.

Composition includes `[dotfiles]`, `[bootstrap.files]`, and
`[bootstrap.directories]`. Equivalent declarations are deduplicated. Different
declarations for the same dotfile target, edit `(path, id)`, managed file, or
managed directory are errors that identify both declaring configs. Independent
roots never acquire precedence from their order in `config_roots`.

Other configuration such as tools, tasks, packages, services, hooks, and repos
is not collected from these roots. Use their existing explicit workflows when
those resources need aggregate behavior.

## How it runs

`mise bootstrap` runs these steps in order:

Before making changes, mise resolves any required
[`[bootstrap.secrets]`](/bootstrap/secrets.html) used by the files phase. This
preflight prevents a missing input from leaving a partially provisioned host.

1. `mise bootstrap accounts apply` converges
   [`[bootstrap.users]` and `[bootstrap.groups]`](/bootstrap/accounts.html).
2. `mise bootstrap plugins apply` installs package manager plugins declared in
   [`[bootstrap.plugins]`](/bootstrap/packages/plugins.html).
3. Built-in managers install missing [`[bootstrap.packages]`](/bootstrap/packages/).
4. `mise bootstrap files apply` converges
   [`[bootstrap.files]` and `[bootstrap.directories]`](/bootstrap/files.html).
5. `mise bootstrap services apply` converges existing systemd system units from
   [`[bootstrap.services]`](/bootstrap/services.html).
6. `mise bootstrap firewall apply` converges host firewall policy and rules from
   [`[bootstrap.linux.firewall]`](/bootstrap/firewall.html).
7. `mise bootstrap compose apply` converges
   [`[bootstrap.compose]`](/bootstrap/compose.html) projects.
8. `mise bootstrap repos apply` clones or updates
   [`[bootstrap.repos]`](/bootstrap/repos.html).
9. `mise bootstrap dotfiles apply` applies [`[dotfiles]`](/dotfiles.html).
10. `mise bootstrap mise-shell-activate apply` configures shell activation from
    [`[bootstrap.mise_shell_activate]`](/bootstrap/shell.html).
11. `mise bootstrap macos defaults apply` writes
    [`[bootstrap.macos.defaults]`](/bootstrap/macos-defaults.html).
12. `mise bootstrap macos launchd-agents apply` writes and loads
    [`[bootstrap.macos.launchd.agents]`](/bootstrap/launchd.html).
13. `mise bootstrap linux systemd-units apply` converges
    [`[bootstrap.linux.systemd.units]`](/bootstrap/systemd.html)
    by writing unit files, enabling/disabling them, and starting/stopping them
    as configured.
14. `mise bootstrap user apply` applies [`[bootstrap.user]`](/bootstrap/user.html).
15. `mise install` installs missing `[tools]`.
16. Plugin package managers apply after their host tools are available.
17. `mise run bootstrap` runs a task named `bootstrap`, if one exists.
18. `[bootstrap.hooks.final]` runs after the bootstrap task, if configured.

Use `mise bootstrap --skip <part>` to skip specific parts. Supported parts are
`accounts`, `plugins`, `packages`, `files`, `services`, `firewall`, `compose`, `repos`, `dotfiles`, `mise-shell-activate`,
`macos-defaults`, `macos-launchd-agents`, `linux-systemd-units`, `user`, `tools`,
`task`, and `final-hook`. The old shorter names `shell`, `defaults`, `launchd`,
and `systemd` are still accepted as aliases. The flag can be repeated or
comma-separated, for example `mise bootstrap --skip tools,task`.

Use `mise bootstrap --only <part>` to run only specific parts. It supports the
same part names and can be repeated or comma-separated, for example
`mise bootstrap --only dotfiles,tools`. `--only` and `--skip` are mutually
exclusive.

Use `mise bootstrap --update` to refresh system package manager metadata
before installing packages (apk: `--update-cache`, apt: `apt-get update`).

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

[bootstrap.secrets]
service_token = "EXAMPLE_SERVICE_TOKEN"

[bootstrap.groups.example]
system = true

[bootstrap.users.example]
system = true
group = "example"
home = "/var/lib/example"
create_home = true

[bootstrap.directories."/opt/example"]
owner = "root"
group = "root"
mode = "0755"

[bootstrap.files."/etc/example.conf"]
content = 'token={{ secret(name="service_token") }}'
template = true
owner = "root"
group = "root"
mode = "0644"
notify = ["example"]

[bootstrap.services.example]
state = "running"
enabled = true
on_change = "reload_or_restart"

[bootstrap.linux.firewall]
backend = "auto"
state = "enabled"
default_incoming = "deny"
default_outgoing = "allow"

[[bootstrap.linux.firewall.rules]]
name = "https"
port = 443
protocol = "tcp"
action = "allow"

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

Then converge the whole machine (`--yes` skips the confirmation prompts):

```sh
mise bootstrap --yes
```

To preview what would change without touching anything:

```sh
mise bootstrap --dry-run
```

For a structured resource plan, use `mise bootstrap plan`. The provisioning
planner reports accounts, system packages, privileged files and directories,
system services, firewall policy and rules, and Compose projects in dependency order. Other declarative
bootstrap parts will join the same graph as they adopt the resource model.

```sh
mise bootstrap plan
mise bootstrap plan --json
mise bootstrap plan --detailed-exitcode
```

With `--detailed-exitcode`, the command exits 0 when nothing would change, 2
when the plan contains changes, and 1 when planning fails or any resource has
an `unknown` state. Unknown resources do not count as changes, but they block a
successful convergence result. A package is unknown when its manager is
unavailable on the current platform or cannot install the requested version.
This matches apply behavior: unsupported pins remain visible for manual
resolution instead of being reported as changes mise would skip.

When `mise bootstrap` applies or would apply something that needs user
follow-up, it prints a final `bootstrap: follow-up` section after a successful
run. Dry runs use `bootstrap: follow-up if applied`. If a later bootstrap phase
fails after earlier phases already produced follow-up items, mise prints those
items before returning the error. The section is omitted when there is nothing
actionable to report.

By default, bootstrap refuses dotfile conflicts rather than replacing local
files. Use `mise bootstrap --force-dotfiles` when you explicitly want the
dotfiles phase to replace conflicting whole-file dotfile targets.

## Inspecting state

Use `mise bootstrap status` to inspect the declarative bootstrap state in one
place. It reports every declarative part — packages, repos, dotfiles, shell
activation, macOS defaults, LaunchAgents, systemd units, and login shell —
plus `[tools]` and any system dependencies that installed tools require:

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
mise bootstrap firewall status
mise bootstrap user status
```

`mise bootstrap status --missing` checks the whole declarative bootstrap
surface in one command. The narrower `mise bootstrap packages status
--missing` and `mise bootstrap dotfiles status --missing` commands are useful when you
only want to check one part without installing anything.

## What goes where

| Config                                                         | Use for                                                       |
| -------------------------------------------------------------- | ------------------------------------------------------------- |
| [`[bootstrap.packages]`](/bootstrap/packages/)                 | OS packages from apk, apt, dnf, pacman, brew, flatpak, or mas |
| [`[bootstrap.repos]`](/bootstrap/repos.html)                   | Git repos cloned before dotfiles are applied                  |
| [`[dotfiles]`](/dotfiles.html)                                 | Whole-file dotfiles and small managed edits to existing files |
| [`[bootstrap.mise_shell_activate]`](/bootstrap/shell.html)     | mise activation snippets in shell startup files               |
| [`[bootstrap.macos.*]`](/bootstrap/macos-defaults.html)        | Curated macOS preferences for Dock/Finder/keyboard/trackpad   |
| [`[bootstrap.macos.defaults]`](/bootstrap/macos-defaults.html) | macOS user preferences written through `defaults write`       |
| [`[bootstrap.macos.launchd.agents]`](/bootstrap/launchd.html)  | macOS user LaunchAgents written and loaded with `launchctl`   |
| [`[bootstrap.linux.systemd.units]`](/bootstrap/systemd.html)   | Linux systemd user services managed with `systemctl --user`   |
| [`[bootstrap.linux.firewall]`](/bootstrap/firewall.html)       | Linux host firewall policy and managed rules                  |
| [`[bootstrap.user]`](/bootstrap/user.html)                     | Current-user settings such as `login_shell`                   |
| `[bootstrap.hooks]`                                            | Commands that run at named bootstrap phases                   |
| `[tools]`                                                      | Versioned dev tools managed by mise                           |
| `[tasks.bootstrap]`                                            | Anything custom that should run after tools are installed     |

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
The `pre-dotfiles` and `post-dotfiles` phases also wrap
`mise bootstrap dotfiles apply`.

## Common workflows

### New machine

```sh
mise trust
mise bootstrap --yes
```

### Add a package

```sh
mise bootstrap packages use apk:zlib-dev apt:libssl-dev
```

This writes `[bootstrap.packages]` and installs what is missing.

### Capture an edited dotfile

```sh
$EDITOR ~/.zshrc
mise bootstrap dotfiles add ~/.zshrc
```

`mise bootstrap dotfiles add` stores the live file under `dotfiles.root` and writes an
explicit `[dotfiles]` entry with `mode`.

### Edit a managed dotfile

```sh
mise bootstrap dotfiles edit ~/.zshrc
mise bootstrap dotfiles apply ~/.zshrc
```

For symlinked dotfiles, `edit` opens the managed source, so it works with the
default `symlink` mode.

## Advanced: self-managing config

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
