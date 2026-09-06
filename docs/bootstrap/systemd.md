# Linux systemd user units

mise can declare Linux systemd user services and timers in
`[bootstrap.linux.systemd.units]` and apply them with
`mise bootstrap linux systemd-units apply` or as part of
[`mise bootstrap`](/bootstrap.html):

Use [system services](/bootstrap/services.html) for units managed by the system
manager. These user units need a reachable user manager and run with the user's
permissions. Install your executable before applying the example; `my-sync`
is a placeholder for a program you provide.

```toml
[bootstrap.linux.systemd.units.my-sync]
description = "sync files"
exec_start = "~/.local/bin/my-sync --watch"
after = ["network-online.target"]
wants = ["network-online.target"]
environment = { PATH = "/usr/local/bin:/usr/bin:/bin" }
environment_file = ["-%h/.config/my-sync.env"]
nice = 10
umask = "0007"
working_directory = "~"
restart = "on-failure"
restart_sec = "5s"
standard_output = "journal"
standard_error = "journal"
```

Oneshot and hardened services can use additional service directives:

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

An entry containing a timer key is rendered as a `.timer` instead of a
`.service`. For example:

```toml
[bootstrap.linux.systemd.units.healthcheck]
description = "check daemon health"
type = "oneshot"
exec_start = "~/.local/bin/daemon healthcheck"
start = false
wanted_by = []

[bootstrap.linux.systemd.units.healthcheck-timer]
description = "periodically check daemon health"
on_boot_sec = "2min"
on_unit_inactive_sec = "5min"
randomized_delay_sec = "30s"
unit = "healthcheck"
```

The service is left disabled and stopped during apply so the timer controls its
execution. `persistent` catches up missed calendar events when used with
`on_calendar`; it does not add catch-up behavior to monotonic timers such as
`on_unit_inactive_sec`. See the
[systemd timer reference](https://www.freedesktop.org/software/systemd/man/latest/systemd.timer.html#Persistent=).

A bare `unit` value (no unit-type suffix) is resolved to the mise-owned service
`dev.mise.<unit>.service` — so `unit = "healthcheck"` targets the `healthcheck`
service entry above. To point a timer at an unmanaged unit, give the fully
qualified name (e.g. `unit = "nginx.service"`), which is written verbatim.

A timer must set at least one of `on_boot_sec`, `on_unit_active_sec`,
`on_unit_inactive_sec`, or `on_calendar`. Service-only keys such as
`exec_start`, `environment`, and `restart` are rejected on timer entries; use a
separate service entry for the unit triggered by the timer.

Each unit is written to `~/.config/systemd/user/dev.mise.<name>.service` or
`~/.config/systemd/user/dev.mise.<name>.timer` and managed with
`systemctl --user`. Unit names may contain letters, numbers, `.`, `_`, `-`, and
`@`. mise owns only the unit files it creates with the `dev.mise.` prefix.

## Supported keys

| TOML key               | systemd key                    |
| ---------------------- | ------------------------------ |
| `description`          | `Description`                  |
| `after`                | `After`                        |
| `wants`                | `Wants`                        |
| `requires`             | `Requires`                     |
| `exec_start`           | `ExecStart`                    |
| `type`                 | `Type`                         |
| `remain_after_exit`    | `RemainAfterExit`              |
| `exec_stop`            | `ExecStop`                     |
| `timeout_start_sec`    | `TimeoutStartSec`              |
| `timeout_stop_sec`     | `TimeoutStopSec`               |
| `no_new_privileges`    | `NoNewPrivileges`              |
| `private_tmp`          | `PrivateTmp`                   |
| `environment`          | `Environment`                  |
| `environment_file`     | `EnvironmentFile`              |
| `nice`                 | `Nice`                         |
| `umask`                | `UMask`                        |
| `working_directory`    | `WorkingDirectory`             |
| `restart`              | `Restart`                      |
| `restart_sec`          | `RestartSec`                   |
| `standard_output`      | `StandardOutput`               |
| `standard_error`       | `StandardError`                |
| `on_boot_sec`          | `OnBootSec`                    |
| `on_unit_active_sec`   | `OnUnitActiveSec`              |
| `on_unit_inactive_sec` | `OnUnitInactiveSec`            |
| `on_calendar`          | `OnCalendar`                   |
| `randomized_delay_sec` | `RandomizedDelaySec`           |
| `accuracy_sec`         | `AccuracySec`                  |
| `persistent`           | `Persistent`                   |
| `unit`                 | `Unit`                         |
| `wanted_by`            | `WantedBy`                     |
| `start`                | run `systemctl --user restart` |

`requires` does not imply ordering; add the same unit to `after` when it must
start first. `environment_file` accepts a list of absolute paths or paths using
systemd specifiers such as `%h`; prefix a path with `-` to make it optional.
systemd does not expand `~` or `$HOME` in these paths. Environment variables are
not appropriate for secrets; use systemd credentials for sensitive values.

Unit commands do not inherit your interactive shell's mise activation. Set an
explicit executable path and the environment the service needs. `ExecStart`
uses systemd's command syntax; shell operators need an explicitly invoked shell
or a wrapper script.

`exec_start`, `exec_stop`, and `working_directory` expand bare `~` and `~/` to the current
user's home directory before writing the service file. `wanted_by` defaults to
`["default.target"]` for services and `["timers.target"]` for timers; set
`wanted_by = []` to write the unit and disable any previous enablement. `start`
defaults to `true`; set `start = false` to write and enable without keeping the
unit running.

## Semantics

- **Declarative and additive** — unit names merge across the
  [config hierarchy](/configuration.html) (global → project). A more local
  config replaces the full declaration for the same unit name. When an entry
  changes between a service and a timer, mise stops, disables, and removes the
  stale sibling unit.
- **Linux-only** — on other platforms the section is inert:
  `mise bootstrap linux systemd-units status` lists entries as skipped and
  `mise bootstrap linux systemd-units apply` ignores them.
- **User units only** — mise writes to `~/.config/systemd/user` and uses
  `systemctl --user`. To manage system services in `/etc/systemd/system`, use
  [managed files](/bootstrap/files.html) and [system services](/bootstrap/services.html).
- **Target user only** — run mise as the user who owns the services, with a
  reachable systemd user manager. `sudo mise` is skipped because `systemctl --user`
  would target the wrong user manager.
- **Manual application only** — mise never writes or starts systemd units
  implicitly; only `mise bootstrap linux systemd-units apply` and `mise bootstrap` do.

## Commands

```sh
mise bootstrap linux systemd-units status            # shows systemd user service state
mise bootstrap linux systemd-units status --json     # machine-readable
mise bootstrap linux systemd-units status --missing  # exit 1 if any unit is missing, changed, or inactive

mise bootstrap linux systemd-units apply           # write and start missing/changed units
mise bootstrap linux systemd-units apply --dry-run # print the commands without running them
mise bootstrap linux systemd-units apply --yes     # skip the confirmation prompt
```

`status` reports each unit as `active`, `inactive`, `differs`, or `missing`.
`apply` rewrites changed unit files, runs `systemctl --user daemon-reload`,
enables units with `wanted_by`, disables units with `wanted_by = []`, and
restarts them when `start = true` or stops them when `start = false`.
