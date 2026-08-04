# Linux users and groups

`[bootstrap.groups]` and `[bootstrap.users]` declaratively manage local Linux
accounts. Mise applies groups before users and applies accounts before privileged
files, so a managed file can safely refer to an account created in the same
configuration.

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

Present users require an explicit primary `group`. The group may be managed in
the same configuration or already exist on the host. Optional `uid` and `gid`
fields pin numeric IDs; mise reports `unknown` and refuses to apply when a
requested ID belongs to another account. `system = true` selects the platform's
system-account range when creating an account and does not reclassify an
existing account. Changing an existing numeric UID or GID updates the account
database only; mise does not recursively rewrite ownership of existing files.

User fields are convergent when specified and unmanaged when omitted:

- `uid`, `group`, `home`, `shell`, and `comment` manage the corresponding
  passwd fields.
- `groups` manages supplementary groups. By default mise only adds missing
  memberships and preserves memberships not listed in the config.
- `exclusive_groups = true` makes `groups` exact. An explicit `groups = []`
  then removes every supplementary membership.
- `create_home` controls creation of a new user's home. It defaults to `true`
  for regular users and `false` for system users.
- `move_home = true` moves an existing home when changing `home`; without it,
  only the passwd entry changes.

Names are passed as typed process arguments, never through a shell. Mise uses
the standard shadow-utils commands (`groupadd`, `groupmod`, `groupdel`,
`useradd`, `usermod`, and `userdel`) inside its narrowly scoped elevated helper.
The feature is Linux-only.

On a non-Linux host, aggregate commands such as `mise bootstrap`,
`mise bootstrap status`, and `mise bootstrap plan` ignore these declarations
with a warning so one configuration can be shared across platforms. Explicit
`mise bootstrap accounts` commands fail instead of silently doing nothing.
When a managed file or directory names one of these ignored declarations as
its owner or group, that ownership field is ignored with a warning too. Its
content, mode, and any unrelated local owner or group still converge normally.

## Removal

Removal is explicit and ordered users-before-groups:

```toml
[bootstrap.users.old-service]
state = "absent"
remove_home = true

[bootstrap.groups.old-service]
state = "absent"
```

User homes are preserved by default. Set `remove_home = true` only when the
account's home and mail spool should also be deleted. Mise refuses to remove
UID 0, GID 0, or the user running mise. It also leaves the operating system's
normal safeguards in place; for example, `groupdel` rejects a group that is
still another user's primary group.

## Commands

```sh
mise bootstrap accounts status
mise bootstrap accounts status --json
mise bootstrap accounts apply --dry-run
mise bootstrap accounts apply --yes
```

`mise bootstrap plan` includes account resources and their dependencies.
Managed groups precede users that reference them, managed accounts precede
present files and directories that name them as owner or group, and absent
users precede managed group removal.
