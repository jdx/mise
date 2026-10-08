---
description: "Track your dotfiles, declare tools and packages, and bring a second machine to the same setup through a private Git repository."
socialDescription: "Track dotfiles, declare tools and packages, and set up a second machine from Git."
---

# Set up a machine

This tutorial takes one machine from a single tracked file to a declared setup,
then brings a second machine to the same state with one command. You keep
editing your files where they are. mise saves their history locally and shares
it through a private Git repository.

You need Git on each machine (on macOS, the Xcode Command Line Tools) and an
empty private repository on GitHub or another Git host. The examples use zsh;
[Shell activation](/bootstrap/shell.html) has the Bash and fish equivalents.

## On your first machine

### Install mise

Skip this step if `mise --version` already works.

```sh
curl -fsSL https://mise.run | sh
export PATH="$HOME/.local/bin:$PATH"
```

The installer puts `mise` in `~/.local/bin`. The `export` line affects only the
current shell; a later step puts that directory on `PATH` for new shells. See
[Installing mise](/installing-mise.html) for other methods.

### Track a file

Track your shell's startup file:

```sh
mise dot track ~/.zshrc
```

Pick a file that already exists. If you use Bash, track `~/.bashrc` instead;
the rest of this tutorial uses `~/.zshrc`.

Tracking saves the file's current contents as a checkpoint, a version you can
restore later. The file stays where it is, and mise adds this entry to
`~/.config/mise/config.toml`:

```toml
[dotfiles]
"~/.zshrc" = { mode = "track" }
```

### Save edits automatically

Add this table to `~/.config/mise/config.toml`:

```toml
[bootstrap.services.mise-history]
builtin = "history-watch"
```

Install the service and check that it is running:

```sh
mise bootstrap services apply
mise dot status
```

The watcher saves your edits to local Git history, kept apart from the files
themselves. It runs as a systemd user service on Linux, a LaunchAgent on macOS,
or a Scheduled Task on Windows. If you would rather save by hand, skip the
service and run `mise dot save` after editing.

### Declare tools, packages, and shell activation

Add the rest of this machine's setup to `~/.config/mise/config.toml`, merging
the `[dotfiles]` table with the one already there:

```toml
[tools]
node = "24"

[bootstrap.packages]
"brew:tmux" = { os = "macos" }
"apt:tmux" = "latest"

[dotfiles]
"~/.zshrc" = { mode = "track" }
"~/.zshenv/local-bin" = { line = 'export PATH="$HOME/.local/bin:$PATH"' }

[bootstrap.mise_shell_activate]
zprofile = "shims"
```

Each part does one job:

- `[tools]` installs Node.js with mise.
- `[bootstrap.packages]` installs tmux with Homebrew on macOS and with apt on
  Debian or Ubuntu. An entry whose package manager is missing on a machine is
  skipped, so one config can list both.
- The `[dotfiles]` line entry adds `~/.local/bin` to `PATH` in `~/.zshenv`,
  which zsh reads first, so new shells find `mise`. Leave it out if you
  installed mise with a package manager that puts it on `PATH`.
- `zprofile = "shims"` puts tool shims on `PATH` for login shells, including
  the ones editors start without a prompt.

For interactive shells, add `eval "$(mise activate zsh)"` to your tracked
`~/.zshrc` if it is not there yet. The tracked file carries the line to your
other machines. mise does not add its own activation block to a file you track,
which is why this config sets only `zprofile`; see
[Shell activation](/bootstrap/shell.html) for both modes.

Preview the changes, then apply them:

```sh
mise bootstrap --dry-run
mise bootstrap
```

Open a new shell and run `mise doctor` to check that activation works.

### Inspect and restore a change

Edit your tracked file, then save a checkpoint now so you can inspect it without
waiting for the watcher:

```sh
mise dot save ~/.zshrc
mise dot history --path ~/.zshrc
```

To see what a checkpoint changed, replace `CHECKPOINT_ID` with an ID from that
list:

```sh
mise dot history diff CHECKPOINT_ID --path ~/.zshrc --patch
```

To go back to the previous version:

```sh
mise dot rollback ~/.zshrc
```

Rollback picks the latest saved version that differs from the current file,
shows the change, and applies it. It saves the current contents first, so you
can reverse the rollback:

```sh
mise dot undo
```

A rollback is a new commit, so the versions you left behind stay in history.
Once you share your setup, the restored version reaches your other machines
too.

### Connect a private repository {#share-your-setup-optional}

To share your setup, connect a private Git repository, called an origin. First,
track your mise configuration, so the next machine also gets your tools,
packages, shell activation, and the watcher:

```sh
mise dot track ~/.config/mise/config.toml
```

Authenticate with your repository host. For GitHub:

```sh
mise use -g gh
mise exec gh -- gh auth login --hostname github.com --git-protocol https --web
mise exec gh -- gh auth setup-git --hostname github.com
```

The Git credential helper lets the watcher authenticate in the background
without a prompt. An SSH remote with credentials available to the watcher works
too.

::: warning Everything you saved gets shared
Connecting an origin pushes every saved version, including earlier edits. A
credential you later delete from a file stays in old commits. Set up
[encrypted files](/dotfiles/encryption.html) before a file's first save if it
needs encryption, and keep private decryption keys out of tracking.
:::

Replace `you/setup` with your repository and connect it:

```sh
mise dot origin set https://github.com/you/setup.git --sync sync
```

mise prints a preview of the connection, then connects. With the watcher
running, `--sync sync`
pushes your saved edits shortly after each save
([`history.sync_interval`](/configuration/settings.html#history.sync_interval))
and checks for changes from other machines on a schedule
([`history.fetch_interval`](/configuration/settings.html#history.fetch_interval)).
To exchange changes only when you ask, connect with `--sync manual` or
`--sync fetch-only` instead; see
[Choose a sync mode](/dotfiles/sync.html#choose-a-sync-mode).

## On a new machine {#set-up-another-machine}

### Install and authenticate

Install Git, then install mise and authenticate with your repository host:

```sh
curl -fsSL https://mise.run | sh
export PATH="$HOME/.local/bin:$PATH"
mise exec gh@latest -- gh auth login --hostname github.com --git-protocol https --web
mise exec gh@latest -- gh auth setup-git --hostname github.com
```

`mise exec gh@latest` runs `gh` without writing it to your global config, which
your shared configuration is about to provide.

### Adopt your setup

```sh
mise bootstrap --adopt you/setup
```

`you/setup` is shorthand for a GitHub repository. For another host, pass the
full URL, such as `git@gitea.example.com:you/setup.git`.

mise lists the files it will restore, restores them, and then runs bootstrap
from the restored configuration: it installs the packages
and tools, writes the `PATH` line and shell activation, and starts the watcher.
If the configuration or a template source it needs is missing from history,
bootstrap names the files to track and share from the first machine.

Put setup that files do not cover, such as installing shell plugins or fixing
permissions, in the shared configuration's `[tasks.bootstrap]`:

```toml
[tasks.bootstrap]
run = "install -d -m 700 ~/.ssh"
```

Adoption runs the task once the files are restored, and every later
`mise bootstrap` runs it again, so make it safe to repeat.

### If the machine already has these files {#when-the-machine-already-has-these-files}

A new machine often already has a `~/.zshrc`. mise never overwrites it during
adoption. Files that match the repository are accepted silently, but a file
that differs is held for a decision, and adoption stops before it bootstraps
anything:

```text
the setup from <url> is paused; nothing was bootstrapped.
```

List what is waiting, then take the repository's version of everything:

```sh
mise dot status
mise dot pull --take-remote-all
```

To make that choice up front, adopt with `--take-remote-all`:

```sh
mise bootstrap --adopt you/setup --take-remote-all
```

mise saves each version it replaces first, so `mise dot undo` reverses the whole
pull. Once the last file is decided, the same `pull` writes the remaining
tracked files. Then run `mise bootstrap` to finish the parts that are not
dotfiles, such as tools and services.

To decide one file at a time, name it:

```sh
mise dot pull --take-remote ~/.zshrc
```

To keep this machine's version of a file, save it first. `--keep-local`
publishes this machine's saved version, and a new machine has not saved one
yet:

```sh
mise dot save ~/.zshrc
mise dot pull --keep-local ~/.zshrc
```

`--keep-local` also names exceptions to a blanket choice. This takes the
repository's version of everything except `~/.zshrc`:

```sh
mise dot pull --take-remote-all --keep-local ~/.zshrc
```

Moving the conflicting files aside before `mise bootstrap --adopt` avoids these
decisions, because mise writes a missing file directly.

If this machine already saved history of its own, for example because it
tracked the same files before you shared them, adoption stops because the two
histories are unrelated. `--replace-history` discards this machine's history
and adopts the repository's. Files that differ still need a decision, so
combine it with `--take-remote-all`:

```sh
mise bootstrap --adopt you/setup --replace-history --take-remote-all
```

mise saves the versions it replaces first, on top of the adopted history, so
`mise dot undo` restores them. `--force-dotfiles` is unrelated: it applies to
`[dotfiles]` link and copy targets, not to shared history.

### Check the connection

```sh
mise dot status
```

The new machine shares changes in the mode set by
[`history.sync`](/configuration/settings.html#history.sync). From here, edits
on either machine reach the other. A pull restores files but runs no setup, so
after a shared change to tools, packages, services, or the `bootstrap` task, run
`mise bootstrap` to apply it. `mise dot status` reminds you until you do.

To set up a machine over SSH from this one, see
[Remote hosts](/bootstrap/remote.html#private-configuration-repositories). mise
fetches your private repository here, using this machine's Git credentials, and
sends it to the host. The host needs its own credentials only for later
synchronization.

## Add more files

Track configuration files you edit by hand. Preview a directory before you
track it with `mise dot track --dry-run <dir>`: themes, plugins, caches, and
application state usually do not belong in history. mise skips a nested Git
repository and records nothing for it; track its root as a separate entry, or
back it up separately. See
[nested repositories](/dotfiles/history.html#nested-repositories).

Before a system upgrade, save everything once:

```sh
mise dot save --best-effort
```

This saves your tracked dotfiles only. Use your operating system's backup tools
for packages and other system state.

To keep a different version of a file on another operating system, track it
for that OS, for example `mise dot track ~/.zshrc --os macos`. See
[History](/dotfiles/history.html) for variants, exclusions, and save options.

## Next steps

- If two machines change the same lines, mise pauses sharing and keeps both
  versions. `mise dot status` and `mise doctor` report it; follow
  [Resolve a conflict](/dotfiles/sync.html#resolve-a-conflict) to pick a
  version.
- To create a file from variables, such as a `~/.gitconfig` with a different
  email on each machine, use a template; see
  [Templates](/dotfiles/managed.html#templates).
- To give each machine a different combination of setup, see
  [Machine modules](/bootstrap/modules.html).
- For the watcher on other platforms and service management, see
  [Services](/bootstrap/services.html).
