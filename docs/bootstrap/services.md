# System services

`[bootstrap.services]` declaratively manages the lifecycle of existing Linux
systemd system units. Package installation and `[bootstrap.files]` run first,
so a service may be installed by a package or supplied as a managed unit file.
After file changes, mise reloads systemd before applying service changes.

```toml
[bootstrap.packages]
"apt:docker.io" = "latest"

[bootstrap.services.docker]
state = "running"
enabled = true
```

Names without a unit suffix receive `.service`. Explicit unit names such as
`postgresql@16-main.service`, sockets, and timers are also accepted.

## Options

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
cannot leave a host partially provisioned. Mise runs one `daemon-reload`,
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

A masked unit must also be stopped and disabled:

```toml
[bootstrap.services.old-worker]
state = "stopped"
enabled = false
masked = true
```

Mise does not guess when a unit is missing, systemd is unavailable, or a unit
cannot be enabled (for example, a static unit). Status and plans report the
resource as `unknown`; apply fails closed instead of running an unsafe command.

```sh
mise bootstrap services status
mise bootstrap services status --json
mise bootstrap services apply --dry-run
mise bootstrap services apply --yes
```

System service management is Linux-only and requires root privileges. Mise
prompts through sudo only when a change is required.
