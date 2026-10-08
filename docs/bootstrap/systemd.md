---
description: "Declare systemd user services and timers in mise.toml, then write, enable, and start them with systemctl --user."
socialDescription: "Declare systemd user services and timers in mise.toml."
---

# systemd user units

Declare systemd user services and timers in `[bootstrap.linux.systemd.units]`.
[`mise bootstrap`](/bootstrap.html), or
[`mise bootstrap linux systemd-units apply`](/cli/bootstrap/linux/systemd-units/apply.html),
writes each to `~/.config/systemd/user/dev.mise.<name>.service` (or `.timer`)
and enables and starts it with `systemctl --user`.

Units run as you, in your systemd user manager. To run one program in the
background on Linux, macOS, and Windows from a single declaration, use a
[user service](/bootstrap/services.html#user-services) instead; a name cannot
be declared in both places, because both write the same unit file. Use this
section for systemd features such as timers, unit dependencies, hardening, and
environment files. For units that the system manager runs, such as a database
installed by a package, use [system services](/bootstrap/services.html#system-services).

## Example

Install your program first. `my-sync` stands in for it:

```toml
[bootstrap.linux.systemd.units.my-sync]
description = "sync files"
exec_start = "~/.local/bin/my-sync --watch"
restart = "on-failure"
```

```sh
mise bootstrap linux systemd-units apply --dry-run
mise bootstrap linux systemd-units apply
```

mise writes `~/.config/systemd/user/dev.mise.my-sync.service`, runs
`systemctl --user daemon-reload`, enables the unit for `default.target`, and
restarts it. systemd restarts the program when it fails.

A unit does not get your shell's mise activation, so tools from `[tools]` are
not on its `PATH`. Use absolute paths, or `~/` paths in the `exec_*` keys, and
set the environment the program needs. `exec_start` uses systemd's command
syntax, not a shell: for pipes or redirection, run `/bin/sh -c '...'` or a
wrapper script.

## Run a command once at login

A `oneshot` service runs a command and finishes. With
`remain_after_exit = true`, systemd keeps it marked active, so `exec_stop` runs
when the unit stops, for example when your user manager shuts down:

```toml
[bootstrap.linux.systemd.units.daemon-lifecycle]
type = "oneshot"
remain_after_exit = true
exec_start = "~/.local/bin/daemon start"
exec_stop = "~/.local/bin/daemon stop"
timeout_start_sec = "120"
timeout_stop_sec = "30"
no_new_privileges = true
private_tmp = true
```

## Tie a service to the desktop session

A service can start and stop with your graphical session and run a check before
it starts:

```toml
[bootstrap.linux.systemd.units.panel]
description = "desktop panel"
part_of = ["graphical-session.target"]
after = ["graphical-session.target"]
exec_start_pre = ["~/.local/bin/panel --check-config"]
exec_start = "~/.local/bin/panel"
wanted_by = ["graphical-session.target"]
```

`wanted_by` only starts the service with the session; `part_of` also stops and
restarts it with `graphical-session.target`. If an `exec_start_pre` command
fails, systemd does not run `exec_start`, and `systemctl --user status` shows
the check as the command that failed.

## Run on a schedule

An entry that sets any timer key becomes a `.timer` unit. Pair it with a
service entry for the work, and point the timer at that service with `unit`:

```toml
[bootstrap.linux.systemd.units.backup]
type = "oneshot"
exec_start = "~/.local/bin/backup"
start = false
wanted_by = []

[bootstrap.linux.systemd.units.backup-daily]
on_calendar = "daily"
persistent = true
unit = "backup"
```

The service sets `start = false` and `wanted_by = []`, so apply leaves it
stopped and not enabled, and only the timer starts it. `persistent = true` runs
a job that was missed while the timer was not running, such as while the
machine was off or you were logged out, as soon as the timer starts again. It
applies only to `on_calendar`, not to monotonic timers. See the
[systemd timer reference](https://www.freedesktop.org/software/systemd/man/latest/systemd.timer.html#Persistent=).

A monotonic timer runs relative to boot or to the service's last run. This one
assumes a `healthcheck` service entry declared like `backup` above:

```toml
[bootstrap.linux.systemd.units.healthcheck-timer]
description = "periodically check daemon health"
on_boot_sec = "2min"
on_unit_inactive_sec = "5min"
randomized_delay_sec = "30s"
unit = "healthcheck"
```

A bare `unit` value names the service that mise writes for that entry, so
`unit = "backup"` targets `dev.mise.backup.service`. A value with a unit-type
suffix, such as `unit = "nginx.service"`, is written as is.

A timer must set at least one of `on_boot_sec`, `on_unit_active_sec`,
`on_unit_inactive_sec`, or `on_calendar`. Service keys such as `exec_start`,
`environment`, and `restart` are rejected on a timer entry.

## Set the environment

```toml
[bootstrap.linux.systemd.units.my-sync]
exec_start = "~/.local/bin/my-sync --watch"
environment = { PATH = "/usr/local/bin:/usr/bin:/bin", LOG_LEVEL = "info" }
environment_file = ["-%h/.config/my-sync.env"]
working_directory = "~"
nice = 10
umask = "0007"
standard_output = "journal"
standard_error = "journal"
```

`environment_file` takes a list of absolute paths or paths that use systemd
specifiers such as `%h`. A leading `-` makes a file optional. systemd does not
expand `~` or `$HOME` in these paths.

Do not put secret values in `environment`, which writes them into the unit
file. Put them in an `environment_file` with mode `0600`, for example one
rendered by a [`[bootstrap.files]`](/bootstrap/files.html) template that uses a
[secret input](/bootstrap/secrets.html). The service still receives them as
environment variables; this section has no key for systemd credentials
(`LoadCredential=`).

## Use templates {#templates}

String values are [Tera templates](/templates.html), rendered with the
declaring config's context. That makes a unit relocatable with the project that
defines it:

```toml
[bootstrap.linux.systemd.units.my-service]
description = "my service"
exec_start = "{{ config_root }}/bin/serve"
working_directory = "{{ config_root }}"
environment_file = ["{{ config_root }}/.env"]
```

Templates are rendered against the declaring config, not the current
directory, so <code v-pre>{{ config_root }}</code> does not change with where
you run `mise bootstrap`. It is that config's root: the project directory for a
project config, including `.mise/config.toml`, and `MISE_GLOBAL_CONFIG_ROOT`
(default `$HOME`) for the global config. With the config above in
`~/src/my-project/mise.toml`, the unit gets
`WorkingDirectory=/home/you/src/my-project` and
`EnvironmentFile=/home/you/src/my-project/.env`. This matters most for
`environment_file`, where `%h` is the only other way to write a path in your
home directory.

Values without template syntax are written unchanged, so systemd specifiers
such as `%h` and `%i` reach the unit file, and `~` expansion still applies
after rendering. `exec()` is not available; see
[templates in bootstrap](/bootstrap.html#templates).

## How configs combine

Units merge by name across the
[config hierarchy](/configuration.html#configuration-hierarchy). A more local
config replaces the whole declaration for that name; mise does not merge keys
across files. A unit that fails validation or whose template fails to render is
reported with a warning and skipped, and the other units still apply.

If an entry changes between a service and a timer, apply stops, disables, and
deletes the old unit. mise manages only unit files in `~/.config/systemd/user`
whose names start with `dev.mise.`.

## Remove a unit

Set `state = "absent"` to stop, disable, and delete a unit, and keep it removed
while the entry says so:

```toml
[bootstrap.linux.systemd.units.backup]
state = "absent"
```

An absent entry removes both `dev.mise.<name>.timer` and
`dev.mise.<name>.service`, so you do not have to say which kind it was, and its
other keys can stay in place. Apply stops and disables the timer before the
service, deletes the unit files, and runs `systemctl --user daemon-reload`.

Deleting a declaration instead leaves the unit installed. To remove a service
unit once, run
[`mise bootstrap services remove`](/cli/bootstrap/services/remove.html):

```sh
mise bootstrap services remove my-sync
```

If the entry is still declared, the next `mise bootstrap` installs it again.
The command removes only `.service` units.
[`mise bootstrap unapply`](/bootstrap/modules.html#remove-a-module-s-resources)
removes the services and timers that a machine module declared.

## Preview and apply

Check each unit with
[`mise bootstrap linux systemd-units status`](/cli/bootstrap/linux/systemd-units/status.html),
and preview the commands before you apply them:

```sh
mise bootstrap linux systemd-units status            # state of each unit
mise bootstrap linux systemd-units status --json     # the same, as JSON
mise bootstrap linux systemd-units status --missing  # exit 1 if any unit is missing, changed, or not in its desired started/stopped state
mise bootstrap linux systemd-units apply --dry-run   # print the commands
mise bootstrap linux systemd-units apply             # apply after a confirmation prompt
mise bootstrap linux systemd-units apply --yes       # apply without prompting
```

`systemd` is a shorter alias for `systemd-units`.

Status reports each unit as `active`, `inactive`, `differs` (the unit file or
its enablement does not match the declaration), or `missing`. An absent entry
is `present` while any of its unit files exist and `absent` once they are gone. Apply changes
every unit that is not in its desired state: it writes the unit file, runs
`systemctl --user daemon-reload`, enables units that have `wanted_by` and
disables units with `wanted_by = []`, then restarts each unit with
`start = true` or stops it with `start = false`.

## Reference

`[Unit]` keys, for services and timers:

| Key           | systemd key   | Notes                                                          |
| ------------- | ------------- | -------------------------------------------------------------- |
| `description` | `Description` |                                                                |
| `after`       | `After`       | List of units                                                  |
| `before`      | `Before`      | List of units                                                  |
| `wants`       | `Wants`       | List of units                                                  |
| `requires`    | `Requires`    | List of units; does not order them, so add them to `after` too |
| `binds_to`    | `BindsTo`     | List of units                                                  |
| `part_of`     | `PartOf`      | List of units                                                  |
| `conflicts`   | `Conflicts`   | List of units                                                  |

`[Service]` keys:

| Key                 | systemd key        | Notes                                        |
| ------------------- | ------------------ | -------------------------------------------- |
| `exec_start`        | `ExecStart`        | Required for a service; `~` expanded         |
| `exec_start_pre`    | `ExecStartPre`     | List; one line per entry, in order           |
| `exec_start_post`   | `ExecStartPost`    | List; one line per entry, in order           |
| `exec_stop`         | `ExecStop`         |                                              |
| `exec_stop_post`    | `ExecStopPost`     | List; one line per entry, in order           |
| `type`              | `Type`             | Such as `"oneshot"`                          |
| `remain_after_exit` | `RemainAfterExit`  |                                              |
| `timeout_start_sec` | `TimeoutStartSec`  |                                              |
| `timeout_stop_sec`  | `TimeoutStopSec`   |                                              |
| `restart`           | `Restart`          | Such as `"on-failure"`                       |
| `restart_sec`       | `RestartSec`       |                                              |
| `environment`       | `Environment`      | Table of variables                           |
| `environment_file`  | `EnvironmentFile`  | List of paths; `-` prefix makes one optional |
| `working_directory` | `WorkingDirectory` | `~` expanded                                 |
| `nice`              | `Nice`             | -20 to 19                                    |
| `umask`             | `UMask`            | Octal, `"0000"` to `"0777"`                  |
| `no_new_privileges` | `NoNewPrivileges`  |                                              |
| `private_tmp`       | `PrivateTmp`       |                                              |
| `standard_output`   | `StandardOutput`   | Such as `"journal"`                          |
| `standard_error`    | `StandardError`    |                                              |

In the `exec_*` keys, a leading `~` or `~/` is expanded to your home directory,
and systemd's command prefixes stay in front of the path, so
`exec_start_pre = ["-~/bin/check"]` runs an optional check from your home
directory.

`[Timer]` keys; setting any of them makes the entry a timer:

| Key                    | systemd key          | Notes                                       |
| ---------------------- | -------------------- | ------------------------------------------- |
| `on_boot_sec`          | `OnBootSec`          |                                             |
| `on_unit_active_sec`   | `OnUnitActiveSec`    |                                             |
| `on_unit_inactive_sec` | `OnUnitInactiveSec`  |                                             |
| `on_calendar`          | `OnCalendar`         | Such as `"daily"` or `"Mon *-*-* 09:00"`    |
| `randomized_delay_sec` | `RandomizedDelaySec` |                                             |
| `accuracy_sec`         | `AccuracySec`        |                                             |
| `persistent`           | `Persistent`         | Catch up missed `on_calendar` runs          |
| `unit`                 | `Unit`               | A bare name means `dev.mise.<name>.service` |

`[Install]` and mise keys:

| Key         | Effect                                                                                                                             |
| ----------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `wanted_by` | `WantedBy`; defaults to `["default.target"]` for services and `["timers.target"]` for timers. `[]` writes the unit and disables it |
| `start`     | Defaults to `true`, which restarts the unit after apply; `false` stops it                                                          |
| `state`     | `"present"` (the default) or `"absent"`, which removes the unit; see [Remove a unit](#remove-a-unit)                               |

Unit names may contain letters, numbers, `.`, `_`, `-`, and `@`.

## On macOS and Windows

The section is ignored on other platforms, so one config can serve several
machines. `status` lists each unit as skipped, `apply` does nothing, and the
full `mise bootstrap` notes the skipped units in its follow-up summary.

## Troubleshooting

| Problem                                                     | What to do                                                                                                               |
| ----------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| Units are skipped with "cannot target SUDO_USER"            | Run mise as the user who owns the units, not with `sudo mise`                                                            |
| Units are skipped with "systemd user manager not available" | Log in to a session that starts a user manager; containers often have none                                               |
| Units stop when you log out of a server                     | Enable lingering once with `sudo loginctl enable-linger $USER`; mise does not enable it for you                          |
| A unit fails to start                                       | Read its output with `journalctl --user -u dev.mise.<name>` and its state with `systemctl --user status dev.mise.<name>` |

## See also

- [Bootstrap](/bootstrap.html#how-it-runs) for where units fall in the run
  order.
- [Services](/bootstrap/services.html) for background programs declared once
  for every platform, and for system units.
