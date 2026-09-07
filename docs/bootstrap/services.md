# Services

`[bootstrap.services]` declares services in two scopes:

- **System services** (the default) manage the lifecycle of existing Linux
  systemd system units: start, stop, enable, mask, and reload on change.
- **User services** (`scope = "user"`) are services mise defines for the
  current user, declared once and installed on every platform: a systemd user
  unit on Linux, a LaunchAgent on macOS, a Scheduled Task on Windows.

## User services

```toml
[bootstrap.services.mise-history]        # the built-in history watcher
builtin = "history-watch"                # implies scope = "user"

[bootstrap.services.my-agent]
scope = "user"
command = "~/.local/bin/my-agent --serve"
description = "My agent"
restart = "on-failure"                   # "always" | "on-failure" | "never"
environment = { RUST_LOG = "info" }
working_directory = "~"
requires_tools = true                    # converge after [tools] are installed
```

One declaration is rendered for the platform's user service manager:

| platform | definition                                                                            | manager            |
| -------- | ------------------------------------------------------------------------------------- | ------------------ |
| Linux    | `~/.config/systemd/user/dev.mise.<name>.service`                                      | `systemctl --user` |
| macOS    | `~/Library/LaunchAgents/dev.mise.<name>.plist`                                        | `launchctl`        |
| Windows  | Scheduled Task `mise\<name>` (definition kept under `$MISE_STATE_DIR/user-services/`) | `schtasks`         |

### User service options

- `command`: the command line to run. `~` and `~/` are expanded. Required
  unless `builtin` is set.
- `builtin`: a definition mise supplies. `"history-watch"` runs
  `mise bootstrap dotfiles watch` through a durable mise executable with
  `restart = "on-failure"` and a low priority. A builtin implies
  `scope = "user"`; `command` cannot be combined with it.
- `description`: shown by the service manager.
- `restart`: `"on-failure"` (default), `"always"`, or `"never"`. On Linux this
  is `Restart=`; on macOS `KeepAlive` (`{ SuccessfulExit = false }` for
  on-failure). Task Scheduler restarts only failed runs, so on Windows
  `"always"` and `"on-failure"` both restart up to three times a minute apart
  after a failure and run again at logon (when `enabled = true`); a clean
  exit is not restarted. Strict `"always"` semantics are a Linux and macOS
  feature; a service that must survive a clean exit on Windows should loop
  inside its own program.
- `environment` and `working_directory` map directly to the platform
  definition. On Windows, environment variables are set through `cmd.exe`,
  so values containing characters it would reinterpret (`%`, `"`, `&`, `|`,
  `<`, `>`, `^`) are rejected, and so is a `command` containing `%`, `&`,
  `|`, `<`, `>`, or `^` once `environment` is set (without `environment`
  the command runs directly). Move such a command into a script, or set the
  variables inside the program.
- `state`: `"running"` (default), `"stopped"` (installed but not running), or
  `"absent"` (the installed definition is removed and stays removed while
  declared so).
- `enabled`: whether the service starts at login (default `true`). On macOS
  this is `RunAtLoad`, which launchd also honours when the agent is loaded,
  so a stopped agent is written without it (it starts at login again once it
  is set running). launchd reads any `KeepAlive` as run-at-load too, so an
  agent with `enabled = false` is written without one: it is started once by
  the apply but neither starts at login nor is restarted after a failure
  until it is enabled again.
- `requires_tools`: converge in a second pass after `[tools]` and plugin
  package managers, so a service that runs a tool starts after it exists. The
  built-in watcher needs only mise and converges in the services step.

Names must contain only letters, numbers, `.`, `_`, or `-`, and must not also
appear in `[bootstrap.linux.systemd.units]` or
`[bootstrap.macos.launchd.agents]`: both would write the same definition.

### Durable executable

A builtin is written with an absolute path to the mise that installed it.
mise uses the running executable unless it lives in a temporary directory or
in the staging directory of `mise bootstrap remote`, and otherwise a `mise`
found on `PATH` outside those. When only a staged binary exists the service is
reported as `unknown: no durable mise executable; install mise on this host
first` and is never written with a path that will be deleted.

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
`state = "absent"`) are rejected on a system-scope entry, so a missing
`scope = "user"` cannot silently turn a service definition into a lookup of a
system unit. Managed-file notifications apply to system services only.

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
