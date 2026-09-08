---
description: "Set up a machine with tracked dotfiles, local history, recovery, and optional synchronization."
---

# Set up a machine with mise

Keep editing your dotfiles where they are. This guide shows how to save local
history, restore a file, and optionally share your setup through a Git repository.
Start with one file; add more once you have tried restoring a change.

## Install mise

You need Git installed. If mise is already installed, skip the first two commands.

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

Tracking saves a baseline and adds a declaration to `~/.config/mise/config.toml`.
The file stays in place:

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
mise bootstrap
mise bootstrap dotfiles status
```

The watcher runs as a systemd user service on Linux or a LaunchAgent on macOS.
It commits edits to the local Git history. No origin connection is needed.
The repository is stored separately from your live files.

If you prefer to save manually, skip the service and run
`mise bootstrap dotfiles save` after editing.

## Inspect and restore a change

Edit your tracked file, then save a checkpoint explicitly so you can inspect it
without waiting for the watcher:

```sh
mise bootstrap dotfiles save
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

Review the proposed changes before confirming. Rollback saves a protective
checkpoint first. To reverse the rollback:

```sh
mise bootstrap dotfiles undo
```

A rollback creates a new commit, preserving the versions you left behind.
A connected origin receives that commit according to your sync mode.

## Share your setup (optional)

Explicitly track the bootstrap configuration too, so another machine can recreate
your tools and watcher service. Managing `.zshrc` did not enroll this file:

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

Replace `you/setup` with your repository. This example chooses manual sync:

```sh
mise bootstrap dotfiles origin set https://github.com/you/setup.git --sync manual
```

Before confirming, review your tracked files and their history. Connecting an
origin makes every committed version eligible for synchronization, including
earlier local saves. There is no separate backup or per-file local-only mode.

For encrypted contents in the repository, use `encrypt = true` on an explicitly
tracked file and configure public recipients before its first capture. Keep
the decryption identity outside tracking. See [encrypted files](/history.html#encrypted-shared-files).

Choose the mode that fits your workflow:

| Mode         | Watcher behavior                                                |
| ------------ | --------------------------------------------------------------- |
| `manual`     | Saves locally; does not use the network automatically.          |
| `fetch-only` | Fetches remote changes; does not publish or apply them.         |
| `sync`       | Publishes saved changes, fetches, and applies incoming changes. |

Change modes with `mise settings set history.sync MODE`.

Manual mode postpones network publication, not local commits. A later sync
pushes the accumulated commits unchanged, then you can apply fetched changes:

```sh
mise bootstrap dotfiles save
mise bootstrap dotfiles sync
mise bootstrap dotfiles pull
```

`sync` exchanges saved changes with the repository; `pull` applies fetched
changes to your files. Applying changes does not run bootstrap tasks or render
templates. Run `mise bootstrap` when updated declarations need to be applied.

## Set up another machine

Install Git first. Then install mise and authenticate with the
same repository host:

```sh
curl https://mise.run | sh
export PATH="$HOME/.local/bin:$PATH"
mise use -g gh
mise x gh -- gh auth login --hostname github.com --git-protocol https --web
mise x gh -- gh auth setup-git --hostname github.com
mise bootstrap --from-git you/setup
```

Review the proposed files before confirming. Bootstrap reads enrollment from
the repository, restores the tracked files, and applies the explicitly tracked
configuration, including the watcher declaration added earlier. If configuration
or required template sources were not enrolled, bootstrap reports the missing
prerequisites rather than silently tracking them.

If an existing file differs, mise holds it for a decision instead of silently
overwriting it. Follow the reported conflict instructions. Check this machine's
sync mode with `mise bootstrap dotfiles status` and choose its mode explicitly
with `mise settings set history.sync MODE`.

For setup over SSH, see [remote bootstrap](/bootstrap/remote.html). For a private GitHub repository, borrow read-only access from this machine:

```sh
mise bootstrap remote --host devbox --install-mise --from-git you/setup \
  --github-relay-read-only --github-relay-repo you/setup
```

GitHub access is borrowed read-only for this run. The target needs its own
credentials for ongoing synchronization.

## Resolve a conflict

If two machines change the same lines, mise preserves both versions and pauses
publication and incoming application for the entire setup. Local commits and
fetching continue. Desktop notifications are enabled by default; status and
`mise doctor` report the pause even without a working notifier.

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
`[vars]` and `[dotfiles]` tables:

```toml
[vars]
email = "you@example.com"

[dotfiles]
"~/templates" = { mode = "track" }
"~/.gitconfig" = { source = "~/templates/gitconfig.tera", mode = "template" }
```

Create `~/templates/gitconfig.tera` (create `~/templates` first if needed):

```ini
[user]
    email = {{ vars.email }}
```

Run `mise bootstrap` to render `~/.gitconfig`. Edit the template or its variables
for future changes. The explicitly enrolled template directory is committed and synchronized.
The rendered file is not tracked unless you explicitly enroll it too.

See [dotfiles](/dotfiles.html) for templates and OS-specific variants.

## Add more files

On Omarchy, start with individual configuration files you edit. Inspect a
directory before tracking it: themes, plugins, backgrounds, and application state
may not belong in your dotfile history. Nested Git repositories are recorded as Git pointers, not copied recursively.

Before an update, save the current tracked files:

```sh
mise bootstrap dotfiles save --best-effort
```

This saves dotfiles, not installed packages or the rest of the operating system.

On macOS, you can keep a tracked file separate from its Linux counterpart:

```sh
mise bootstrap dotfiles track ~/.zshrc --os macos
```

On either platform, check tracking and watcher status with:

```sh
mise bootstrap dotfiles status
```

For directory exclusions and capture policies, see [dotfiles](/dotfiles.html).
For service management and other platforms, see
[user services](/bootstrap/services.html).
