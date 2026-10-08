---
description: "Deploy whole directory trees of dotfiles as named groups, and choose which groups each machine applies."
socialDescription: "Deploy directory trees of dotfiles as groups, and choose which groups each machine applies."
---

# Groups

A group is a named directory tree of dotfiles, such as one per application or
one per machine role. mise deploys every file in the tree without a list of
entries, and each machine chooses which groups it applies.

## Define a group {#define-a-group}

Declare a group with a `[dotfile_groups.<name>]` table that names its `root`:

```toml
[dotfile_groups.zsh]
root = "zsh"           # links ~/.dotfiles/zsh/.zshrc to ~/.zshrc

[dotfile_groups.home]
root = "home"          # ~/.dotfiles/home
target = "~"
mode = "symlink-each"
dot_prefix = true
exclude = ["README.md"]
```

mise walks the root and deploys each file at the same path under the target.
A relative `root` always starts at
[`dotfiles.root`](/configuration/settings.html#dotfiles.root), not at the
directory of the config file that declares the group. Group names can contain
letters, digits, `-`, `_`, and `.`. If a more local config file defines a
group with the same name, its table replaces the whole group.

### Group keys {#group-keys}

| Key          | Default        | Meaning                                                                                                                                                             |
| ------------ | -------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `root`       | (required)     | The directory tree to deploy. A relative path starts at `dotfiles.root`.                                                                                            |
| `target`     | `~`            | The directory the tree deploys into.                                                                                                                                |
| `mode`       | `symlink-each` | `symlink-each` links each file, `copy` copies each file, and `symlink` links the whole tree. `dot_prefix`, `manifest`, and `entries` need `symlink-each` or `copy`. |
| `exclude`    |                | Paths under the root to skip, using the [pattern syntax](/dotfiles/reference.html#patterns).                                                                        |
| `dot_prefix` | `false`        | Deploy `dot-<name>` names as `.<name>`, as described in [Visible source names](/dotfiles/managed.html#dot-prefix).                                                  |
| `manifest`   |                | `"git"` deploys only files in Git's index, as described in [Deploy only files committed to Git](/dotfiles/managed.html#git-tracked-directories).                    |
| `relative`   |                | Link by relative paths, as described in [Relative symlinks](/dotfiles/managed.html#relative).                                                                       |
| `entries`    |                | Whole-file entries for parts of the tree; see [Group entries](#group-entries). Requires `symlink-each` or `copy`.                                                   |

A group with `mode = "symlink"` and an `entries` table is invalid: mise warns
and ignores the whole group. With `symlink`, mise also ignores `dot_prefix`
and `manifest`.

## Group entries {#group-entries}

`[dotfile_groups.<name>.entries]` describes parts of the tree with the same
syntax as [`[dotfiles]` entries](/dotfiles/managed.html#whole-file-entries),
keyed by target path. Each entry is cut out of the walk, so it decides how its
path is deployed. For example, to link a directory as a whole instead of each
file in it, so that files the application creates there also land in your
dotfiles:

```toml
[dotfile_groups.home]
root = "home"
dot_prefix = true

[dotfile_groups.home.entries]
"~/.config/kitty" = { mode = "symlink" }                     # ~/.config/kitty -> home/dot-config/kitty
"~/.ssh/config" = { mode = "copy", permissions = "0600" }
"~/.gitconfig" = { source = "git/config.tmpl", mode = "template" }
"~/.kitty-old.conf" = { mode = "absent" }
```

- An entry without `source` finds it under the root, at its path inside the
  group's target: `~/.config/kitty` in the `home` group reads
  `~/.dotfiles/home/dot-config/kitty`.
- A relative `source` starts at the root. When it lies inside the root, or
  inside the tree of another entry that walks a directory, that walk skips it,
  so `git/config.tmpl` is rendered to `~/.gitconfig` and not also linked to
  `~/git/config.tmpl`.
- An entry without `mode` deploys like the group: a directory the group's way,
  a file as one link, or as one copy in a `copy` group.
- An entry that walks a directory inherits the group's `dot_prefix` and
  `manifest`, and the group's `exclude` patterns that contain no `/`, unless
  it sets its own.
- Every entry, including each target its `variants` name, must lie inside the
  group's target, and cannot be the target itself. An entry beneath another
  walking entry is cut out of that one too. An entry beneath a directory
  linked as a whole would change a file in the root itself, so mise reports it
  as a conflict before anything is written.

Group entries are whole-file entries only. Declare
[block, line, and merge edits](/dotfiles/edits.html) in `[dotfiles]`. To put an
entry in a group, declare it under `[dotfile_groups.<name>.entries]`; a
`[dotfiles]` entry with a `group` key is invalid.

## Select groups {#selecting-groups}

Every declared group applies until you select some. Use
`[bootstrap] dotfile_groups` to choose the groups a machine applies, for
example in that machine's `~/.config/mise/config.local.toml`:

```toml
[bootstrap]
dotfile_groups = ["home", "zsh"]
```

A more local config file's list replaces the others. `[dotfiles]` entries
apply on every machine, whichever groups it selects. mise warns when the list
names a group that nothing declares.

Two selected groups cannot deploy the same target file, and one group cannot
link a directory as a whole while another places files inside it. mise
reports either case as a conflict naming both groups, before anything is
written.

## Deselect and remove groups {#deselecting-and-removing-groups}

Deselecting a group, or deleting its table, leaves its files in place, and
`mise dot status` lists them as `orphaned`. mise knows those files because it
records what each group deployed in its
[state directory](/dotfiles/reference.html#state-files). A file also becomes
orphaned when a selected group stops deploying it, for example after a new
`exclude` pattern or after you delete a copied source file. A file that any
active entry still deploys is never orphaned, even when the entry has moved to
another group. If mise cannot read a group, for example because its `root` is
missing, it warns about the group instead of listing its files.

To remove orphaned files, or every file of one group:

```sh
mise dot apply --prune          # apply, then remove orphaned files
mise dot unapply --group work   # remove one group's files
```

`--prune` and `unapply --group` remove a link only while it still points at
the source mise linked, and a copied file only while it still holds what mise
wrote. They leave anything you changed in place with a warning; add `--force`
to remove changed copies too. Directories they empty are removed, up to the
target of the group or entry that deployed the file. `unapply --group` works
whether or not the group is still selected or declared. `--prune` asks before
it removes anything unless you pass `--yes`, and it covers every group, so it
takes no target arguments.

Both commands compare files with what mise last wrote, not with the current
source, so editing a group's root does not stop `unapply --group` from
removing an untouched copy. Neither removes a path that resolves through a
linked directory, for example after you link a directory as a whole, or a path
inside `dotfiles.root`, even with `--force`, so they never delete source files.

## Add files to a group {#adding-files-to-a-group}

[`mise dot add`](/cli/dotfiles/add.html) captures a file inside a group's
target into that group's root, without writing an entry. In a `dot_prefix`
group, the source gets the `dot-` name. With the two groups above, which both
deploy into `~`, name the group for a new file:

```sh
mise dot add --group home ~/.config/starship.toml   # -> ~/.dotfiles/home/dot-config/starship.toml
```

When several groups contain the path, mise picks the one whose target is
deepest. Among groups at the same depth, the one whose root already holds the
file wins. For a new file that no root holds yet, mise refuses until you pass
`--group`.

[`mise dot edit`](/cli/dotfiles/edit.html) opens the group's source file for a
path inside a group's tree, and takes `--group` the same way when it has to
create the file. A group with `manifest = "git"` deploys only files in Git's
index, so `add` refuses to capture into it; copy the file into the root and
`git add` it instead.

## Coming from GNU Stow {#coming-from-gnu-stow}

Groups cover the same ground as Stow packages:

| GNU Stow                        | mise                                                                                                         |
| ------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| A package directory             | A group, `[dotfile_groups.<name>]`, with that directory as `root`                                            |
| `stow --dotfiles`               | `dot_prefix = true`                                                                                          |
| Relative links (Stow's default) | [`dotfiles.relative_symlinks`](/configuration/settings.html#dotfiles.relative_symlinks) or `relative = true` |
| `stow -D <package>`             | `mise dot unapply --group <name>`                                                                            |
| `stow --adopt`                  | `mise dot add --group <name> <file>`                                                                         |
| Choosing which packages to stow | `[bootstrap] dotfile_groups`                                                                                 |

Stow links a whole directory when nothing else lives in it. A `symlink-each`
group always links file by file; to link a directory as a whole, add a
`symlink` [group entry](#group-entries) for it.
