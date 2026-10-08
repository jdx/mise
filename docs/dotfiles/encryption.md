---
description: "Encrypt tracked dotfiles before mise saves them to Git, choose age or SSH recipients, and remove plaintext from shared history."
socialDescription: "Encrypt tracked dotfiles before saving them to Git, and remove plaintext from history."
---

# Encrypted files

mise can encrypt a tracked file with age before it saves the file to Git, so
the history and your origin hold only ciphertext while the file you edit stays
readable. Set it up before a file's first save: encrypting a file later does
not remove the plaintext versions already in history.

## Encrypt a file {#encrypt-a-file}

List public recipients in your global config and mark the file or directory
for encryption:

```toml
[history.encryption]
recipients = ["<age-or-plugin-public-recipient>", "<recovery-public-recipient>"]

[dotfiles]
"~/.config/app/credentials" = { mode = "track", encrypt = true }
```

Replace the placeholders with public recipients for your machines and an
independent recovery key; [Choose recipients](#choose-recipients) shows how to
get them. Public recipients travel with the repository. Keep the private keys
outside tracking.

To track a new file encrypted from its first checkpoint, configure the
recipients first and then run:

```sh
mise dot track ~/.config/app/credentials --encrypt
```

Run `mise dot track --encrypt` on its own, not as the command inside
[`mise dot capture`](/dotfiles/history.html#capturing-an-external-command), so
that mise can check the first encrypted checkpoint before it saves the entry.

Tracked template sources can be encrypted too:

```toml
[dotfiles]
"~/templates/private" = { mode = "track", encrypt = true }
"~/.config/app/config" = { mode = "template", source = "~/templates/private/app.tera" }
```

The rendered output is not encrypted. Set its
[permissions](/dotfiles/managed.html#permissions), and track it, separately.

## What encryption covers {#what-encryption-covers}

mise encrypts file contents before it stores them in Git. File names and
public metadata stay visible, and the files you edit or restore stay
unencrypted. Missing keys or recipients stop the operation; mise never falls
back to saving plaintext.

mise encrypts each saved version to every recipient in the list. A machine
whose recipient is missing can still transfer the encrypted history but cannot
read those files: a pull that has to inspect or apply one fails with
`cannot unlock <path>` instead of skipping it.

## Choose recipients {#choose-recipients}

A recipient is a public key. Every machine that must read the history needs
its own recipient in the list. mise accepts these forms:

| Recipient      | Example                    | Notes                                       |
| -------------- | -------------------------- | ------------------------------------------- |
| age x25519     | `age1qyqszq...`            | From `age-keygen`. Works unattended.        |
| SSH public key | `ssh-ed25519 AAAAC3Nza...` | Your existing key, if it has no passphrase. |
| Tagged age     | `age1tag1...`              | Works unattended.                           |
| age plugin     | `age1yubikey1...`          | Interactive only; see the warning below.    |

The matching private key is the identity mise decrypts with. mise finds
identities at `~/.config/mise/age.txt`, `~/.ssh/id_ed25519`, and
`~/.ssh/id_rsa`. Point it elsewhere with the
[`age.key_file`](/configuration/settings.html#age.key_file),
[`age.identity_files`](/configuration/settings.html#age.identity_files), or
[`age.ssh_identity_files`](/configuration/settings.html#age.ssh_identity_files)
settings.

::: warning Plugin recipients stop automatic saves
Plugin recipients such as `age1yubikey1...` need an interactive terminal, and
mise parses the whole list at once. The watcher runs in the background, so a
list that contains any plugin recipient stops automatic saving with
`plugin-dependent age recipients require interactive synchronization`, even
when native recipients are in the list too. For files the watcher saves, use
only the age, tagged, and SSH forms.
:::

### Use an existing SSH key {#use-an-existing-ssh-key}

If your SSH private key has no passphrase, its public key works as a recipient
without new tools. Add the contents of the `.pub` file:

```sh
cat ~/.ssh/id_ed25519.pub
```

```toml
[history.encryption]
recipients = ["ssh-ed25519 AAAAC3Nza... you@desktop"]
```

mise then decrypts with `~/.ssh/id_ed25519`.

mise does not prompt for SSH key passphrases, so a passphrase-protected key
cannot decrypt history at all. The error names the file and the reason when
mise first needs it. Generate a dedicated age key instead. This differs from
Git authentication, where a passphrase-protected key in an SSH agent is the
better choice: an agent does not help age decryption.

### Generate a dedicated age key {#generate-a-dedicated-age-key}

Install the age CLI and create an identity:

```sh
mise use -g age
mkdir -p ~/.config/mise
mise exec -- age-keygen -o ~/.config/mise/age.txt
# Public key: age1qyqszq...
```

`age.txt` holds the private identity, and mise reads it from that path. The
printed `age1...` value is the recipient. Keep the file out of tracking, and
restrict it with `chmod 600 ~/.config/mise/age.txt`. Repeat this on each
machine and add every public key to `recipients`.

### Add a recovery recipient {#add-a-recovery-recipient}

If you lose the only machine that holds an identity, the encrypted history
becomes unreadable, because re-encrypting requires decrypting first. Generate
a second identity that lives on none of your machines:

```sh
(umask 077 && mise exec -- age-keygen -o ~/recovery-key.txt)
cat ~/recovery-key.txt
```

Add its public key to `recipients`, store the file's contents in a password
manager or another offline place, then delete the local copy:

```sh
rm ~/recovery-key.txt
```

Treat it like a backup code: it decrypts everything encrypted after you add
it. Do not leave it in a tracked path, or anywhere the watcher saves.

A configuration for two machines plus recovery:

```toml
[history.encryption]
recipients = [
  "age1qyqszq...",  # desktop
  "age1ljx8w2...",  # laptop
  "age1v9zm4f...",  # recovery, stored in the password manager
]
```

A replacement machine needs both Git access to the origin and a matching age
identity. Keep the two recoverable separately, and test restoring onto a fresh
machine before you rely on it.

### Change recipients {#change-recipients}

Changing the list re-encrypts each file the next time it is saved. Commits
already in history keep the recipients they were written with, so a machine
added later can read the versions saved after the change, not the ones
before it. Add every machine's recipient before you save private contents
that all of them must read.

## Plaintext already in history {#plaintext-already-in-history}

Encrypting a file that was saved before leaves its earlier plaintext versions
in Git. Before a push, mise checks every reachable commit, including
intermediate saves and merge parents, against your encryption settings. An
earlier plaintext version blocks the push even if the newest version is
encrypted. To push, remove that history or explicitly allow it.

### Secrets in saved versions {#secrets-in-saved-versions}

The credential filter looks at file names, so a tracked `~/.bashrc` is saved
however many tokens you export in it. Before a push, mise also reads the
versions the origin does not have yet and refuses to publish when a line looks
like a secret: a provider token (`ghp_`, `github_pat_`, `glpat-`, `sk-`,
`AKIA`, `xox`), a private key block, or an assignment to a name ending in
`_KEY`, `_TOKEN`, `_SECRET`, or `_PASSWORD`. The error names the file, line,
and saved version, never the value. Files over 1 MiB and binary files are not
read.

Removing the secret from the file does not help, because the earlier version
still holds it. Rotate the secret, then encrypt the file or
[remove the plaintext from history](#remove-plaintext-from-history). To
publish anyway, use [`--allow-plaintext-history`](#allow-plaintext-history),
which skips this check along with the encryption check. Versions the origin
already has are not read again. The scan is a safety net for obvious cases,
not proof that a version is free of secrets.

### Allow plaintext history {#allow-plaintext-history}

To publish the older unencrypted versions anyway, skip the check for one sync:

```sh
mise dot sync --allow-plaintext-history
```

To allow it for every sync and pull, including the watcher's, add this to your
global config:

```toml
[settings.history]
allow_plaintext_history = true
```

Both publish the old plaintext to the origin. New saves still follow each
file's encryption setting. mise ignores
[`history.allow_plaintext_history`](/configuration/settings.html#history.allow_plaintext_history)
in project config.

### Remove plaintext from history {#remove-plaintext-from-history}

If you saved credentials before you turned on encryption, rotate them first.
Encryption does not protect the copies in older commits, even in a private
repository.

Stop the watcher service and back up the history repository. Keep the backup
secure, because it contains the plaintext too. If the service has the
`mise-history` name used on these pages, stop it and remove its installed
definition:

```sh
mise bootstrap services remove mise-history
```

This uses the user service manager on Linux, macOS, and Windows, and keeps it
from restarting the watcher during the repair. If you named the service
differently, use that name.

If the plaintext was never pushed, you can drop the affected checkpoints and
save the current file again with encryption on. This drops all checkpoints
after the last safe commit, including changes to other files. Find that commit
with `mise dot history`, or inspect the repository with the `git log` command
below.

Replace `safe` and the credentials path below with your commit and tracked
file, and make sure encryption is configured for that path before you save.
History uses a bare Git repository, so move the branch with `git update-ref`:

```sh
repo="${MISE_STATE_DIR:-${XDG_STATE_HOME:-$HOME/.local/state}/mise}/history/repo.git"
git --git-dir="$repo" log --all -- home/.config/app/credentials
old="$(git --git-dir="$repo" rev-parse refs/heads/main)"
safe="<commit before plaintext was saved>"

if git --git-dir="$repo" merge-base --is-ancestor "$safe" "$old"; then
  git --git-dir="$repo" update-ref -m "remove plaintext history" \
    refs/heads/main "$safe" "$old" &&
    mise dot save ~/.config/app/credentials \
      --description "save encrypted credentials" &&
    mise dot sync
fi
```

The ancestry check rejects commits outside the current history. Passing `old`
to `update-ref` makes it fail if another process has moved the branch. The
save records the live file with encryption and rebuilds mise's checkpoint
index. Moving the branch makes the discarded commits unreachable but does not
erase their plaintext objects from the repository right away. Once you have
checked the repaired history and no longer need those objects for recovery,
remove them:

```sh
git --git-dir="$repo" reflog expire --expire=now --all
git --git-dir="$repo" gc --prune=now
```

The backup still contains the discarded history. Delete it only when you no
longer need it.

To keep later checkpoints instead, use a tool such as
[git-filter-repo](https://github.com/newren/git-filter-repo) on a separate,
secure copy. Check that no reachable commit contains plaintext for the
protected path before you replace the local `refs/heads/main`. Do not filter
mise's active repository in place.

If the plaintext was already pushed, pause history on every machine that uses
the repository. Repair the history, then push the replacement branch with
Git's `--force-with-lease`; mise itself never force-pushes. On every other
machine, move the existing history store to a secure backup and run
`mise bootstrap --adopt <url>` to start a fresh store from the reviewed
replacement. A fresh adoption compares existing files with the incoming setup
before it creates local ancestry: identical files are accepted, and files that
differ wait for your decision. A machine that still has unrelated local
history you want to discard can
[replace it in one operation](/dotfiles/sync.html#adopt-the-origins-history)
with `mise bootstrap --adopt <url> --replace-history --yes`.

An ordinary `mise dot sync` never replaces existing history. Do not restart
any watcher until every machine uses the replacement, or an old store can
bring the plaintext back. Your Git host may still keep old objects or backups.

Once the repair is complete, restart the watcher:

```sh
mise bootstrap services apply
```
