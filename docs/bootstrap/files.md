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

File content may come from `source` or inline `content`. Relative source paths
are resolved from the configuration file that declares them. Present files
must declare exactly one content source. Targets must be absolute paths, and
mise refuses to manage `/` itself.

Mise creates only directories that are explicitly declared. If multiple
missing levels are needed, declare each directory so its ownership and mode are
intentional; mise never creates undeclared ancestors with process defaults.

By default, a target with the wrong node type is reported as `unknown` and
apply refuses to destroy it. Set `replace = true` on that file or directory to
replace the conflicting type. Replacing a directory with a file only removes
an empty directory; recursive destruction still requires an explicit
`state = "absent"` directory declaration with `recursive = true`.

Set `template = true` to render file content with mise's template engine. This
is explicit so literal <span v-pre>`{{ ... }}`</span> content remains untouched
by default. A template can consume a declared
[bootstrap secret input](/bootstrap/secrets.html) with
<span v-pre>`{{ secret(name="logical_name") }}`</span>. Secret values are never
included in plans, dry-run descriptions, status output, or privileged helper
output.

Mise compares content, type, mode, owner, and group before applying changes.
Writes use a temporary file in the target directory followed by an atomic
rename. If the current user cannot inspect a target or search one of its parent
directories, mise compares its metadata and content in one privileged batch.
Plans and file content are sent to narrowly scoped mise helpers over stdin, so
file content does not appear in process arguments or logs.

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
