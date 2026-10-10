---
description: "Track configuration files where they are, or deploy them from a dotfiles repository as links, copies, and templates."
socialDescription: "Track dotfiles in place, or deploy them from a repository as links, copies, and templates."
---

# Dotfiles

mise saves the history of the configuration files you edit in place, or
deploys them from a dotfiles repository as links, copies, and templates. The
commands are under [`mise dotfiles`](/cli/dotfiles.html), which these pages
shorten to its alias `mise dot`; `mise bootstrap dotfiles` runs the same
commands. Dotfiles work on their own, with or without
[`mise bootstrap`](/bootstrap.html).

## Recommended setup {#recommended-setup}

If you are not sure where to start, track your files in place, let the watcher
save every edit, and share them through a private Git repository. Add the
watcher to `~/.config/mise/config.toml`:

```toml
[bootstrap.services.mise-history]
builtin = "history-watch"
```

Then track your files and your mise configuration, start the watcher, and
connect the repository:

```sh
mise dot track ~/.zshrc ~/.config/nvim ~/.config/mise/config.toml
mise bootstrap services apply
mise dot origin set https://github.com/you/dotfiles.git --sync sync
```

On another machine, `mise bootstrap --adopt you/dotfiles` restores them.
[Set up a machine](/bootstrap/setup.html) walks through each step. The rest of
these pages cover the other approaches and every option; you do not need them
to get started.

## Choose an approach {#choose-an-approach}

Tracking leaves each file where it is, as a regular file you keep editing, and
saves its versions in a local Git repository that you can roll back and share
between machines. Deploying keeps the files in a source directory you
maintain, such as a dotfiles repository, and `mise dot apply` makes the live
files match it. You can combine the two and track a file that mise deploys.

| You want to                                                 | Use                                                         |
| ----------------------------------------------------------- | ----------------------------------------------------------- |
| Keep editing a file where it is, and undo or share changes  | [`mode = "track"`](/dotfiles/history.html)                  |
| Keep files in a repository and link them into place         | [`symlink` or `symlink-each`](/dotfiles/managed.html#modes) |
| Give an application a regular file that it can also rewrite | [`copy`](/dotfiles/managed.html#modes)                      |
| Generate a file that differs per machine                    | [`template`](/dotfiles/managed.html#templates)              |
| Deploy a whole tree, and choose parts of it per machine     | [Groups](/dotfiles/groups.html)                             |
| Own a few keys in a file that an application also writes    | [`merge = true`](/dotfiles/edits.html#merge)                |
| Add a line or block to a file you do not otherwise manage   | [`line` or `block`](/dotfiles/edits.html)                   |
| Remove an old file from every machine                       | [`mode = "absent"`](/dotfiles/managed.html#absent)          |

## Track a file in place {#tracking-files-in-place}

```sh
mise dot track ~/.zshrc
```

mise leaves the file where it is, saves its current contents as a checkpoint,
and adds `"~/.zshrc" = { mode = "track" }` to your global config
(`~/.config/mise/config.toml`). Run `mise dot save` after you edit it, or
install the [watcher](/dotfiles/history.html#automatic-saves) to save every
edit automatically. [Dotfiles history](/dotfiles/history.html) covers
comparing and rolling back versions, and
[Sync across machines](/dotfiles/sync.html) shares them through a private Git
repository. [Set up a machine](/bootstrap/setup.html) walks through the whole
flow on two machines.

## Deploy from a repository {#deploy-from-a-repository}

Suppose your dotfiles live in a Git repository:

```text
~/src/dotfiles/
├── mise.toml
├── git/config
└── nvim/
```

Declare each target and its source in the repository's `mise.toml`:

```toml
# ~/src/dotfiles/mise.toml
[dotfiles]
"~/.gitconfig" = { source = "git/config", mode = "copy" }
"~/.config/nvim" = { source = "nvim" }   # a symlink, the default mode
```

Trust the file, preview, apply, and check the result:

```sh
cd ~/src/dotfiles
mise trust
mise dot apply --dry-run
mise dot apply
mise dot status
```

mise copies `git/config` to `~/.gitconfig` and links `~/.config/nvim` to the
`nvim` directory, and `status` reports both as `applied`. Relative sources
start from the directory of the config file that declares them. To change a
file later, edit its source and run `mise dot apply` again.

A repository's `mise.toml` is project config. mise reads its `[dotfiles]`
only after you trust it, and only when it runs inside that directory, or
with `mise -C ~/src/dotfiles dot apply` from anywhere else.

To deploy the whole repository into your home directory, one entry can walk
it:

```toml
[dotfiles]
"~" = { source = ".", mode = "symlink-each", exclude = ["mise.toml", "*.md", ".git"] }
```

For a GNU Stow layout, with one directory per application, use
[groups](/dotfiles/groups.html). To start from files you already have,
[`mise dot add`](/dotfiles/managed.html#capturing-changes) moves each one into
your dotfiles directory and links it back. [Managed files](/dotfiles/managed.html)
covers modes, templates, permissions, and conflicts.

## Where to declare dotfiles {#where-to-declare-dotfiles}

Put `[dotfiles]` in one of these places:

- Your global config, `~/.config/mise/config.toml`, so that it applies from
  any directory. `mise dot track` writes its entries here, and so does
  `mise dot add`, unless
  [`write_targets.dotfiles`](/configuration/settings.html#write_targets.dotfiles)
  names another global file.
- A [`conf.d` folder](/configuration.html#conf-d-folders) next to the sources
  it uses. The folder can be a symlink into your dotfiles repository.
- The `mise.toml` of your dotfiles repository, as above.

Entries merge across these files like the rest of the
[config hierarchy](/configuration.html#configuration-hierarchy): a more local
file replaces an entry with the same target. Tracking entries and the
`[history]` table are read only from global and system config. To give some
entries to one machine or operating system, put them in `config.local.toml`,
or in a platform file such as `config.macos.toml` with the `auto_env` setting
on; see [Platform environments](/configuration/environments.html#platform-environments).

## With mise bootstrap {#with-mise-bootstrap}

[`mise bootstrap`](/bootstrap.html) applies `[dotfiles]` as one step of a full
machine setup, after it clones [repositories](/bootstrap/repos.html), so a
source can come from a repository it just cloned; see the
[run order](/bootstrap.html#how-it-runs). `mise dot apply` runs the same step
on its own. Both run your `pre-dotfiles` and `post-dotfiles`
[hooks](/bootstrap.html#hooks) before and after they write (`--dry-run` prints
the hooks instead), and the [`[history.reload]` commands](/dotfiles/history.html#reload-an-application-after-restoring-files)
that match the files they wrote. `mise dot add` and `mise dot edit --apply`
do not run these hooks.

## Commands {#commands}

| Task                          | Commands                                                                                                                                         |
| ----------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| Preview and apply             | [`mise dot diff`](/cli/dotfiles/diff.html), [`mise dot apply`](/cli/dotfiles/apply.html)                                                         |
| Check state                   | [`mise dot status`](/cli/dotfiles/status.html)                                                                                                   |
| Adopt a file or capture edits | [`mise dot add`](/cli/dotfiles/add.html), [`mise dot edit`](/cli/dotfiles/edit.html)                                                             |
| Remove deployed files         | [`mise dot unapply`](/cli/dotfiles/unapply.html)                                                                                                 |
| Track and save                | [`mise dot track`](/cli/dotfiles/track.html), [`mise dot save`](/cli/dotfiles/save.html), [`mise dot untrack`](/cli/dotfiles/untrack.html)       |
| Browse and restore            | [`mise dot history`](/cli/dotfiles/history.html), [`mise dot rollback`](/cli/dotfiles/rollback.html), [`mise dot undo`](/cli/dotfiles/undo.html) |
| Share between machines        | [`mise dot origin`](/cli/dotfiles/origin.html), [`mise dot sync`](/cli/dotfiles/sync.html), [`mise dot pull`](/cli/dotfiles/pull.html)           |

See [`mise dotfiles`](/cli/dotfiles.html) for every subcommand and flag.

## Next steps {#next-steps}

- [Managed files](/dotfiles/managed.html): entries, modes, templates,
  permissions, and conflicts.
- [Groups](/dotfiles/groups.html): deploy directory trees and choose them per
  machine.
- [Edit part of a file](/dotfiles/edits.html): blocks, lines, and merged keys.
- [Dotfiles history](/dotfiles/history.html): save, compare, and roll back
  tracked files.
- [Dotfiles reference](/dotfiles/reference.html): every key, pattern rule, and
  status state.
- [Dotfiles That Save Themselves](https://jdx.dev/posts/2026-09-07-dotfiles-that-save-themselves/)
  walks through tracking and syncing a real setup.
