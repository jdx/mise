---
description: "Run your own programs and mise's built-in services in the background, or start and enable existing Linux system services."
socialDescription: "Run background services for your user, or manage Linux system services."
---

# Services

Use `[bootstrap.services]` to keep a program running in the background as your
user on Linux, macOS, and Windows, or to start, stop, and enable the Linux
systemd units that a package installed. Several bootstrap sections can run
something in the background; pick the one that fits:

| You want to                                                           | Use                                                           |
| --------------------------------------------------------------------- | ------------------------------------------------------------- |
| Run one program in the background on Linux, macOS, and Windows        | A [user service](#user-services) (`scope = "user"`)           |
| Save dotfile edits or update tools in the background                  | A [built-in service](#built-in-services)                      |
| Start, stop, or enable a unit that a package or managed file provides | A [system service](#system-services)                          |
| Use systemd features such as timers, dependencies, or hardening       | [`[bootstrap.linux.systemd.units]`](/bootstrap/systemd.html)  |
| Use launchd features such as calendar schedules or queue directories  | [`[bootstrap.macos.launchd.agents]`](/bootstrap/launchd.html) |
| Run containers                                                        | [`[bootstrap.compose]`](/bootstrap/compose.html)              |

## User services

A user service runs a program as you, starts it when you log in, and restarts
it after a failure. mise writes the service definition your platform's service
manager expects, without root.

### Run your own program

Set `scope = "user"` and the command to run, then start it with
[`mise bootstrap services apply`](/cli/bootstrap/services/apply.html) and check
it with [`mise bootstrap services status`](/cli/bootstrap/services/status.html).
This example assumes `my-agent` is installed at that path:

```toml
[bootstrap.services.my-agent]
scope = "user"
command = "~/.local/bin/my-agent --serve"
```

```sh
mise bootstrap services apply
mise bootstrap services status
```

mise creates a definition for your platform and starts the service:

| Platform | Definition                                                                               | Manager            |
| -------- | ---------------------------------------------------------------------------------------- | ------------------ |
| Linux    | `~/.config/systemd/user/dev.mise.<name>.service`                                         | `systemctl --user` |
| macOS    | `~/Library/LaunchAgents/dev.mise.<name>.plist`                                           | `launchctl`        |
| Windows  | Scheduled Task `mise\<name>`, with its definition under `$MISE_STATE_DIR/user-services/` | `schtasks`         |

mise runs `command` directly, not through a shell, so pipes, redirection, and
globs do not work there; put such a command in a script or run `sh -c '...'`.
A leading `~/` in the program path is expanded.

On Linux, the command becomes the unit's `ExecStart=` line, so systemd expands
`$VAR`, `${VAR}`, and specifiers such as `%h`; write `$$` or `%%` for a literal
`$` or `%`. launchd on macOS receives the words unchanged, so a command meant
for both platforms should not rely on `$VAR` or `%` specifiers; write the
values out, or set the variables the program reads in `environment`.

The service does not get your shell's mise activation, so tools from `[tools]`
are not on its `PATH`. Use absolute paths, or run the tool through
[`mise exec`](/cli/exec.html), and add `requires_tools = true` so that the full
`mise bootstrap` installs your tools before it starts the service.

[Set up a development stack](/daemons/development-stack.html#keep-the-supervisor-available-at-login)
shows a complete example: a user service that keeps the Pitchfork supervisor
running from login.

### Built-in services

A built-in service runs mise itself. Name it with `builtin`, which also implies
`scope = "user"`:

```toml
[bootstrap.services.mise-history]
builtin = "history-watch"
```

| `builtin`         | What it does                                                                       | Guide                                                        |
| ----------------- | ---------------------------------------------------------------------------------- | ------------------------------------------------------------ |
| `"history-watch"` | Runs `mise dot watch`, which saves edits to your tracked files as you make them    | [Automatic saves](/dotfiles/history.html#automatic-saves)    |
| `"tool-update"`   | Checks tools that set `auto_update` once an hour and updates the ones that are due | [Automatic tool updates](/dev-tools/#automatic-tool-updates) |

Both run at low priority, restart after a failure, and need no `command`.
Declare them in your global config, `~/.config/mise/config.toml`, because they
act on your whole account rather than one project. A built-in service starts in
the services step of `mise bootstrap`, before tools are installed, because it
needs only mise.

### Install mise at a permanent path {#durable-executable}

Built-in services run mise, so the service definition records the absolute
path of a mise binary that must stay in place after setup. mise uses the
binary that is running, unless it lives in a temporary or remote staging
directory; then it uses a permanent `mise` on `PATH`. If it finds none, status
reports `unknown: no durable mise executable; install mise on this host first`
and mise leaves the service unwritten. Install mise on the host and apply
again; for [remote bootstrap](/bootstrap/remote.html), pass `--install-mise`.

### User service options

| Key                 | Values                                                                                  | Default                                       |
| ------------------- | --------------------------------------------------------------------------------------- | --------------------------------------------- |
| `scope`             | `"user"` or `"system"`                                                                  | `"user"` with `builtin`, otherwise `"system"` |
| `command`           | Command line to run                                                                     | Required unless `builtin` is set              |
| `builtin`           | `"history-watch"` or `"tool-update"`                                                    |                                               |
| `description`       | Text the service manager shows                                                          |                                               |
| `restart`           | `"on-failure"`, `"always"`, or `"never"`                                                | `"on-failure"`                                |
| `environment`       | Table of environment variables, such as `{ LOG_LEVEL = "info" }`                        |                                               |
| `working_directory` | Directory to run in; `~` is expanded                                                    |                                               |
| `state`             | `"running"`, `"stopped"` (installed but not running), or `"absent"`                     | `"running"`                                   |
| `enabled`           | `true` starts the service at login                                                      | `true`                                        |
| `requires_tools`    | `true` starts the service after `mise bootstrap` installs `[tools]` and plugin packages | `false`                                       |

`command`, `builtin`, `description`, `restart`, `environment`,
`working_directory`, `requires_tools`, and `state = "absent"` apply only to user
services. An error that names one of them usually means the entry is missing
`scope = "user"`. User services cannot set `masked` or `on_change`, and
[`notify`](/bootstrap/files.html#restart-a-service-after-a-change) from
managed files reaches system services only.

Names may contain only letters, numbers, `.`, `_`, and `-`. A name cannot also
appear in `[bootstrap.linux.systemd.units]` or
`[bootstrap.macos.launchd.agents]`, because both would write the same
definition.

### Platform differences

On Linux, user services run in your systemd user manager, which starts when you
log in and stops when your last session ends. To keep them running on a server
without a login session, enable lingering once with
`sudo loginctl enable-linger $USER`; mise does not do this for you. `restart`
maps to systemd's `Restart=`, with five seconds between attempts.

On macOS, `restart` maps to launchd's `KeepAlive`, and `"on-failure"` uses
`{ SuccessfulExit = false }`. `enabled` controls `RunAtLoad`. launchd also
treats `KeepAlive` as a request to start when loaded, so mise omits `RunAtLoad`
for a stopped service and omits `KeepAlive` when `enabled = false`. A running
service with `enabled = false` starts once on apply, then stays stopped after
it exits until you start it again or enable it.

On Windows, `"always"` and `"on-failure"` both retry a failed run up to three
times, one minute apart. With `enabled = true`, the task also starts at logon.
A successful exit leaves it stopped, so a program that should keep running must
loop on its own. Task Scheduler cannot set environment variables, so a service
that sets `environment` runs through mise, which applies the variables and then
starts `command` directly, without `cmd.exe`. Values and the command are passed
as written, including characters such as `%`, `&`, and `|`. Variable names must
not be empty or contain `=`, and no name or value may contain a NUL character.
This needs mise installed at a [permanent path](#durable-executable); without
one, status reports the service as `unknown`. Without `environment`, the
command runs directly.

### Remove a user service {#remove-and-disable}

`state = "absent"` removes the installed unit, agent, or task, and keeps it
removed while the entry says so. Deleting the declaration instead leaves the
service installed; remove it once with
[`mise bootstrap services remove`](/cli/bootstrap/services/remove.html):

```sh
mise bootstrap services remove my-agent
```

The command works whether or not the service is still declared. If it is, the
next `mise bootstrap` installs it again.
[`mise bootstrap unapply`](/bootstrap/modules.html#remove-a-module-s-resources)
also removes the user services that a machine module declared.

### Troubleshooting user services

If the platform's service manager is unavailable, for example in a container
without a systemd user manager, mise reports user services as `unknown` and
skips them with a warning, which `mise bootstrap` repeats in its follow-up
summary. It writes nothing.

Built-in services have start limits so that a crash loop stops: on Linux,
systemd allows three starts within five minutes and then stops retrying; on
macOS, launchd waits at least five minutes between launches. These limits apply
to both built-in services, not to services you define with `command`. Run
`mise doctor` and read the service's logs, fix the cause, then run
`mise bootstrap services apply` or `mise bootstrap` again, which resets the
limit and starts the service. On Linux you can also restart it directly:

```sh
systemctl --user reset-failed dev.mise.mise-history.service
systemctl --user start dev.mise.mise-history.service
```

Replace `mise-history` with your service's name. If the history watcher runs
but does not save your files, see
[Checking watcher health](/dotfiles/history.html#health).

## System services

System services start, stop, enable, and mask systemd units that a package or a
[managed file](/bootstrap/files.html) provides. They are Linux-only and need
root: mise uses `sudo` only when a change is needed, and
[`system_packages.sudo = false`](/configuration/settings.html#system_packages.sudo)
forbids it. `mise bootstrap` applies packages and files first and reloads
systemd after file changes, so one configuration can install a unit and start
it:

```toml
[bootstrap.packages]
"apt:docker.io" = "latest"

[bootstrap.services.docker]
state = "running"
enabled = true
```

A name without a unit suffix gets `.service`. Full unit names such as
`postgresql@16-main.service`, sockets, and timers work too. When the unit comes
from a package or file in the same config, preview with the full
`mise bootstrap --dry-run`; `mise bootstrap services apply` on its own does not
install the package or write the file first.

### System service options

| Key         | Values                                                      | Default               |
| ----------- | ----------------------------------------------------------- | --------------------- |
| `state`     | `"running"` or `"stopped"`                                  | `"running"`           |
| `enabled`   | `true` starts the unit at boot                              | `true`                |
| `masked`    | `true` prevents the unit from starting at all               | `false`               |
| `on_change` | `"reload_or_restart"`, `"reload"`, `"restart"`, or `"none"` | `"reload_or_restart"` |

### Reload a service when its files change

A managed file or directory can `notify` a system service when mise changes
it; [Restart a service after a change](/bootstrap/files.html#restart-a-service-after-a-change)
covers the file side and which commands run notifications. The service's
`on_change` value chooses what happens to a running unit:

| `on_change`           | Command                       |
| --------------------- | ----------------------------- |
| `"reload_or_restart"` | `systemctl reload-or-restart` |
| `"reload"`            | `systemctl reload`            |
| `"restart"`           | `systemctl restart`           |
| `"none"`              | None                          |

```toml
[bootstrap.services.docker]
on_change = "restart"
```

Before it changes any system service, mise runs one `systemctl daemon-reload`.
A unit that does not exist yet is accepted only when the same run writes its
unit file (or its `name@.service` template) into a systemd unit directory such
as `/etc/systemd/system` through `[bootstrap.files]`, and that file entry lists
the service in `notify`.

### Stop and disable a service

Deleting a declaration leaves the unit in whatever state it is in. To stop it
and keep it from starting again, keep a declaration. A masked unit must also be
stopped and disabled:

```toml
[bootstrap.services.old-worker]
state = "stopped"
enabled = false
masked = true
```

mise does not guess when a unit is missing, systemd is unavailable, or a unit
cannot be enabled, such as a static unit. Status and plans report the service
as `unknown`, and apply fails instead of running a command that might not be
safe.

## How configs combine

Services merge by name across the
[config hierarchy](/configuration.html#configuration-hierarchy). A more local
config replaces the whole declaration for that name; mise does not merge keys
across files.

## Preview and apply

`mise bootstrap services` covers both scopes:

```sh
mise bootstrap services status            # state of each service
mise bootstrap services status --json     # the same, as JSON
mise bootstrap services status --missing  # exit 1 if any service would change
mise bootstrap services apply --dry-run   # print the commands
mise bootstrap services apply             # apply after a confirmation prompt
mise bootstrap services apply --yes       # apply without prompting
mise bootstrap services remove <name>     # remove an installed user service
```

`mise bootstrap status` and `mise bootstrap plan` list user services as
`user-service:<name>`. `mise bootstrap status --json` includes each user
service's generated definition under `user_services`, so you can inspect it
before applying.

## On macOS and Windows

User services work on all three platforms. System services are Linux-only: on
macOS and Windows, `mise bootstrap`, `mise bootstrap status`, and
`mise bootstrap plan` ignore them, while `mise bootstrap services status` and
`apply` fail when one is declared. Keep system services in a config that only
Linux machines load, such as a [machine module](/bootstrap/modules.html).

## See also

- [Bootstrap](/bootstrap.html#how-it-runs) for where services fall in the run
  order.
- [System files and directories](/bootstrap/files.html) for unit files and the
  configuration that `notify` watches.
- [systemd user units](/bootstrap/systemd.html) and
  [macOS LaunchAgents](/bootstrap/launchd.html) for platform-specific features.
