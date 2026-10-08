---
description: "Create, update, and remove local Linux users and groups with mise bootstrap."
---

# Linux users and groups

`[bootstrap.groups]` and `[bootstrap.users]` declare local Linux accounts, such
as a service user and its groups. mise applies them before it writes
[system files](/bootstrap/files.html), so a file can name an account from the
same config as its owner.

## Example

This creates a system user for a cache service, with its own group and read
access to container logs through a second group:

```toml
[bootstrap.groups.mise-cache]
system = true

[bootstrap.groups.container-readers]
system = true

[bootstrap.users.mise-cache]
system = true
group = "mise-cache"
groups = ["container-readers"]
home = "/var/lib/mise-cache"
shell = "/usr/sbin/nologin"
comment = "mise cache service"
create_home = true
```

Preview the changes, then apply them:

```sh
mise bootstrap accounts apply --dry-run
mise bootstrap accounts apply
```

## Before you apply

Applying changes needs root. Unless you are already root, mise starts a
privileged copy of itself with sudo, and that copy runs the shadow-utils
commands `groupadd`, `groupmod`, `groupdel`, `useradd`, `usermod`, and
`userdel`. sudo can prompt for a password in an interactive terminal; otherwise
mise needs passwordless sudo.
[`system_packages.sudo = false`](/configuration/settings.html#system_packages.sudo)
forbids elevation, so only root can apply. Names reach these commands as
arguments, never through a shell.

Minimal images may lack those commands, and mise then stops with
`required account command 'useradd' was not found`. Install the package that
provides them, such as `shadow` on Alpine.

## Change existing accounts

mise manages only the fields you set and leaves the rest of an account as it
is. Inspect the dry run before you change an account that already exists: UID,
GID, and group changes affect the account database even for files outside this
config.

- `uid` and `gid` pin numeric IDs. If the requested ID belongs to another
  account, mise reports the resource as `unknown` and does not apply it.
- Changing an existing user's `uid` or primary `group` runs `usermod`, which
  also re-owns that user's files inside the home directory and mail spool.
  Files elsewhere keep the old IDs, and mise does not rewrite them.
- Changing a group's `gid` updates the group database only.
- `system = true` picks the system ID range when mise creates an account. It
  does not reclassify an account that already exists.
- `groups` adds missing supplementary groups and keeps memberships that are not
  listed. With `exclusive_groups = true`, the list is exact, and
  `groups = []` removes every supplementary membership.
- Changing `home` updates only the account entry, unless you also set
  `move_home = true` to move the existing directory.

## How configs combine

When several config files declare the same user or group, the most local
declaration is used as a whole; mise does not merge fields across files.

Within a run, mise creates and updates groups before users, and accounts before
the [system files and directories](/bootstrap/files.html) that name them as
owner or group. Removals run in the other order: users before the groups they
depend on. `mise bootstrap plan` shows these dependencies.

## Remove users and groups

Deleting a declaration leaves the account in place. To remove an account,
declare it absent:

```toml
[bootstrap.users.old-service]
state = "absent"
remove_home = true

[bootstrap.groups.old-service]
state = "absent"
```

mise keeps a user's home directory unless you set `remove_home = true`, which
also deletes the mail spool. mise refuses to remove UID 0, GID 0, or the user
running mise, and the operating system's own checks still apply; for example,
`groupdel` rejects a group that is still another user's primary group.

## Preview and apply

```sh
mise bootstrap accounts status
mise bootstrap accounts status --json
mise bootstrap accounts apply --dry-run
mise bootstrap accounts apply --yes
```

`mise bootstrap` applies accounts first, before every other part. To use a new
account as a file owner with `--only`, include both parts:
`mise bootstrap --only accounts,files`.

## Reference

Account names use at most 32 ASCII letters, digits, `_`, or `-`, start with a
letter or `_`, and may end with `$`.

User fields:

| Field              | Default                                 | Effect                                                                    |
| ------------------ | --------------------------------------- | ------------------------------------------------------------------------- |
| `group`            | Required, unless `state = "absent"`     | Primary group. Declare it in the same config or have it exist on the host |
| `groups`           | Not managed                             | Supplementary groups to add                                               |
| `exclusive_groups` | `false`                                 | Remove supplementary groups that `groups` does not list; needs `groups`   |
| `uid`              | Not managed                             | Numeric user ID                                                           |
| `home`             | Not managed                             | Home directory, as an absolute path                                       |
| `create_home`      | `true`, or `false` with `system = true` | Create the home directory for a new user                                  |
| `move_home`        | `false`                                 | Move the existing home directory when `home` changes; needs `home`        |
| `shell`            | Not managed                             | Login shell, as an absolute path                                          |
| `comment`          | Not managed                             | Comment (GECOS) field, without `:` or line breaks                         |
| `system`           | `false`                                 | Create the user in the system ID range                                    |
| `state`            | `"present"`                             | `"absent"` removes the user; it then takes only `remove_home`             |
| `remove_home`      | `false`                                 | With `state = "absent"`, also delete the home directory and mail spool    |

Group fields:

| Field    | Default     | Effect                                                             |
| -------- | ----------- | ------------------------------------------------------------------ |
| `gid`    | Not managed | Numeric group ID                                                   |
| `system` | `false`     | Create the group in the system ID range                            |
| `state`  | `"present"` | `"absent"` removes the group; it then cannot set `gid` or `system` |

## On macOS and Windows

Accounts are Linux-only. On another platform, `mise bootstrap`,
`mise bootstrap status`, and `mise bootstrap plan` ignore these declarations
with a warning, so one config can serve several platforms, while the explicit
`mise bootstrap accounts` commands fail instead of doing nothing. When a
[system file or directory](/bootstrap/files.html) names one of the ignored
accounts as its owner or group, that field is ignored with a warning too; the
file's content, mode, and any other owner or group are still applied.

## See also

- [Bootstrap](/bootstrap.html#how-it-runs) for where accounts fall in the run
  order.
- [System files and directories](/bootstrap/files.html) for files owned by these
  accounts.
