# Set up a machine with mise

> [!WARNING]
> Dotfile tracking and synchronization are experimental. Enable them with
> `mise settings experimental=true`. Interfaces and storage formats may change.
> Existing source-managed dotfiles do not require experimental mode.

Keep editing your dotfiles where they are. This guide shows how to save local
history, restore a file, and optionally share your setup through a Git repository.
Start with one file; add more once you have tried restoring a change.

## Install mise

You need Git installed. If mise is already installed, skip the first two commands.

```sh
curl https://mise.run | sh
export PATH="$HOME/.local/bin:$PATH"
mise settings experimental=true
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
It saves edits to local history. No repository connection is needed.

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

A rollback becomes a new change in local history. If the file is shared, that
change can also be published according to your sync mode.

## Share your setup (optional)

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
mise bootstrap dotfiles origin set https://github.com/you/setup.git --name laptop --sync manual
```

Before confirming, review the files that will be shared and backed up. By default,
shared files and machine backups are stored in plaintext. A file excluded from sharing
may still be included in machine backups; do not assume it stays on this machine.
Existing checkpoints are not uploaded unless you select `--include-existing`.

For encrypted machine backups, add `--encrypt-backups` when connecting.
Keep the decryption identity somewhere safe outside this machine; it is needed
to restore the backups. See [encrypted backups](/history.html#sharing-across-machines)
for recipients and recovery. Encrypting backups does not encrypt shared files.

Choose the mode that fits your workflow:

| Mode         | Watcher behavior                                                |
| ------------ | --------------------------------------------------------------- |
| `manual`     | Saves locally; does not use the network automatically.          |
| `fetch-only` | Fetches remote changes; does not publish or apply them.         |
| `sync`       | Publishes saved changes, fetches, and applies incoming changes. |

Change modes with `mise settings set history.sync MODE`.

In manual mode, save and exchange checkpoints, then apply fetched changes:

```sh
mise bootstrap dotfiles save
mise bootstrap dotfiles sync
mise bootstrap dotfiles pull
```

`sync` exchanges saved changes with the repository; `pull` applies fetched
changes to your files. Applying changes does not run bootstrap tasks or render
templates. Run `mise bootstrap` when updated declarations need to be applied.

## Set up another machine

Install Git first. Then install mise, enable tracking, and authenticate with the
same repository host:

```sh
curl https://mise.run | sh
export PATH="$HOME/.local/bin:$PATH"
mise settings experimental=true
mise use -g gh
mise x gh -- gh auth login --hostname github.com --git-protocol https --web
mise x gh -- gh auth setup-git --hostname github.com
mise bootstrap --from-git you/setup
```

Review the proposed files before confirming. Bootstrap restores the shared
configuration and tracked files, then applies the configuration, including the
watcher declaration added earlier.

If an existing file differs, mise holds it for a decision instead of silently
overwriting it. Follow the reported conflict instructions. Check this machine's
sync mode with `mise bootstrap dotfiles status` and choose its mode explicitly
with `mise settings set history.sync MODE`.

For setup over SSH, see [remote bootstrap](/bootstrap/remote.html). Tracked setups
require explicit target opt-in:

```sh
mise bootstrap remote --experimental --host devbox --install-mise --from-git you/setup \
  --github-relay-read-only --github-relay-repo you/setup
```

GitHub access is borrowed read-only for this run. The target needs its own
credentials for ongoing synchronization.

## Resolve a conflict

If two machines change the same lines, mise preserves both versions and pauses
publication and incoming application for the entire setup. Local history
continues.

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
"~/.gitconfig" = { source = "templates/gitconfig.tera", mode = "template" }
```

Create `~/.config/mise/templates/gitconfig.tera`:

```ini
[user]
    email = {{ vars.email }}
```

Run `mise bootstrap` to render `~/.gitconfig`. Edit the template or its variables
for future changes. The template source is shared; the rendered file is kept in
local history but is not shared as a separate file.

See [dotfiles](/dotfiles.html) for templates and OS-specific variants.

## Add more files

On Omarchy, start with individual configuration files you edit. Inspect a
directory before tracking it: themes, plugins, backgrounds, and application state
may not belong in your dotfile history. Nested Git repositories are skipped.

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
