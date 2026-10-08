---
description: "Bootstrap a machine from a Git repository: a bootstrap project with --from, or global config and setup repositories with --adopt."
socialDescription: "Bootstrap a machine from a Git repository with --from or --adopt."
---

# Bootstrap from a repository

`mise bootstrap` can clone a repository and run from the configuration in it.
Use `--from` for a bootstrap project that lives apart from your global config,
and `--adopt` to make a repository your global mise config or to restore a
setup repository of tracked dotfiles.

## Choose `--from` or `--adopt`

| Repository holds                                       | Command                        | Where the repository goes                        | Afterwards                                                          |
| ------------------------------------------------------ | ------------------------------ | ------------------------------------------------ | ------------------------------------------------------------------- |
| A bootstrap project: `mise.toml` and the files it uses | `mise bootstrap --from <url>`  | `$MISE_DATA_DIR/bootstrap-repo`, or `--from-dir` | Rerun the same command to apply the project again                   |
| Global mise config: `config.toml`, `conf.d/`, `tasks/` | `mise bootstrap --adopt <url>` | Your global config directory, `~/.config/mise`   | Every later mise command reads it as your global config             |
| A setup repository created with `mise dot origin set`  | `mise bootstrap --adopt <url>` | mise's history store; files go to their paths    | Later changes arrive through the history watcher or `mise dot pull` |

`--adopt` detects a setup repository by the `.mise-history/format.toml` file
that `mise dot origin set` writes, so the same flag covers both kinds. `--from`
and `--adopt` cannot be combined.

Either way, the bootstrap runs from the repository's configuration, and mise
trusts that configuration for this run. Review the repository before you run
the command. Add `--dry-run` to preview, and `-E <env>` to select a
[config environment](/configuration/environments.html), such as `-E work` for
`mise.work.toml` in a bootstrap project or `config.work.toml` in global config.

## Bootstrap project (`--from`) {#a-bootstrap-project}

A bootstrap project is a repository with a `mise.toml` and the source files it
refers to, such as dotfiles and templates. mise clones it and runs
`mise bootstrap` from the checkout:

```sh
mise bootstrap --from https://github.com/you/workstation.git
mise -E work bootstrap --from git@github.com:you/workstation.git
```

The checkout goes to `$MISE_DATA_DIR/bootstrap-repo`, normally
`~/.local/share/mise/bootstrap-repo`. Use `--from-dir <DIR>` to choose another
directory.

To check out a branch, tag, or commit instead of the default branch, add
`?ref=` to the URL. A `git::` prefix is accepted but not needed:

```sh
mise bootstrap --from 'https://github.com/you/workstation.git?ref=v1'
```

When the checkout already exists:

- Its `origin` must be the URL you pass. mise uses the checkout as it is.
- `--update` pulls new commits first, and accepts only a fast-forward.
- With `?ref=`, `--update` looks the ref up on the remote again. A branch is
  checked out and fast-forwarded, and a branch wins over a tag with the same
  name. A tag moves to the commit it now names. A branch or tag that no longer
  exists on the remote is an error.

`--dry-run` with no checkout yet prints the clone command and stops, because
there is no configuration to preview until the checkout exists.

## Global configuration (`--adopt`) {#global-mise-configuration}

Use `--adopt` when the repository is your global mise config, with files such as
`config.toml`, `config.work.toml`, `conf.d/`, and `tasks/`:

```sh
mise bootstrap --adopt you/mise-config
```

`OWNER/REPO` expands to `https://github.com/OWNER/REPO.git`. HTTPS and SSH URLs
and local paths work too. mise clones the repository's default branch into
`$MISE_CONFIG_DIR`, normally `~/.config/mise`, and runs bootstrap from it. The
files stay in place, so every later mise command reads them as your global
config. If you set `$MISE_GLOBAL_CONFIG_FILE`, mise clones into that file's
directory and loads that file.

Cloning needs `git`, and an SSH URL such as `git@github.com:you/setup.git` also
needs `ssh`. On a fresh machine that lacks them, mise offers to install them with
the host package manager (apt, dnf, pacman, apk, zypper, Homebrew, scoop, or
winget) before it clones. Pass `--yes` to accept without being asked. The
repository's own `[bootstrap.packages]` can't do this, because it has not been
cloned yet. With `--dry-run`, mise reports what is missing instead of
installing it.

The destination must be missing, empty, or already a Git checkout whose
`origin` is the repository you name. Move an existing `~/.config/mise` aside
before adopting into it. With an existing checkout, `--update` fast-forwards it
before the bootstrap runs. The checkout is an ordinary Git repository: commit
and pull in it as you would anywhere else.

## Setup repository (`--adopt`) {#shared-dotfile-history}

A setup repository holds the history of your tracked dotfiles, shared with
[`mise dot origin set`](/cli/dotfiles/origin/set.html). Adopting one restores
your tracked files and runs bootstrap from the restored configuration:

```sh
mise bootstrap --adopt you/setup
```

mise does the following:

1. Fetches the repository's default branch into its own history store.
2. Restores each tracked file to its path on this machine.
3. Remembers the repository as the origin for later synchronization.
4. Runs bootstrap from the restored mise configuration.

mise does not clone a setup repository into `$MISE_CONFIG_DIR`. If that
directory is already a Git checkout, though, its `origin` must be the repository
you adopt. Setup repositories always come from their latest commit, so
`--update` only refreshes package metadata and `[bootstrap.repos]` in the
bootstrap that follows.

Track the mise configuration and any template sources on the first machine, so
bootstrap can recreate tools and services and render templates here. If a file
on this machine differs from the shared version, adoption pauses before it
writes anything or runs bootstrap, so you can decide each file. `--dry-run`
shows the files it would write and those held for a decision.
`--replace-history` discards unrelated local history while adopting. It does
not resolve files that differ; add `--take-remote-all` to take the repository's
version of each.

[Set up a machine](/bootstrap/setup.html#set-up-another-machine) walks through
adopting a setup repository, including files that already exist. For
synchronization after that, see [Sync across machines](/dotfiles/sync.html).

## On other machines

To adopt a repository on another machine over SSH, see
[Remote hosts](/bootstrap/remote.html#private-configuration-repositories).
