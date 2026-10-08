---
description: "Link, copy, or render configuration files from sources you maintain, and control their permissions, conflicts, and removal."
socialDescription: "Link, copy, or render dotfiles from sources you maintain."
---

# Managed files

A managed file is a target that mise creates from a source you maintain: a
link to the source, a copy of it, a rendered template, or content written in
the config itself. Declare each target in `[dotfiles]` and run
[`mise dot apply`](/cli/dotfiles/apply.html).

## Entries {#whole-file-entries}

Each entry in `[dotfiles]` uses the target path as its key. Use an absolute
path or one that starts with `~/`. A source can be a file or a directory:

```toml
[dotfiles]
"~/.zshrc" = { mode = "symlink" }
"~/.ssh/config" = { source = "ssh/config", mode = "copy" }
"~/.config/nvim" = "dotfiles/nvim"
```

When you omit `source`, mise looks under
[`dotfiles.root`](/configuration/settings.html#dotfiles.root) (`~/.dotfiles`
by default) at the same path relative to your home directory: `~/.zshrc`
reads `~/.dotfiles/.zshrc`, and `~/.config/foo.toml` reads
`~/.dotfiles/.config/foo.toml`. A target outside your home directory needs a
`source` or [inline `content`](#inline-content).

A relative `source` starts from the directory of the config file that declares
it, so `source = "ssh/config"` in `~/.config/mise/config.toml` refers to
`~/.config/mise/ssh/config`. A string value, as in the `~/.config/nvim` entry
above, is a source that uses the mode from
[`dotfiles.default_mode`](/configuration/settings.html#dotfiles.default_mode).

To check your work, preview with `mise dot apply --dry-run` (add `--verbose`
to see content diffs) or [`mise dot diff`](/cli/dotfiles/diff.html), apply with
`mise dot apply`, and confirm with
[`mise dot status`](/cli/dotfiles/status.html). Apply skips targets that
already match. The [reference](/dotfiles/reference.html#entry-keys) lists
every entry key.

## Modes {#modes}

`mode` decides how mise creates the target from its source. Without it, mise
uses `dotfiles.default_mode`, which is `symlink` unless you change it.

| Mode           | What happens when you apply                                       | Use when                                                              |
| -------------- | ----------------------------------------------------------------- | --------------------------------------------------------------------- |
| `symlink`      | Create one link to a file or a whole directory.                   | Edits through the target should change the source.                    |
| `symlink-each` | Create directories and link each file within them.                | The target directory also holds files you want mise to leave alone.   |
| `copy`         | Copy a file or directory, overwriting matching files.             | The application needs a regular file or writes its own configuration. |
| `template`     | Render a source file with the [template engine](/templates.html). | The output depends on machine-specific variables.                     |
| `absent`       | Remove a file or symlink at the target; takes no source.          | A file you no longer use should not exist on any machine.             |

`mode = "track"` and `mode = "track-local"` are not deployment modes: they
save the history of a file where it is, without a source. See [Dotfiles history](/dotfiles/history.html).
On Windows, `symlink` can fall back to copying and `symlink-each` always
copies; see [Windows](#windows).

To link a directory as a whole:

```toml
[dotfiles]
"~/.config/nvim" = { source = "dotfiles/nvim", mode = "symlink" }
```

`symlink-each` requires a directory source. When you delete or exclude a
source file, the next apply removes its link, and other files in the target
directory stay in place. mise remembers the links it created in its
[state directory](/dotfiles/reference.html#state-files), so keep that
directory: later applies use it to update or remove those links. When two
[config environments](/configuration/environments.html) point the same
`symlink-each` target at different sources, applying with the other
environment, such as `mise -E work dot apply`, removes links that only the old
source had, repoints shared paths, and leaves unmanaged files alone.

A directory `copy` is additive. It keeps target files whose sources you
deleted or excluded, so remove those leftover copies yourself.

::: warning Copies do not flow back
Applying a `copy` entry overwrites changes made directly to the target. Save
them to the source first with `mise dot add`; see
[Adopt files and capture edits](#capturing-changes).
:::

## Templates {#templates}

A `template` entry renders its source with mise's
[Tera templates](/templates.html) on every apply:

```toml
[dotfiles]
"~/.gitconfig" = { source = "dotfiles/gitconfig.tera", mode = "template" }

[vars]
email = "you@example.com"
```

<div v-pre>

```jinja
[user]
    email = {{ vars.email }}
{% if os() == "macos" %}
[credential]
    helper = osxkeychain
{% endif %}
```

</div>

Templates can use `env`, `vars`, `exec()`, and the rest of the
[template context](/templates.html#variables). They can also read a declared
[bootstrap secret input](/bootstrap/secrets.html) with
<span v-pre>`{{ secret(name="logical_name") }}`</span>. Pass `--prompt-secrets`
to a dotfiles command to prompt securely for missing values. Applying a
template writes its rendered content and gives the target the source file's
permissions, or the ones [`permissions`](#permissions) sets. A later apply
also repairs changed permissions.

`status`, `diff`, and `apply` render templates to compare their output. This
runs any `exec()` calls in those templates with your trusted config. Secret
values are redacted from diffs and other command output. With `--dry-run`,
mise skips rendering dotfile templates and labels them `(if changed)`. Other
config expressions can still run during a dry run, so use it only with
trusted config.

The experimental [`mise oci build`](/dev-tools/mise-oci.html#bootstrap-and-dotfiles-in-oci-images)
renders templates with a restricted context; see that page for what it
rejects.

### Remove the target when a template renders empty {#remove-empty}

A template normally writes its output even when that output is empty. With
`remove_empty = true`, an output that is empty or holds only whitespace
removes the target instead. One template can then decide whether a file
exists at all, for example a work-only config:

```toml
[dotfiles]
"~/.config/app/work.toml" = { source = "work.toml.tera", mode = "template", remove_empty = true }
```

<div v-pre>

```jinja
{% if env.WORK == "1" %}
[proxy]
url = "http://proxy.example.com"
{% endif %}
```

</div>

With `WORK=1`, `mise dot apply` writes the file. Without it, the template
renders empty and the next apply removes the file. Setting the variable again
recreates it. `status` and `diff` show a pending removal before any apply.

mise removes the target only when the file is empty, holds only whitespace,
or still holds exactly what mise last wrote there. If you edited it, apply
reports a [conflict](#conflicts) and keeps the file; `mise dot apply --force`
removes it anyway. A directory at the target, and a target mise cannot read
(for example one written with `permissions = "0200"`), are conflicts too.
The record of what mise wrote is [kept locally](/dotfiles/reference.html#state-files),
so on a machine that has never applied the template, an existing target with
other content is a conflict.

Parent directories that mise created for the target are removed with it once
they are empty. In the example, if `~/.config/app` did not exist before the
first apply, removing `work.toml` also removes `app`. Directories that already
existed, that still hold other files, or that another entry needs are kept.
mise removes only directories physically inside your home directory, never
the home directory itself: `/opt/app` stays for a target `/opt/app/app.toml`,
and so does `~/.config/app` when `~/.config` is a symlink to a directory
outside your home.

`remove_empty` requires `mode = "template"`. When the target is also
[tracked](/dotfiles/history.html), `mise dot rollback` and `mise dot undo`
bring back a file this removed.

## Inline content {#inline-content}

Use `content` to write a whole file from a string in the config instead of
keeping a separate source file. On Unix the file gets permissions `0600`, so
only its owner can read and write it, unless [`permissions`](#permissions)
sets others:

```toml
[dotfiles]
"~/.config/example.conf" = { content = "enabled = true\n" }
```

`content` takes the place of `source` and `mode`. It can be combined with
`permissions`, but not with `source`, `mode`, `exclude`, `manifest`,
`encrypt = true`, `remove_empty`, `dot_prefix`, `relative = true`, target
`variants`, the track-only keys `include` and `allow_plaintext`, or the edit
options `block`, `line`, `template`, and `comment`. The
[modes and keys table](/dotfiles/reference.html#modes-and-keys) compares all
entry kinds.

## Permissions {#permissions}

Set `permissions` to an octal string to give the target those permissions
instead of the ones it would otherwise get. It works with `copy` and
`template` entries that have a file source, and with inline `content`:

```toml
[dotfiles]
"~/.netrc" = { source = "netrc.tera", mode = "template", permissions = "0600" }
```

`mise dot status` reports a target whose permissions have changed since, and
the next apply sets them again.

On its own, `permissions` manages only the permissions of a file or directory
that already exists. mise never creates it, never changes its content, and
never infers a source for it from `dotfiles.root`:

```toml
[dotfiles]
"~/.ssh" = { permissions = "0700" }
"~/.ssh/config" = { permissions = "0600" }
```

For a path outside your home directory, or one that needs an owner and group,
use [`[bootstrap.files]`](/bootstrap/files.html#permissions-without-content)
instead.

When a permissions-only target does not exist, apply warns and skips it, and
`status` counts it as `applied` with the reason
`target absent; permissions not applied`, so `mise dot status --missing` does
not fail. A directory that another entry creates in the same apply still gets
its permissions. mise does not follow a symlink at the target: `status`
reports it, apply skips it with a warning, and a link swapped in while mise
runs is refused. `mise dot edit` does not create a missing permissions-only
target, and `mise dot unapply` never removes one.

A mode that denies even the owner read access, such as `0200`, makes mise
check only the target's permissions, because it cannot read the content back.

`permissions` cannot be combined with `symlink` or `symlink-each`, which have
no permissions of their own, with `track`, whose history records the file's
mode itself, or with a directory source. A permissions-only target cannot
contain wildcards. On Windows, `permissions` is ignored with a warning.

## Remove files {#absent}

Use `mode = "absent"` to remove a file you no longer want on your machines,
such as the configuration of a tool you replaced:

```toml
[dotfiles]
"~/.oldrc" = { mode = "absent" }
```

`mise dot apply` deletes `~/.oldrc` if it exists and does nothing once it is
gone. The entry is the instruction, so mise removes a regular file or a symlink
without comparing its content. It removes a symlink itself, never the file or
directory the link points to. `mise dot apply --dry-run` prints
`rm <target>`, and `mise dot status` shows the entry as `would remove` while
the file is there and `absent` once it is gone.

An `absent` entry never removes a directory, or anything else that is not a
regular file or symlink, such as a socket or FIFO. For those targets, `status`
and `apply` report an error naming the path, even with `--force`, and you
remove it yourself. The one exception is a parent directory that mise created
when an earlier entry wrote this target: once the file is gone and the
directory is empty, it goes too, as [for templates](#remove-empty).

An `absent` entry takes no `source`, `content`, `exclude`, `manifest`,
`permissions`, `encrypt = true`, `remove_empty`, `relative = true`,
`dot_prefix`, or block and line edit keys; mise warns and ignores an entry
that sets one. No other entry can place a file beneath an `absent` target, and
an edit entry cannot change the file it removes. The target names exactly one
path, so it cannot contain `*`, `?`, or `[`. To remove a file whose name
contains those characters, declare `state = "absent"` under
[`[bootstrap.files]`](/bootstrap/files.html#removing-resources).

When the removed path is [tracked](/dotfiles/history.html), the removal is
recorded like any other apply, so `mise dot undo` restores the file.
`mise dot unapply` leaves the target alone, because mise did not create the
file. In an image built with the experimental
[`mise oci build`](/dev-tools/mise-oci.html#bootstrap-and-dotfiles-in-oci-images),
an `absent` entry hides the base image's file at that path.

[Target variants](#platform-specific-destinations) work with `absent`, so you
can remove a file on some machines only:

```toml
[dotfiles."~/.bash_profile"]
mode = "absent"
variants = [{ os = "macos" }]
```

Machines that match no variant skip the entry.

## Directories

### Match several source files {#matching-multiple-source-files}

Source paths can contain the glob wildcards `*`, `**`, `?`, and `[ab]`. When a
wildcard source matches several paths, the target must contain matching
wildcards so that each source expands to its own target:

```toml
[dotfiles]
"~/.config/*.toml" = "dotfiles/config/*.toml"
"~/.local/share/app/**/*.json" = { source = "dotfiles/app/**/*.json", mode = "copy" }
"~/.config/app?.toml" = "dotfiles/config/app?.toml"
"~/.config/theme-[ab].toml" = "dotfiles/config/theme-[ab].toml"
```

### Exclude files {#excluding-files}

`symlink-each` entries, and `copy` entries with a directory source, walk the
source directory. Add an `exclude` list of glob patterns to skip files in it,
for example when the source is the repository that also holds `mise.toml`:

```toml
[dotfiles]
"~" = { source = ".", mode = "symlink-each", exclude = ["mise.toml", "*.md", ".git"] }
```

Patterns are relative to the source directory and follow the
[pattern syntax](/dotfiles/reference.html#patterns): a pattern without `/`,
such as `*.md`, matches a name at any depth, and a pattern with `/`, such as
`nvim/spell`, is anchored to the source root. Use `**` to match across
directories.

For `symlink-each`, excluding a file that mise linked before removes its link
on the next apply, as deleting the source would. A directory `copy` is
additive: exclusions prevent future copies but leave existing target files in
place. To leave files out of a [tracked directory](/dotfiles/history.html#exclude-files-from-one-directory),
use the tracked entry's own `exclude` list.

### Deploy only files committed to Git {#git-tracked-directories}

Set `manifest = "git"` on a `symlink-each` or directory `copy` entry to manage
only the files in Git's index. Use it for a dotfiles repository whose
`.gitignore` ignores everything (`*`) and opts files in with `git add -f`: the
index is already the list of files to deploy, so you do not repeat it in
mise:

```toml
[dotfiles]
"~" = { source = ".", mode = "symlink-each", manifest = "git" }
```

mise runs `git ls-files` in the source directory. Ignored and untracked files
are left alone, and removing a file from the index removes its `symlink-each`
link on the next apply. `exclude` filters the Git file list further. A Git
manifest requires a directory source and `symlink-each` or `copy` mode.

### Visible source names {#dot-prefix}

Set `dot_prefix = true` on a `symlink-each` or directory `copy` entry to keep
the files in your dotfiles repository visible. As with GNU Stow's
`--dotfiles` option, every path component named `dot-<name>` deploys as
`.<name>`, and other names deploy unchanged:

```toml
[dotfiles]
"~" = { source = "home", mode = "symlink-each", dot_prefix = true, exclude = ["README.md"] }
```

| Source                            | Target                      |
| --------------------------------- | --------------------------- |
| `home/dot-bashrc`                 | `~/.bashrc`                 |
| `home/dot-config/foo/config.toml` | `~/.config/foo/config.toml` |
| `home/bin/dot-helper`             | `~/bin/.helper`             |
| `home/.editorconfig`              | `~/.editorconfig`           |

`exclude` and `manifest = "git"` match the source names, such as
`dot-bashrc`. If two source paths deploy to the same target, such as
`dot-bashrc` and `.bashrc`, apply fails and reports both paths. `mise dot add`
refuses to capture into a `dot_prefix` entry, because it would copy target
names into the source; edit the source directly instead. A
[group](/dotfiles/groups.html) with `dot_prefix` does accept new files and
stores them under `dot-` names.

## Different targets on different machines {#platform-specific-destinations}

`variants` means one thing on a deployed entry and another on a tracked entry.
Here it picks the target path for each machine. On a
[tracked entry](/dotfiles/history.html#variants), it keeps separate contents
at the same path instead.

To deploy one source to a different path on each operating system:

```toml
[dotfiles."vscode/settings.json"]
source = "dotfiles/vscode/settings.json"
mode = "copy"
variants = [
  { os = "macos", target = "~/Library/Application Support/Code/User/settings.json" },
  { os = "linux", target = "~/.config/Code/User/settings.json" },
  { os = "windows", target = "~/AppData/Roaming/Code/User/settings.json" },
]
```

Target variants work with `copy`, `symlink`, `symlink-each`, `template`, and
[`absent`](#absent). Each variant selects machines by `os` (optionally with an
architecture, such as `os = "macos/arm64"`), by `profile` (a
[config environment](/configuration/environments.html) selected with `-E` or
`MISE_ENV`), or with `default = true`. The most specific matching variant
wins, a tie makes the entry invalid, and a machine that matches no variant and
has no default skips the entry. The [variant selectors](/dotfiles/reference.html#variant-selectors)
reference has the scoring rules.

A variant's `target` overrides the table key. When every variant supplies a
`target`, the key can be a logical name, as above. Otherwise the key must be
an absolute or home-relative target path. Every target must be absolute or
start with `~/`.

When every variant supplies a `target`, you can omit `source`, and mise reads
the source from the entry key as a path under `dotfiles.root`:

```toml
[dotfiles."vscode/settings.json"]
mode = "copy"
variants = [
  { os = "macos", target = "~/Library/Application Support/Code/User/settings.json" },
  { os = "linux", target = "~/.config/Code/User/settings.json" },
]
```

This reads `~/.dotfiles/vscode/settings.json` on both machines. Such a key
cannot contain `..`. An explicit `source` is the same on every machine and,
when relative, resolves from the directory of the config file. If any variant
omits `target`, the entry needs an explicit `source`, and that variant uses
the table key as its target:

```toml
[dotfiles."~/.config/example/settings.json"]
source = "dotfiles/example/settings.json"
mode = "symlink"
variants = [
  { profile = "work", target = "~/.config/example-work/settings.json" },
  { default = true },
]
```

`status`, `diff`, `apply`, and `unapply` use the selected target. Changing
which target a machine selects does not remove the file deployed at the old
one.

To give a whole set of entries to one operating system, put them in a
platform config file such as `config.macos.toml` instead, with the
[`auto_env`](/configuration/settings.html#auto_env) setting on; see
[Platform environments](/configuration/environments.html#platform-environments).
Use `variants` when one entry needs a different target on each machine.

## Relative symlinks {#relative}

By default, links point at their source by an absolute path. Set
[`dotfiles.relative_symlinks`](/configuration/settings.html#dotfiles.relative_symlinks)
to link by a path relative to the link's directory instead, as GNU Stow does.
Relative links keep working when the home directory is mounted at a different
path on another machine, for example over NFS, or when the whole tree moves:

```toml
[settings]
dotfiles.relative_symlinks = true

[dotfiles]
"~/.config/foo" = { source = "~/dotfiles/foo", mode = "symlink" } # ~/.config/foo -> ../dotfiles/foo
"~/.bashrc" = { source = "~/dotfiles/bashrc", relative = false }  # stays absolute
```

An entry's `relative` key overrides the setting, and `relative = true`
requires `symlink` or `symlink-each`. With relative links on, the next apply
repoints absolute links to the same source. Turning them off leaves relative
links that already reach the source in place. Windows ignores the option,
because directory links there are junctions, which cannot be relative.

## Adopt files and capture edits {#capturing-changes}

To start managing a file you already have, run
[`mise dot add`](/cli/dotfiles/add.html):

```sh
mise dot add ~/.inputrc
```

mise moves the file to its source path under `dotfiles.root`
(`~/.dotfiles/.inputrc`), adds an entry to your global config, and applies it.
With the default `symlink` mode, `~/.inputrc` becomes a link to the moved
file. When the source is on another filesystem, mise copies the file, keeping
symlinks and permissions, instead of moving it.

- `--mode copy` keeps a regular file at the target; `--mode` takes any mode.
- `--source <path>` uses that source instead of one under `dotfiles.root`.
- `--no-apply` copies the file to its source and writes the entry without
  changing the original file, so you can review both first.
- `--dry-run` prints the source and config changes without making them.

The entry goes to your global config, or to the file named by
[`write_targets.dotfiles`](/configuration/settings.html#write_targets.dotfiles).
Use `--local` to write the project config or `--path` to name another file.
`add` leaves out a source it can infer and the default `symlink` mode, so the
example writes `"~/.inputrc" = {}`. A mode you pass with `--mode` is always
written.

If you edit a copied target in place, run `mise dot add` on it again to save
the change back to its source:

```sh
$EDITOR ~/.config/starship.toml
mise dot add ~/.config/starship.toml
```

`mise dot add --changed` updates the sources of every changed regular file
managed in `copy` mode. Each file's config must be trusted. It skips directory
copies, symlinks, templates, and inline content.

A target inside a [group](/dotfiles/groups.html#adding-files-to-a-group)'s
tree is captured into that group's root instead, without a new entry.

[`mise dot edit <target>`](/cli/dotfiles/edit.html) opens the target's source
in your editor, and `--apply` applies the target when the editor exits. For a
tracked or permissions-only entry it opens the file itself, and for inline
`content`, an `absent` entry, or an edit without a source file it opens the
config file that declares it. For a target that is not managed yet, it first
adds it the way `mise dot add --no-apply` does, after asking (`--yes` skips
the question).

## Conflicts {#conflicts}

A `symlink` or `symlink-each` entry never replaces a real file or directory
where a link should go, or a directory where a file should go. Apply stops
and lists the conflicting paths; `mise dot apply --force` replaces them. A
real file needs `--force` even when its contents and permissions match the
source. To keep the file's content instead, [adopt it](#capturing-changes)
with `mise dot add`. An existing symlink that points elsewhere is repointed
without `--force`.

A `copy`, `template`, or inline `content` entry overwrites the target's
content without `--force`, and replaces a symlink at the target with a file.
A directory where a copied, templated, or inline file should go, or a file
where a directory copy should go, is still a conflict and needs `--force`.
Check `mise dot diff` before you change which source a target uses.

[Edit entries](/dotfiles/edits.html#conflicts) apply without `--force`.

## Unapply {#unapplying}

Deleting an entry from your config leaves its file, block, or line in place.
Run [`mise dot unapply <target>`](/cli/dotfiles/unapply.html) first, while
mise can still identify what the entry created, or replace the entry with
[`mode = "absent"`](#absent) to remove the file on machines that already
applied it. A `state = "absent"` declaration under
[`[bootstrap.files]`](/bootstrap/files.html#removing-resources) also removes a
file that no entry created.

`mise dot unapply` removes targets without removing their `[dotfiles]`
entries or source files. It uses the current config, the filesystem, and what
mise recorded about earlier applies to decide what each entry owns:

| Entry                                   | What unapply removes                                                                         |
| --------------------------------------- | -------------------------------------------------------------------------------------------- |
| `symlink`                               | The link, only while it still points to the configured source.                               |
| `symlink-each`                          | Exact source-to-target links, including dangling ones for deleted sources. Other files stay. |
| `copy` file, `template`, inline content | The file, only while its content still matches. A changed target needs `--force`.            |
| `copy` directory                        | Matching files one by one. Unmanaged files stay, and directories go only when empty.         |
| `permissions` only                      | Nothing.                                                                                     |
| `absent`                                | Nothing. mise does not recreate the file it removed.                                         |
| Block and line edits                    | Blocks with their markers. A plain line has no marker and needs `--force`. Merged keys stay. |

When a `symlink`, `copy`, `template`, or inline `content` target is removed,
the parent directories that mise created for it go too, once they are empty.
Directories that existed before mise wrote the target, that hold other files,
or that another remaining entry needs are kept. mise removes only directories
physically inside your home directory, never the home directory itself or
anything outside it, including through a symlinked parent.

When an entry needs `--force`, unapply stops before it removes anything and
names the entry. If you deleted a source file from a copied directory,
unapply cannot identify its old copy, so remove that file yourself. Use
`--dry-run` to preview removals.

Once a file is [tracked](/dotfiles/history.html), every `mise dot apply`,
`add`, `unapply`, and `edit` saves a checkpoint of your tracked files before
and after it runs, so [`mise dot undo`](/cli/dotfiles/undo.html) can reverse
the change.

## Files outside your home directory {#root-owned-files}

mise writes dotfiles as the user running it and never uses `sudo`. A target
such as `/etc/hosts` works only when mise runs as root, for example in a
container; otherwise apply fails with a permission error. To manage a whole
root-owned file from a normal user, declare it under
[`[bootstrap.files]`](/bootstrap/files.html) instead, which retries a change
with root privileges when the filesystem refuses it. Block and line edits have
no privileged equivalent, so editing a root-owned file such as `/etc/hosts`
still requires running mise as root.

## Manage the mise config itself {#self-managing-mise-config}

The global mise config and `dotfiles.root` can be dotfiles too:

```toml
[settings]
dotfiles.root = "~/.dotfiles"

[dotfiles]
"~/.dotfiles" = "~/src/dotfiles"
"~/.config/mise/config.toml" = "~/src/dotfiles/mise/config.toml"
```

Clone the real repository (`~/src/dotfiles` here) before the first
`mise dot apply` or `mise bootstrap`. Use the real repository path for sources
the first run needs, because `~/.dotfiles` does not exist until mise creates
that link. Replacing `~/.config/mise/config.toml` changes what every later
mise command reads, so make sure the source holds a valid config before you
apply it.

## Windows {#windows}

A `symlink` entry for a file creates a symbolic link when Windows allows it,
which it does without elevation once Developer Mode is on, and copies the file
otherwise. A directory link is a junction. `symlink-each` always copies each
file, so edit the source rather than the target and run `mise dot apply`.
`mise dot status` accepts either form on disk. `relative` and `permissions`
are ignored on Windows.
