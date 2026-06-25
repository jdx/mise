# systemd <Badge type="warning" text="experimental" />

mise can declare Linux systemd user services in
`[bootstrap.linux.systemd.units]` and apply them with
`mise bootstrap linux systemd-units apply`:

```toml
[bootstrap.linux.systemd.units.my-sync]
description = "sync files"
exec_start = "~/.local/bin/my-sync --watch"
after = ["network-online.target"]
wants = ["network-online.target"]
environment = { PATH = "/usr/local/bin:/usr/bin:/bin" }
working_directory = "~"
restart = "on-failure"
restart_sec = "5s"
standard_output = "append:%h/.local/state/my-sync.log"
standard_error = "journal"
```

Each unit is written to `~/.config/systemd/user/dev.mise.<name>.service` and
managed with `systemctl --user`. Unit names may contain letters, numbers, `.`,
`_`, `-`, and `@`. mise owns only the service files it creates with the
`dev.mise.` prefix.

## Supported keys

| TOML key            | systemd key                    |
| ------------------- | ------------------------------ |
| `description`       | `Description`                  |
| `after`             | `After`                        |
| `wants`             | `Wants`                        |
| `exec_start`        | `ExecStart`                    |
| `environment`       | `Environment`                  |
| `working_directory` | `WorkingDirectory`             |
| `restart`           | `Restart`                      |
| `restart_sec`       | `RestartSec`                   |
| `standard_output`   | `StandardOutput`               |
| `standard_error`    | `StandardError`                |
| `wanted_by`         | `WantedBy`                     |
| `start`             | run `systemctl --user restart` |

`exec_start` and `working_directory` expand bare `~` and `~/` to the current
user's home directory before writing the service file. `wanted_by` defaults to
`["default.target"]`; set `wanted_by = []` to write the unit and disable any
previous enablement. `start` defaults to `true`; set `start = false` to write
and enable without keeping the unit running.

## Semantics

- **Declarative and additive** — unit names merge across the
  [config hierarchy](/configuration.html) (global → project). A more local
  config replaces the full declaration for the same unit name.
- **Linux-only** — on other platforms the section is inert:
  `mise bootstrap linux systemd-units status` lists entries as skipped and
  `mise bootstrap linux systemd-units apply` ignores them.
- **User services only** — mise writes to `~/.config/systemd/user` and uses
  `systemctl --user`. System services in `/etc/systemd/system` are not
  supported.
- **Target user only** — run mise as the user who owns the services, with a
  reachable systemd user manager. `sudo mise` is skipped because `systemctl --user`
  would target the wrong user manager.
- **Manual application only** — mise never writes or starts systemd services
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
`apply` rewrites changed service files, runs `systemctl --user daemon-reload`,
enables units with `wanted_by`, disables units with `wanted_by = []`, and
restarts them when `start = true` or stops them when `start = false`.
