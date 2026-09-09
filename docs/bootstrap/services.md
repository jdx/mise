---
description: "Run background services for your user, or manage existing Linux system services."
---

# Services

Use `[bootstrap.services]` to run a program in the background and start it
again when you log in or reboot. Choose the kind of service you need:

- [User services](#user-services) run programs as your current user on
  Linux, macOS, and Windows. The dotfile history watcher is one example.
- [System services](#system-services) start, stop, and configure existing
  Linux systemd units. This is the default scope for entries without `builtin`.

## User services

To save edits to your [tracked dotfiles](/dotfiles.html) automatically, add
this to your global mise configuration:

```toml
[bootstrap.services.mise-history]
builtin = "history-watch"
```

Install the service and check it:

```sh
mise bootstrap services apply
mise bootstrap dotfiles status
```

Once the watcher is running, keep editing your files normally. See
[automatic saves](/history.html#automatic-saves) for saving behavior and
[troubleshooting](#troubleshooting-user-services) if it fails to start.

### Run your own program

For a custom service, set `scope = "user"` and supply its command. This
example assumes you have installed `my-agent` at the given path:

```toml
[bootstrap.services.my-agent]
scope = "user"
command = "~/.local/bin/my-agent --serve"
```

Run `mise bootstrap services apply`, then `mise bootstrap services status`.
By default, the service starts at login and restarts after a failure.
If its program comes from `[tools]`, add `requires_tools = true` and run
the full `mise bootstrap` to install those tools first.

mise creates a service definition for your platform:

| platform | definition                                                                            | manager            |
| -------- | ------------------------------------------------------------------------------------- | ------------------ |
| Linux    | `~/.config/systemd/user/dev.mise.<name>.service`                                      | `systemctl --user` |
| macOS    | `~/Library/LaunchAgents/dev.mise.<name>.plist`                                        | `launchctl`        |
| Windows  | Scheduled Task `mise\<name>` (definition kept under `$MISE_STATE_DIR/user-services/`) | `schtasks`         |

### User service options

- `command`: the command line to run. `~` and `~/` are expanded. Required
  unless `builtin` is set.
- `builtin`: a service supplied by mise. `"history-watch"` runs
  `mise bootstrap dotfiles watch` at low priority. It sets `scope = "user"`
  and `restart = "on-failure"`. Use it without `command`.
- `description`: shown by the service manager.
- `restart`: `"on-failure"` (default), `"always"`, or `"never"`. Windows
  restarts after failures only; see [platform differences](#platform-differences).
- `environment`: environment variables passed to the program, for example
  `{ LOG_LEVEL = "info" }`.
- `working_directory`: the directory where the program runs.
- `state`: `"running"` (default), `"stopped"` (installed but not running), or
  `"absent"` (the installed definition is removed and stays removed while
  declared so).
- `enabled`: whether the service starts at login (default `true`). On macOS,
  setting this to `false` also disables restarting after a failure.
- `requires_tools`: install and start the service after `[tools]` and plugin
  package managers during bootstrap. The built-in watcher starts in the
  earlier services step because it only needs mise.

Names must contain only letters, numbers, `.`, `_`, or `-`, and must not also
appear in `[bootstrap.linux.systemd.units]` or
`[bootstrap.macos.launchd.agents]`: both would write the same definition.

### Remove and disable

`state = "absent"` removes the installed unit, agent, or task and keeps it
absent on later runs while declared so. Deleting the declaration leaves the
installed service in place until it is removed once:

```sh
mise bootstrap services remove my-agent
```

The next `mise bootstrap` recreates it if it is still declared.

### Status and apply

`mise bootstrap services status` and `mise bootstrap services apply` cover
both scopes; `mise bootstrap status` and `mise bootstrap plan` list user
services as `user-service:<name>`. `mise bootstrap status --json` includes
each user service's rendered definition under `user_services`, so what mise
would install can be inspected before applying. When the platform's user service manager is unavailable (for
example, no systemd user manager in a container), user services are reported
as `unknown` and skipped with a follow-up note; nothing is written.

Fields that only apply to user services (`command`, `builtin`, `description`,
`restart`, `environment`, `working_directory`, `requires_tools`, and
`state = "absent"`) require user scope. If you see an error about one of these
fields, check that the entry has `scope = "user"` or `builtin`.
Managed-file notifications apply to system services only.

### Troubleshooting user services

If the history watcher stops, run `mise doctor` and inspect the service logs.
On Linux, it allows three starts within five minutes before stopping retries.
On macOS, repeated launches are spaced at least five minutes apart.
These limits apply to the built-in watcher.

After fixing the cause on Linux, rerun `mise bootstrap` to reset the limit
and start the watcher. You can also restart it directly:

```sh
systemctl --user reset-failed dev.mise.mise-history.service
systemctl --user start dev.mise.mise-history.service
```

If you used another service name, replace `mise-history` in those commands.

#### Install mise at a permanent path {#durable-executable}

The watcher needs a mise executable that will still exist after setup ends.
If status reports `unknown: no durable mise executable; install mise on this
host first`, install mise on that host and apply the service again. For
remote bootstrap, use `--install-mise`.

mise writes an absolute executable path into built-in service definitions.
It uses the running binary unless that binary is in a temporary directory or
remote staging directory. In that case it looks for a permanent mise binary
on `PATH`. If it cannot find one, it leaves the service unwritten.

### Platform differences

On Linux, `restart` maps to systemd's `Restart`. On macOS it maps to
launchd's `KeepAlive`; `"on-failure"` uses `{ SuccessfulExit = false }`.

On Windows, `"always"` and `"on-failure"` both retry failed runs up to three
times, one minute apart. With `enabled = true`, the service also starts
again at logon. A successful exit leaves it stopped. If a Windows program
needs to keep running after completing its work, make it loop internally.

On macOS, `enabled` controls `RunAtLoad`. launchd also treats `KeepAlive` as
a request to start when loaded. mise omits `RunAtLoad` for a stopped service
and omits `KeepAlive` when `enabled = false`. A running service with
`enabled = false` starts once on apply, then stays stopped after an exit
until you start it again or re-enable it.

On Windows, setting `environment` uses `cmd.exe`. Values containing `%`,
`"`, `&`, `|`, `<`, `>`, or `^` are rejected. When `environment` is set,
`command` also rejects `%`, `&`, `|`, `<`, `>`, and `^`. Move such a command
into a script or set variables in the program. Without `environment`, the
command runs directly.

## System services

Package installation and `[bootstrap.files]` run first, so a service may be
installed by a package or supplied as a managed unit file. After file changes,
mise reloads systemd before applying service changes.

```toml
[bootstrap.packages]
"apt:docker.io" = "latest"

[bootstrap.services.docker]
state = "running"
enabled = true
```

Names without a unit suffix receive `.service`. Explicit unit names such as
`postgresql@16-main.service`, sockets, and timers are also accepted.

This section manages system units already supplied by packages or
[managed files](/bootstrap/files.html). A service that runs as your user is a
[user service](#user-services) (`scope = "user"`, above); hand-written user
units go through [systemd user units](/bootstrap/systemd.html).

Preview with `mise bootstrap services apply --dry-run`. If the unit will be
created by the same configuration, use the full bootstrap to install its package
or file before converging the service.

### System service options

- `state`: `"running"` (default) or `"stopped"`
- `enabled`: whether the unit starts at boot (default `true`)
- `masked`: whether systemd must prevent the unit from starting (default
  `false`)
- `on_change`: action to take when a changed managed file or directory
  notifies the service: `"reload_or_restart"` (default), `"reload"`,
  `"restart"`, or `"none"`

Managed files and directories can notify one or more services. Notifications
run only after a resource actually changes; dry runs show the same action. A
notification never starts or restarts a service declared `state = "stopped"`;
`on_change` applies only while the desired service state is running.

```toml
[bootstrap.files."/etc/docker/daemon.json"]
content = '{ "log-driver": "local" }'
notify = ["docker"]

[bootstrap.services.docker]
state = "running"
enabled = true
on_change = "reload_or_restart"
```

Notification names are validated before any bootstrap mutation, so a typo
cannot leave a host partially provisioned. mise runs one `daemon-reload`,
re-inspects all affected units, and validates every action before changing any
service. A missing unit is retried only when the changed notification source is
that unit's managed file in a systemd system-unit search directory (including
an instantiated unit's `name@.service` template). A notification from an
ordinary configuration file cannot make an unrelated missing unit appear and
therefore remains `unknown`. This allows a unit newly written by
`[bootstrap.files]` to be started safely without weakening fail-closed behavior.
Once an interactive user confirms a managed-file change, its notification
handlers run as part of that confirmed change; unrelated service drift remains
separately confirmable.

`mise bootstrap services status` and `mise bootstrap services apply` inspect
and converge service lifecycle state only. They do not synthesize a file
notification before its file has changed. Aggregate `mise bootstrap status`
and `mise bootstrap plan` include the notification consequences of pending
managed-file changes, while `mise bootstrap files apply` runs those handlers
only after the causal file operation succeeds.

Removing a service declaration leaves its current state unmanaged. To stop it
and prevent future starts, keep an explicit declaration. A masked unit must
also be stopped and disabled:

```toml
[bootstrap.services.old-worker]
state = "stopped"
enabled = false
masked = true
```

mise does not guess when a unit is missing, systemd is unavailable, or a unit
cannot be enabled (for example, a static unit). Status and plans report the
resource as `unknown`; apply fails closed instead of running an unsafe command.

```sh
mise bootstrap services status
mise bootstrap services status --json
mise bootstrap services apply --dry-run
mise bootstrap services apply --yes
```

System service management is Linux-only and requires root privileges. mise
prompts through sudo only when a change is required.
