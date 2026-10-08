---
description: "Write system files and directories with exact content, owner, group, and mode, using sudo only where a change needs it."
socialDescription: "Write system files and directories with exact content, owner, group, and mode."
---

# System files and directories

Use `[bootstrap.files]` and `[bootstrap.directories]` for paths that need
specific content, an owner, a group, or a mode, such as configuration under
`/etc` or a service's data directory. mise creates, updates, or removes each
declared path and uses `sudo` only for the changes that need it.

Both sections can also write files in your home directory. Use
[`[dotfiles]`](/dotfiles.html) for files you edit and keep in a dotfiles
repository, since mise can link them and save your edits back. Use
`[bootstrap.files]` for files outside your home directory, files that need a
particular owner or group, and files that should reload a service when they
change.

These sections work on Linux and macOS. On Windows, any entry that applies
makes `mise bootstrap`, `mise bootstrap plan`, and
[`mise bootstrap files`](/cli/bootstrap/files.html) fail, so give entries in a
shared config an [`os` selector](#choose-platforms) such as `unix`.

## Example

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

Create `files/example.conf` next to this `mise.toml`, then preview and apply
with [`mise bootstrap files apply`](/cli/bootstrap/files/apply.html):

```sh
mise bootstrap files apply --dry-run
mise bootstrap files apply
```

mise creates `/opt/example`, copies the source to `/etc/example.conf`, and sets
the declared owner, group, and mode on both. Because the entries name an owner,
both changes need root, and mise runs them through `sudo`. The full
[`mise bootstrap`](/bootstrap.html) applies these sections too.

## Set a file's content

Content comes from `source`, a file mise reads, or from inline `content`. A
file can use one of them, not both:

```toml
[bootstrap.files."/etc/sysctl.d/90-inotify.conf"]
content = """
fs.inotify.max_user_watches = 524288
"""
```

A relative `source` is resolved from the directory that contains the declaring
config file, and a `source` that starts with `~/` from your home directory.

Targets must be absolute paths or start with `~/`. mise refuses to manage `/`
itself. A file without `source` or `content` manages only its
[permissions](#permissions-without-content).

### Render content as a template

Set `template = true` to render the content with mise's
[template engine](/templates.html). Without it, a literal
<code v-pre>{{ ... }}</code> in the content is written unchanged.

```toml
[vars]
cache_dir = "/var/cache/example"

[bootstrap.files."/etc/example.conf"]
template = true
content = """
cache_dir = {{ vars.cache_dir }}
written_to = {{ target }}
"""
```

A file template can use `vars`, `env`, <code v-pre>{{ target }}</code> (the
destination path), and <code v-pre>{{ config_root }}</code>. Here
`config_root` is the directory that contains the declaring config file, for
example `.mise/` for `.mise/config.toml` or `~/.config/mise` for the global
config. This differs from [systemd units](/bootstrap/systemd.html),
[LaunchAgents](/bootstrap/launchd.html), and
[Compose projects](/bootstrap/compose.html), where `config_root` is the
config's root: the project directory, or `MISE_GLOBAL_CONFIG_ROOT` (default
`$HOME`) for the global config.

A template can read a declared [secret input](/bootstrap/secrets.html) with
<code v-pre>{{ secret(name="api_token") }}</code>. Secret values never appear
in plans, dry runs, status output, or the output of mise's privileged helper.
For a file that contains credentials, set `mode = "0600"` and an owner that
only the service account or root can read. See
[templates in bootstrap](/bootstrap.html#templates) for which bootstrap values
are templates and where `exec()` works.

### Remove the file when a template renders empty

Set `remove_empty = true` with `template = true` to delete the target whenever
the template renders to nothing but whitespace. One declaration can then turn a
file on and off:

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
removes it: the plan shows `absent (template rendered empty)`, and `notify`
fires as for any other change. mise still refuses to remove a directory at the
target. If a secret the template uses is unavailable, mise cannot tell whether
the template is empty, so status reports the file as `unknown` (not inspected),
mise leaves the file in place, and apply fails. The entry is still validated as
a present file, so its parent cannot be a directory declared absent.

## Create directories

A declared directory is created with any missing parents, like `mkdir -p`. The
declared owner, group, and mode apply only to the declared directory; parents
that mise creates on the way get the operating system defaults. Declare a
parent separately when it needs its own owner or mode.

Files are different: mise does not create a missing parent directory for a
`[bootstrap.files]` target. Status and plan still show `create`, but apply
fails because the parent does not exist. When the parent might not exist yet,
declare it in `[bootstrap.directories]`, which creates any missing parents as
well. mise creates declared directories before the files inside them.

## Choose platforms

Set `os` on a file or directory to manage it only on matching machines. It
takes one selector or a list. A selector is an operating system (`linux`,
`macos`, or `windows`), or `unix` for every platform except Windows, optionally
followed by an architecture, such as `linux/x64` or `macos/arm64`. It uses the
same names as [OS-specific tools](/dev-tools/#os-specific-tools), so aliases
such as `darwin`, `win`, and `aarch64` also work.

```toml
[bootstrap.files."/etc/docker/daemon.json"]
os = "linux"
source = "./files/docker-daemon.json"

[bootstrap.directories."~/.colima/default"]
os = "macos"

[bootstrap.files."~/.colima/default/colima.yaml"]
os = "macos"
source = "./files/colima.yaml"
```

The Colima file declares its parent directory because `~/.colima/default` does
not exist until Colima has run once.

An entry whose selectors do not match the machine is skipped as if it were not
declared: apply, status, and dry run ignore it, and mise does not touch the
target.

## Manage permissions only {#permissions-without-content}

Leave out `source` and `content` to manage a file's mode, owner, or group while
something else manages what it contains, such as a package, an installer, or
you:

```toml
[bootstrap.files."/etc/ssh/sshd_config"]
mode = "0600"
owner = "root"
```

Declare at least one of `mode`, `owner`, or `group`. mise compares and changes
only the declared fields: without `mode`, the mode stays as it is rather than
being reset to `0644`. The file is changed in place, so its content and inode
are untouched, and hard links and open handles keep pointing at it. Changing
the owner or group can clear setuid and setgid bits, as `chown` does; declare
`mode` to keep them.

These entries never create, replace, or remove the file:

- A missing target is skipped with a warning, and apply still succeeds.
- A symlink, directory, or other non-regular file is reported as `unknown`, and
  apply warns and leaves it alone. mise opens the file without following
  symlinks, so a change never lands on a symlink's target.
- `template`, `remove_empty`, and `replace` need `source` or `content`, so they
  are rejected here.
- [`mise bootstrap unapply`](/cli/bootstrap/unapply.html) keeps the file,
  because mise never managed its content.

`notify` fires when mise changes the file's permissions.

A mode change to a file you own runs as you. A declared `owner` or `group`, or
a mode change to another user's file, runs as root. On Linux and macOS, the
owner of a file can change its mode without `sudo` even when they cannot read
it; other Unix systems may retry that change through `sudo`. Where root would
cross an untrusted symlink, as described in
[How mise applies changes](#how-mise-applies-changes), apply warns and leaves a
permissions-only file unchanged instead of failing.

## Run files before packages {#files-before-packages}

Set `phase = "pre-packages"` on files and directories the package manager
needs, such as repository definitions and signing keys:

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

Put the vendor's key and repository definition in the source files. Changing
repository files does not refresh package metadata, so run
`mise bootstrap --update` to refresh it in the same run.

`mise bootstrap` applies files with `phase = "pre-packages"` after accounts and
package manager plugins and before the `pre-packages` hook. Files with the
default phase, `"post-packages"`, are applied after built-in package
installation. Each file is applied once per run.

A declared parent directory must be created no later than its children and
removed no earlier than them, so a `pre-packages` file inside a declared
directory needs that directory in the `pre-packages` phase too. Conflicting
phases fail validation before bootstrap changes anything.

`mise bootstrap --only files` runs both phases and `--skip files` skips both.
`--only packages` does not apply files. `mise bootstrap files apply` applies
every file and directory regardless of phase. Plans show each entry's phase.

## Restart a service after a change

Add `notify` to a file or directory to reload or restart
[system services](/bootstrap/services.html#system-services) when mise changes
it:

```toml
[bootstrap.files."/etc/docker/daemon.json"]
content = '{ "log-driver": "local" }'
notify = ["docker"]

[bootstrap.services.docker]
state = "running"
```

Each name must be a system service declared in `[bootstrap.services]`; a user
service or an undeclared name fails validation before anything changes. The
service's `on_change` setting chooses whether to reload, restart, or do
nothing, and a service declared `state = "stopped"` is never started by a
notification.

Notifications run only after a change succeeds. `mise bootstrap` collects them
from both file phases and runs them in its services step.
`mise bootstrap files apply` runs them right after its own changes; it also
offers any other pending changes to declared system services, behind a separate
prompt that `--yes` answers. `mise bootstrap services apply` does not run
notifications, because it makes no file changes to react to.
`mise bootstrap status` and `mise bootstrap plan` show the reloads and restarts
that pending file changes would cause.

## How configs combine

Entries merge by target path across the
[config hierarchy](/configuration.html#configuration-hierarchy). When several
config files declare the same path, the most local declaration is used as a
whole; mise does not merge keys across files. Within one file, two keys that
resolve to the same path, such as `"~/.oldrc"` and `"/home/you/.oldrc"`, are
an error.

A path cannot be declared as both a file and a directory, and a path declared
present cannot sit inside a directory declared absent.

## Remove files and directories {#removing-resources}

Deleting a declaration leaves its target in place. To remove a path, declare it
absent:

```toml
[bootstrap.files."/etc/obsolete.conf"]
state = "absent"

[bootstrap.directories."/opt/obsolete"]
state = "absent"

[bootstrap.files."~/.oldrc"]
state = "absent"
```

An absent file is removed whatever its content, and once it is gone, applying
again changes nothing. A file you own in your home directory is removed without
`sudo`.

A directory must be empty before mise removes it; apply fails otherwise. To
delete a directory and everything in it, add `recursive = true`. The dry run
shows this as `remove directory <path> recursively`, and mise runs it through
`sudo` so that an unreadable entry cannot stop it halfway.

To remove the files and directories a [machine module](/bootstrap/modules.html)
contributed, run `mise bootstrap unapply <env>` before you delete its
declarations. See
[Remove a module's resources](/bootstrap/modules.html#remove-a-module-s-resources).

## Preview and apply

```sh
mise bootstrap files status            # state of each declared path
mise bootstrap files status --json     # the same, with origin details
mise bootstrap files status --missing  # exit 1 if anything would change
mise bootstrap files apply --dry-run   # print the changes without making them
mise bootstrap files apply             # apply after a confirmation prompt
mise bootstrap files apply --yes       # apply without prompting
```

Status lists each path as `create`, `update`, `remove`, `unchanged`, or
`unknown`, with its current and desired state and the config that declared it.
`unknown` means mise cannot change the path safely: it has the wrong type, it
crosses an untrusted symlink, or it cannot be read. The status line says why,
and apply refuses it. Status may use `sudo` to read protected targets.

`--prompt-secrets` on `status` and `apply` prompts for missing
[secret inputs](/bootstrap/secrets.html) instead of failing.
`mise bootstrap plan` orders each file after its managed parent directory, and
removes children before their parents.

## How mise applies changes

mise compares content, type, mode, owner, and group, and changes only what
differs. A file is written to a temporary file in the target directory and then
renamed over the target, so readers never see a partial file.

Each change runs as you when it can, so paths you can write need no `sudo`.
When the filesystem refuses a change with a permission error, mise retries it
and the remaining changes in one batch through `sudo`. Some changes always use
`sudo`: a declared `owner` or `group`, replacing a path of another type, and
recursive removal. If you cannot read a target or search one of its parent
directories, mise inspects it through `sudo` too. File content reaches the
elevated helper on standard input, never on its command line. Set
[`system_packages.sudo = false`](/configuration/settings.html#system_packages.sudo)
to forbid elevation; changes that need root then fail unless mise already runs
as root.

When a change runs as you, symlinked parent directories are followed like any
other path, so `~/.ssh/config` works when `~/.ssh` is a symlink. When a change
runs as root, mise resolves the parent directories one at a time. It follows a
symlink only inside a directory that root owns and no other user can write, as
`/etc` is on macOS. Any other symlinked parent is refused, because a user who
controls it could redirect root's write to a file such as `/etc/shadow`.
Status and dry run report such a target as `unknown` with the symlink it
crosses, and apply fails. Declare the resolved path instead. When mise itself
runs as root, every change is handled this way.

A target of the wrong type, such as a directory where a file is declared, is
reported as `unknown`, and apply leaves it alone. Set `replace = true` to
replace it. A file replaces only an empty directory; to delete a directory with
contents, declare it absent with `recursive = true`.

## Reference

| Key            | Applies to  | Values                                                                      | Default                                                   |
| -------------- | ----------- | --------------------------------------------------------------------------- | --------------------------------------------------------- |
| `source`       | files       | Path to read the content from                                               |                                                           |
| `content`      | files       | Inline content                                                              |                                                           |
| `template`     | files       | `true` renders the content as a template                                    | `false`                                                   |
| `remove_empty` | files       | `true` removes the target when the template renders empty; needs `template` | `false`                                                   |
| `owner`        | both        | User name                                                                   | Not managed                                               |
| `group`        | both        | Group name                                                                  | Not managed                                               |
| `mode`         | both        | Octal string, such as `"0640"`                                              | `"0644"` for files with content, `"0755"` for directories |
| `state`        | both        | `"present"` or `"absent"`                                                   | `"present"`                                               |
| `recursive`    | directories | `true` deletes contents; only with `state = "absent"`                       | `false`                                                   |
| `replace`      | both        | `true` replaces a path of another type                                      | `false`                                                   |
| `phase`        | both        | `"pre-packages"` or `"post-packages"`                                       | `"post-packages"`                                         |
| `os`           | both        | Selector or list of selectors                                               | Every platform                                            |
| `notify`       | both        | Names of system services in `[bootstrap.services]`                          | `[]`                                                      |

An absent file cannot set `source`, `content`, or `remove_empty`. A file
without `source` or `content` that sets no `mode`, `owner`, or `group` is an
error.

### JSON output

The JSON output from `mise bootstrap plan`, `mise bootstrap status`, and
[`mise bootstrap files status`](/cli/bootstrap/files/status.html) includes an
`origin` object for each file and directory. It names the declaring config,
that config's `config_root`, any
[config environment](/configuration/environments.html) in its file name, and
the resolved source path when the file uses `source`. Paths are plain strings
when they are valid UTF-8. On Unix, a path that contains other bytes is written
as `mise:path-bytes:<base64url>`, so no information is lost.

## Troubleshooting

| Problem                                                  | What to do                                                                                                                           |
| -------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| Apply fails because a file's parent does not exist       | Declare the parent in `[bootstrap.directories]`; see [Create directories](#create-directories)                                       |
| A path shows as `unknown`                                | Read the reason in `mise bootstrap files status`. Set `replace = true` for a wrong type, or declare the resolved path past a symlink |
| A change that needs root fails without a terminal        | mise uses `sudo` there only when it needs no password. Run mise as root, allow passwordless `sudo`, or apply interactively           |
| An entry is missing from status                          | Its `os` selector does not match this machine                                                                                        |
| Bootstrap fails on Windows with "only supported on Unix" | Add `os = "unix"` to entries in configs that Windows machines load                                                                   |

## See also

- [Bootstrap](/bootstrap.html#how-it-runs) for where files fall in the run
  order.
- [Secret inputs](/bootstrap/secrets.html) for credentials in file templates.
- [Services](/bootstrap/services.html#system-services) for the services that
  `notify` reloads.
