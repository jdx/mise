# Set up a machine with mise

The recommended way to run mise on a workstation: keep editing your
configuration files where they are, let mise save every change and share the
ones you choose, and recreate the whole setup on the next machine with one
command. Symlink, copy, and template workflows keep working as before and mix
with this freely; tracking files in place is simply the default worth reaching
for first.

This page is one path through the whole thing. Each step shows the commands
and what they print (identifiers and times differ on your machine); the
details live in [dotfiles](/dotfiles.html), [history](/history.html),
[services](/bootstrap/services.html), and [remote bootstrap](/bootstrap/remote.html).

## 1. Install mise and sign in to your repository host

```sh
curl https://mise.run | sh
export PATH="$HOME/.local/bin:$PATH"
mise use -g gh
gh auth login --hostname github.com --git-protocol https --web
gh auth setup-git --hostname github.com
```

`gh auth setup-git` makes git use the GitHub CLI as its credential helper,
so background synchronization can reach a private repository without a
terminal. An SSH remote works too when the history watcher can reach an
agent or an unencrypted key.

## 2. Track a file you already have

```sh
$ mise bootstrap dotfiles track ~/.zshrc ~/.config/hypr
mise history: saved baseline checkpoint 1
mise WARN automatic capture is inactive: declare `[bootstrap.services.mise-history] builtin = "history-watch"` and run `mise bootstrap`; until then edits are saved by `mise bootstrap dotfiles save` or `mise bootstrap dotfiles watch --once`
```

The files stay exactly where they are. The declaration is one line per
destination in `~/.config/mise/config.toml`:

```toml
[dotfiles]
"~/.zshrc" = { mode = "track" }
"~/.config/hypr" = { mode = "track" }
```

## 3. Let a service save your edits

```toml
[bootstrap.services.mise-history]
builtin = "history-watch"
```

```sh
$ mise bootstrap
...
mise user services: applied mise-history
```

The same declaration installs a systemd user unit on Linux, a LaunchAgent
on macOS, and a Scheduled Task on Windows. `mise bootstrap dotfiles status`
reports `automatic capture: running`.

## 4. Connect a repository and pick a mode

```sh
$ mise bootstrap dotfiles origin set https://github.com/you/setup.git --name laptop
Setup repository: https://github.com/you/setup.git (branch main)
Machine: laptop (0192f3a4-…)
Sync mode sync: the watcher publishes saved changes and fetches periodically. Any conflict pauses publication and incoming application for the entire setup; local history, fetching, and eligible machine backups continue. Incoming changes are preflighted together and applied with a protective checkpoint and recovery journal. Applying never runs `mise bootstrap` or renders templates. Run `mise bootstrap` when the new declarations need to be applied.
The repository is empty: the first publication creates `main` with the mise marker.
Published from this machine:
  configuration: 1 file(s)
  tracked (home): 4 file(s)
Not shared:
  ~/.config/mise/config.local.toml: machine-local configuration (private unless explicitly overridden)
Machine backups: 5 of 6 captured file(s) are backed up in plain form under refs/mise-history/0192f3a4-…/ — anyone who can read this repository can read every file in these snapshots. The setup branch is always plaintext; use a private repository.
Existing checkpoints: not uploaded; only checkpoints from now on (pass --include-existing to upload them too)
Connect this setup repository? [y/N] y
mise history: connected https://github.com/you/setup.git (sync); [history.origin] written to ~/.config/mise/config.local.toml
mise history: published 3f2a1c9
```

`sync` is the default: the watcher publishes shortly after a save, fetches
periodically, and applies incoming changes with a protective checkpoint first.
Any conflict pauses publication and incoming application for the entire setup.
Local history, fetching, and eligible backups continue. `fetch-only` never publishes and never changes a file
until you run `pull`; `manual` does nothing on the network by itself
(`--sync <mode>` chooses; `mise settings set history.sync <mode>` changes it
later). `sync` and `pull` are different commands on purpose: `sync` moves
saved changes between the machine and the repository, `pull` writes what
arrived into your files; in `sync` mode the watcher does both. Everything
the disclosure lists leaves the machine in plain text, so use a private
repository.

## 5. Edit a file and look at its history

Edit `~/.config/hypr/bindings.lua` in your editor. Once it has been quiet
for two seconds the watcher saves it:

```sh
$ mise bootstrap dotfiles history --path ~/.config/hypr/bindings.lua
ID  When              Trigger   Description                                Files
7   2026-09-06 09:41  edit      edited hypr/bindings.lua                   1
4   2026-09-05 18:02  edit      edited hypr/bindings.lua, hypr/monitors.lua 2
1   2026-09-03 08:15  baseline  tracked ~/.zshrc, ~/.config/hypr           5
$ mise bootstrap dotfiles history diff 4 --path ~/.config/hypr/bindings.lua --patch
-bind = SUPER, Q, exec, kitty
+bind = SUPER, Q, exec, alacritty
```

A file that keeps changing (a state file an application rewrites every
second) is saved ever more rarely rather than flooding the history, and
never delays the others; `mise bootstrap dotfiles paths --noisy` names it,
and `mise bootstrap dotfiles exclude '<glob>'` leaves it out.

## 6. Restore one file

```sh
$ mise bootstrap dotfiles rollback ~/.config/hypr/bindings.lua
Path                         Action  From          To
~/.config/hypr/bindings.lua  write   file 9c1e2d4  file 51ab7f0
history: apply this plan? [y/N] y
mise history: rolled back ~/.config/hypr/bindings.lua to 4
$ mise bootstrap dotfiles undo
```

A rollback saves a protective checkpoint first, touches only the paths in
its plan, and is a new change like any other: with a repository connected
it is published and other machines receive it; `undo` reverses it.

## 7. Resolve a conflict

Two machines edited the same lines of `~/.zshrc` before either synced. Both
versions are kept, the file on each machine is untouched, and sharing pauses
for the entire setup until the conflict is decided; local history, fetching,
and eligible machine backups continue:

```sh
$ mise bootstrap dotfiles status
...
Setup repository: https://github.com/you/setup.git (branch main, mode sync, machine desktop).
  last publish 2026-09-06 09:41, last fetch 2026-09-06 09:55, last pull 2026-09-06 09:41; 0 checkpoint(s) pending upload.
  sync paused: ~/.zshrc: both sides changed the same lines; sharing is paused for the entire setup; local history continues (`mise bootstrap dotfiles pull --take-remote|--keep-local ~/.zshrc`)
$ mise bootstrap dotfiles pull --take-remote ~/.zshrc
```

`--keep-local` selects this machine's version instead. Choices are recorded
per file, but publication and incoming application wait until every conflict
is resolved and the current versions have been rechecked. Local history,
fetching, and eligible machine backups continue while sharing is paused.
`mise doctor` names the blocking paths too. Desktop notifications are enabled
by default on Linux and macOS, once per pause; set
`settings.history.notify = false` to opt out.

## 8. Set up the next machine

```sh
curl https://mise.run | sh
gh auth login --hostname github.com --git-protocol https --web
gh auth setup-git --hostname github.com
mise bootstrap --from-git you/setup
```

```
https://github.com/you/setup.git is a mise setup repository (format 1); branch main.
Sync mode sync: ...
Path                         Action  Group
~/.config/mise/config.toml   create  configuration
~/.zshrc                     create  tracked/home/.zshrc
~/.config/hypr/bindings.lua  create  tracked/home/.config/hypr/bindings.lua
Set this machine up from the repository? [y/N] y
Wrote 3 file(s) from https://github.com/you/setup.git.
...
mise user services: applied mise-history
```

The branch goes into mise's own store (nothing is cloned into
`~/.config/mise`), the configuration and the tracked files are written by a
recoverable pull, the watcher service comes up with the rest of the
bootstrap, and from then on the machine syncs like the first one. A file
that already exists and differs is held for a decision; when that file is
the configuration itself, the command stops there and names the `pull
--take-remote|--keep-local` that decides it. Over SSH:

```sh
mise bootstrap remote --host devbox --install-mise --from-git you/setup \
  --github-relay-read-only --github-relay-repo you/setup
```

The borrowed GitHub access is read-only and ends with the session; give the
host credentials of its own (step 1, there) for ongoing synchronization.
`--dry-run` shows the same plan on the host without writing anything there.

## Templates next to tracked files

A tracked file is shared as it is. A file that must differ per machine is
rendered from a template source, and only the source is shared:

```toml
[vars]
email = "you@example.com"

[dotfiles]
"~/.gitconfig" = { source = "templates/gitconfig.tera", mode = "template" }
```

`~/.config/mise/templates/gitconfig.tera` (the source) is captured, shared,
and set up on the next machine like any configuration file. `~/.gitconfig`
(the output) is rendered by `mise bootstrap` with this machine's `[vars]`,
kept in local history so an edit to it can be recovered, and never shared as
its own file.

## Platform notes

- **Omarchy**: track `~/.config/hypr`, `~/.config/omarchy`, and
  `~/.config/waybar`; leave the git clones under `~/.config/omarchy/themes`
  and `~/.config/omarchy/plugins` to Omarchy (they are nested repositories
  and are never descended into); exclude `~/.config/omarchy/backgrounds`.
  `omarchy-update` keeps running as it does; add
  `mise bootstrap dotfiles save --best-effort` before it to mark the state it
  starts from.
- **Ubuntu**: the watcher is a systemd user unit
  (`systemctl --user status mise-history`); `apt` packages go in
  `[bootstrap.packages]` and are installed by `mise bootstrap` like tools.
- **macOS**: the watcher is a LaunchAgent (`launchctl list | grep dev.mise`);
  a tracked file can carry a macOS variant
  (`track ~/.zshrc --os macos`) that evolves apart from the Linux one.
- **Windows**: tracking, checkpoints, rollback, sync, and the Scheduled Task
  watcher work; symlink modes need Developer Mode, copy and template modes
  do not. Desktop notifications are Linux and macOS only for now: on Windows,
  `mise bootstrap dotfiles status` and `mise doctor` show a paused setup.
