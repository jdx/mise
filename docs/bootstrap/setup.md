---
description: "Set up a machine with tracked dotfiles, local history, recovery, and optional synchronization."
---

# Set up a machine with mise

Keep editing your dotfiles where they are. This guide shows how to save local
history, restore a file, and optionally share your setup through a Git repository.
Start with one file; add more once you have tried restoring a change.

## Install mise

If mise is already installed, skip the first two commands.

```sh
curl https://mise.run | sh
export PATH="$HOME/.local/bin:$PATH"
```

## Track a file

On macOS with zsh:

```sh
mise bootstrap dotfiles track ~/.zshrc
```

On Omarchy with Bash, use `~/.bashrc` instead. Choose a file that already exists;
the remaining examples use `~/.zshrc`.

Tracking saves the file's current contents as a **checkpoint**, a version you
can restore later. It leaves the file in place and adds this entry to
`~/.config/mise/config.toml`:

```toml
[dotfiles]
"~/.zshrc" = { mode = "track" }
```

## Save edits automatically

Add this table to `~/.config/mise/config.toml`:

```toml
[bootstrap.services.mise-history]
builtin = "history-watch"
```

Install the service and check that it is running:

```sh
mise bootstrap services apply
mise bootstrap dotfiles status
```

The watcher saves edits to local Git history. It runs as a systemd user
service on Linux, a LaunchAgent on macOS, or a Scheduled Task on Windows.
The history repository is stored separately from the files you edit.

If you prefer to save manually, skip the service and run
`mise bootstrap dotfiles save` after editing.

## Inspect and restore a change

Edit your tracked file, then save a checkpoint now so you can inspect it
without waiting for the watcher:

```sh
mise bootstrap dotfiles save ~/.zshrc
mise bootstrap dotfiles history --path ~/.zshrc
```

To see a checkpoint's changes, replace `CHECKPOINT_ID` with an ID from that list:

```sh
mise bootstrap dotfiles history diff CHECKPOINT_ID --path ~/.zshrc --patch
```

To restore the previous version:

```sh
mise bootstrap dotfiles rollback ~/.zshrc
```

Rollback selects the latest saved version that differs from the current
file. Review the proposed changes before confirming. It saves the current
contents first, so you can reverse the rollback:

```sh
mise bootstrap dotfiles undo
```

A rollback creates a new commit, preserving the versions you left behind.
If you enable sharing below, the restored version can reach your other machines.

## Share your setup (optional)

To share changes between computers, connect a private Git repository, called
an **origin**. First, track your mise configuration so the next machine can
also install your tools and start the watcher:

```sh
mise bootstrap dotfiles track ~/.config/mise/config.toml
```

Create an empty private GitHub repository, then authenticate on this machine:

```sh
mise use -g gh
mise x gh -- gh auth login --hostname github.com --git-protocol https --web
mise x gh -- gh auth setup-git --hostname github.com
```

The credential helper lets background synchronization authenticate without an
interactive prompt. You can also use an SSH remote with credentials available
to the watcher.

Before connecting, review your tracked files and their history. Every saved
version will be shared, including earlier local edits. Deleting a credential
from a file later leaves it in old commits. For files that need encryption,
configure [encrypted tracking](/history.html#encrypted-shared-files) before
their first save and keep private decryption keys outside tracking.

Replace `you/setup` with your repository. This example enables automatic
sharing:

```sh
mise bootstrap dotfiles origin set https://github.com/you/setup.git --sync sync
```

Review the connection preview before confirming. With the watcher running,
saved edits are pushed within five minutes by default. It checks for changes
from other machines every fifteen minutes and applies them to your files.
After you set up a second machine below, edits can travel in both directions.

### Choose when to sync

Use `--sync manual` when connecting if you want to decide when to exchange
changes. You can change the mode later with
`mise settings set history.sync MODE`:

| Mode         | Watcher behavior                                                |
| ------------ | --------------------------------------------------------------- |
| `sync`       | Publishes saved changes, fetches, and applies incoming changes. |
| `manual`     | Saves locally; waits for you to run the network commands.       |
| `fetch-only` | Fetches remote changes for you to inspect and apply.            |

In manual mode, mise keeps saving locally. Run these commands to save your
latest edits, push all accumulated commits, fetch remote changes, and apply them:

```sh
mise bootstrap dotfiles save
mise bootstrap dotfiles sync
mise bootstrap dotfiles pull
```

These commands also work in automatic mode when you want to sync immediately.
When a shared change updates tools, services, or template sources, run
`mise bootstrap` to apply that configuration and render templates.

## Set up another machine

Install Git first. Then install mise and authenticate with the
same repository host:

```sh
curl https://mise.run | sh
export PATH="$HOME/.local/bin:$PATH"
mise use -g gh
mise x gh -- gh auth login --hostname github.com --git-protocol https --web
mise x gh -- gh auth setup-git --hostname github.com
mise bootstrap --adopt you/setup
```

Review the proposed files before confirming. Bootstrap restores the tracked
files and applies the saved mise configuration, including the watcher service.
If configuration or required template sources are missing from history,
bootstrap reports which files you need to track and share from the first machine.

If an existing file differs, follow the reported conflict instructions before
setup can continue. Enable automatic sharing on this machine and check its state:

```sh
mise settings set history.sync sync
mise bootstrap dotfiles status
```

Choose `manual` or `fetch-only` here if you want a different mode on this machine.

For setup over SSH, see [remote bootstrap](/bootstrap/remote.html). For a private GitHub repository, borrow read-only access from this machine:

```sh
mise bootstrap remote --host devbox --install-mise --adopt you/setup \
  --github-relay-read-only --github-relay-repo you/setup
```

GitHub access is borrowed read-only for this run. The target needs its own
credentials for ongoing synchronization.

## Resolve a conflict

If two machines change the same lines, mise keeps both versions and pauses
pushing and applying incoming changes for all tracked files. It continues
saving locally and fetching updates. Desktop notifications are enabled by
default on supported Linux and macOS installations. `status` and `mise doctor`
also report the pause, including on Windows or headless machines.

Inspect the conflict:

```sh
mise bootstrap dotfiles status
```

Choose the repository's version of a file:

```sh
mise bootstrap dotfiles pull --take-remote ~/.zshrc
```

Or keep this machine's version:

```sh
mise bootstrap dotfiles pull --keep-local ~/.zshrc
```

Resolve every reported conflict before sharing can resume. In manual mode, run
`mise bootstrap dotfiles sync` to publish your resolution.

## Use a template (optional)

Use tracking for files you edit directly. Use a template when you want mise to
render a file from configuration values.

Add these entries to `~/.config/mise/config.toml`, merging them into any existing
`[vars]` and `[dotfiles]` tables. Use an unused target path for this example:

```toml
[vars]
email = "you@example.com"

[dotfiles]
"~/templates" = { mode = "track" }
"~/.config/mise-template-example.ini" = { source = "~/templates/example.ini.tera", mode = "template" }
```

Create `~/templates/example.ini.tera` (create `~/templates` first if needed):

```ini
[user]
    email = {{ vars.email }}
```

Run `mise bootstrap` to render `~/.config/mise-template-example.ini`.
It will contain the email address from `[vars]`. Edit the template or its variables
for future changes. Tracking `~/templates` saves and shares those source files.
To also save the rendered file's history, add a tracking entry for it.

See [dotfiles](/dotfiles.html) for templates and OS-specific variants.

## Add more files

On Omarchy, start with individual configuration files you edit. Inspect a
directory before tracking it: themes, plugins, backgrounds, and application state
may not belong in your dotfile history. For a nested Git repository, history
records the commit it points to; keep that repository backed up separately.

Before an update, save the current tracked files:

```sh
mise bootstrap dotfiles save --best-effort
```

This saves your tracked dotfiles. Use your operating system's backup tools
for packages and other system state.

On macOS, you can keep a tracked file separate from its Linux counterpart:

```sh
mise bootstrap dotfiles track ~/.zshrc --os macos
```

On either platform, check tracking and watcher status with:

```sh
mise bootstrap dotfiles status
```

For directory exclusions and save options, see [dotfiles](/dotfiles.html).
For service management and other platforms, see
[user services](/bootstrap/services.html).
