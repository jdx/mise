---
description: "Manage system files and directories, including paths that require root privileges."
socialDescription: "Manage system files and directories, including paths that require root privileges."
---

# System files and directories

`[bootstrap.files]` and `[bootstrap.directories]` declaratively manage absolute
paths that may require root privileges. They are separate from `[dotfiles]`,
which manages files in a user's home directory.

```toml
[bootstrap.directories."/opt/example"]
owner = "root"
group = "root"
mode = "0755"

[bootstrap.files."/etc/example.conf"]
source = "./files/example.conf"
owner = "root"
group = "root"
mode = "0644"
```

Create `files/example.conf` beside the declaring configuration before applying
this example. The source is read by mise; `/etc/example.conf` is its destination.
For file templates containing credentials, use mode `"0600"` and ownership that
allows only the intended service account or root to read them.

File content may come from `source` or inline `content`. Relative source paths
are resolved from the configuration file that declares them, and source paths
beginning with `~/` are resolved from the user's home directory. A file may
declare at most one content source; a present file with neither manages only
its [permissions](#permissions-without-content). Targets must be absolute paths
or begin with `~/`, which resolves from the user's home directory. mise refuses
to manage `/` itself.

Directory creation uses `mkdir -p` semantics, so missing parent directories are
created automatically. The configured ownership and mode apply to the declared
directory; implicitly created parents use the operating system defaults. Declare
a parent separately when it needs specific ownership or permissions.

By default, a target with the wrong node type is reported as `unknown` and
apply refuses to destroy it. Set `replace = true` on that file or directory to
replace the conflicting type. Replacing a directory with a file only removes
an empty directory; recursive destruction still requires an explicit
`state = "absent"` directory declaration with `recursive = true`.

Set `template = true` to render file content with mise's template engine. This
is explicit so literal <span v-pre>`{{ ... }}`</span> content remains untouched
by default. Templates can use configured `vars`, the declaring configuration's
directory as <span v-pre>`{{ config_root }}`</span>, and the destination as
<span v-pre>`{{ target }}`</span>. A template can consume a declared
[bootstrap secret input](/bootstrap/secrets.html) with
<span v-pre>`{{ secret(name="logical_name") }}`</span>. Secret values are never
included in plans, dry-run descriptions, status output, or privileged helper
output.

Set `remove_empty = true` beside `template = true` to remove the target when
the template renders to empty or whitespace-only content. This lets a single
declaration switch a file on and off from `vars` or the environment:

```toml
[vars]
proxy_host = "proxy.internal:3128"

[bootstrap.files."/etc/apt/apt.conf.d/95proxy"]
template = true
remove_empty = true
content = """
{% if vars.proxy_host %}Acquire::http::Proxy "http://{{ vars.proxy_host }}";
{% endif %}"""
```

With `proxy_host` set, mise writes the file. Set it to `""` and the next apply
removes `/etc/apt/apt.conf.d/95proxy`; the plan shows the removal as
`absent (template rendered empty)`, and `notify` services fire as for any other
removal. A directory at the target is still refused, as with
`state = "absent"`. When a secret the template needs is unavailable, mise cannot
tell whether the template is empty, so it never removes the file: status reports
it as not inspected and apply fails as for any other template. `remove_empty` is
rejected on files without `template = true` and on `state = "absent"` files.
The file's declared state remains present for validation, so its parent
directories must still allow a present file.

mise compares content, type, mode, owner, and group before applying changes.
Writes use a temporary file in the target directory followed by an atomic
rename. Changes are attempted as the current user first. If the filesystem
rejects an operation with a permission error, mise retries that operation and
the remaining ordered changes in one privileged batch. User-writable targets
therefore do not require `sudo`. If the current user cannot inspect a target or
search one of its parent directories, mise compares its metadata and content in
one privileged batch. Plans and file content are sent to narrowly scoped mise
helpers over stdin, so file content does not appear in process arguments or
logs.

## Permissions without content

Leave out `source` and `content` to manage a file's mode, owner, or group while
something else manages what it contains, such as a package, an installer, or
the user:

```toml
[bootstrap.files."/etc/ssh/sshd_config"]
mode = "0600"
owner = "root"
```

Declare at least one of `mode`, `owner`, or `group`. Only the declared fields
are compared and changed: without `mode`, the mode is left as it is rather than
reset to `0644`. mise changes the existing file in place, so its content and
inode are untouched and hard links and open handles keep pointing at it.
Changing the owner or group may clear setuid and setgid bits, as it does with
`chown`; declare `mode` to keep them.

These entries never create, replace, or remove the file:

- A missing target is skipped with a warning, and apply still succeeds.
- A symlink, directory, or other non-regular file is reported as `unknown`;
  apply warns and leaves it unchanged. The change is made through a handle
  opened without following symlinks, so it never lands on a symlink's target.
  On Linux, the owner of a file it cannot read can still change its mode
  without `sudo`.
- `template`, `remove_empty`, and `replace` require `source` or `content` and
  are rejected here.
- `mise bootstrap unapply` keeps the file, since mise never managed its content.

`notify` fires when mise changes the file's permissions.

How mise resolves the path depends on who makes the change. A mode change to
a file the current user owns is made as that user, and symlinked parent
directories are followed like any other path they open, so
`~/.ssh/config` works when `~/.ssh` is a symlink. A change that needs root,
such as a declared `owner` or `group` or a mode change to another user's file,
resolves the parent directories one at a time. A symlink in a directory owned by
root that no other user can write is followed, as `/etc` is on macOS. Any other
symlinked parent directory is refused, because a user who could write that
directory could otherwise redirect root's change to a file such as
`/etc/shadow`. Status and dry-run report such an entry as `unknown` with the
reason, and apply warns and leaves the file unchanged. Declare the resolved
path instead.

## Files before packages

Set `phase = "pre-packages"` on files and directories needed by the package
manager, such as repository definitions and signing keys:

```toml
[bootstrap.directories."/etc/apt/keyrings"]
mode = "0755"
phase = "pre-packages"

[bootstrap.files."/etc/apt/keyrings/vendor.asc"]
source = "./files/vendor.asc"
phase = "pre-packages"

[bootstrap.files."/etc/apt/sources.list.d/vendor.sources"]
source = "./files/vendor.sources"
phase = "pre-packages"

[bootstrap.packages]
"apt:vendor-tool" = "latest"
```

Provide the vendor's key and repository definition in the source files. Run
`mise bootstrap --update` to refresh package metadata after applying the files;
changing repository files does not automatically refresh metadata.

Early files run after accounts and package manager plugins, before the
`pre-packages` hook. The default phase is `"post-packages"`, which keeps files
after built-in package installation. Each file is applied once per bootstrap
run. Service notifications from both phases are collected for the services step.

Declared parent directories must be created no later than their children and
removed no earlier than their children. Conflicting phase declarations fail
validation before bootstrap makes changes. Set a declared parent's phase to
`"pre-packages"` when an early file needs it.

`mise bootstrap files apply` still applies all declared files and directories.
`mise bootstrap --only files` runs both file phases; `--skip files` skips both.
`--only packages` does not apply files. Plans include each file's phase.

## Preview and inspect

```sh
mise bootstrap files status --json
mise bootstrap files apply --dry-run
```

Check source paths, ownership, modes, and any `unknown` states before applying.
Inspection may need elevated access to read protected targets. A missing source
must be fixed in the configuration checkout; changing target permissions does
not supply that source.

## Removing resources

Removal is always explicit:

```toml
[bootstrap.files."/etc/obsolete.conf"]
state = "absent"

[bootstrap.directories."/opt/obsolete"]
state = "absent"
```

Targets in the home directory work the same way, which is useful for
retiring a file that an older setup created:

```toml
[bootstrap.files."~/.oldrc"]
state = "absent"
```

An absent file is removed whatever its content, and applying again once it is
gone changes nothing. A file you own in your home directory is removed without
`sudo`.

Directories must be empty before removal. Recursively deleting a directory
requires the additional `recursive = true` setting and is shown as a
destructive operation in the plan.

Removing a declaration from configuration does not remove its target.

## Commands

```sh
mise bootstrap files status
mise bootstrap files status --json
mise bootstrap files apply --dry-run
mise bootstrap files apply --yes
```

Files and directories may notify configured `[bootstrap.services]` after they
change:

```toml
[bootstrap.files."/etc/example/config.toml"]
content = "enabled = true"
notify = ["example"]
```

Notifications are applied by the full `mise bootstrap` flow after all managed
files converge. The dedicated `mise bootstrap files apply` command also runs
handlers after its file changes succeed. `mise bootstrap services apply`
converges lifecycle state only and never fires a handler before the causal file
change.

`mise bootstrap plan` includes these resources and automatically orders a
managed file after its managed parent directory. Removal reverses that
dependency so children are removed before their parent.

The JSON output from `mise bootstrap plan`, `mise bootstrap status`, and
`mise bootstrap files status` includes an `origin` object for managed files
and directories. It identifies the declaring config, that config's
`config_root`, any configuration environment encoded by its filename, and a
resolved source path when the file uses `source`.
Paths are ordinary strings when they are valid UTF-8. On Unix, a path containing
non-UTF-8 bytes uses `mise:path-bytes:<base64url>` so provenance remains lossless.
