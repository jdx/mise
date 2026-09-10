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
beginning with `~/` are resolved from the user's home directory. Present files
must declare exactly one content source. Targets must be absolute paths, and
mise refuses to manage `/` itself.

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
