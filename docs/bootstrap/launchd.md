# launchd <Badge type="warning" text="experimental" />

mise can declare macOS user LaunchAgents in
`[bootstrap.macos.launchd.agents]` and apply them with
`mise bootstrap macos launchd-agents apply`:

```toml
[bootstrap.macos.launchd.agents.my-sync]
program = "~/.local/bin/my-sync"
args = ["--watch"]
run_at_load = true
start_interval = 300
start_calendar_interval = { hour = 2, minute = 0 }
environment = { PATH = "/opt/homebrew/bin:/usr/bin:/bin" }
working_directory = "~"
stdout_path = "~/Library/Logs/my-sync.log"
stderr_path = "~/Library/Logs/my-sync.err.log"
```

Each agent is written to `~/Library/LaunchAgents/dev.mise.<name>.plist` and
loaded with `launchctl bootstrap gui/$UID
~/Library/LaunchAgents/dev.mise.<name>.plist`. Agent names may contain letters,
numbers, `.`, `_`, and `-`. mise owns only the plist files it creates with the
`dev.mise.` label prefix.

## Supported keys

| TOML key                  | launchd key               |
| ------------------------- | ------------------------- |
| `program`                 | `ProgramArguments[0]`     |
| `args`                    | `ProgramArguments[1..]`   |
| `run_at_load`             | `RunAtLoad`               |
| `keep_alive`              | `KeepAlive`               |
| `start_interval`          | `StartInterval`           |
| `start_calendar_interval` | `StartCalendarInterval`   |
| `environment`             | `EnvironmentVariables`    |
| `working_directory`       | `WorkingDirectory`        |
| `stdout_path`             | `StandardOutPath`         |
| `stderr_path`             | `StandardErrorPath`       |
| `kickstart`               | run `launchctl kickstart` |

`program`, `working_directory`, `stdout_path`, and `stderr_path` expand bare
`~` and `~/` to the current user's home directory before writing the plist.
`args` are passed through exactly as written.
`start_calendar_interval` accepts `minute` (0-59), `hour` (0-23), `day`
(1-31), `weekday` (0-7), and `month` (1-12), and writes the corresponding
launchd calendar keys.

## Semantics

- **Declarative and additive** — agent names merge across the
  [config hierarchy](/configuration.html) (global → project). A more local
  config replaces the full declaration for the same agent name.
- **macOS-only** — on other platforms the section is inert:
  `mise bootstrap macos launchd-agents status` lists entries as skipped and
  `mise bootstrap macos launchd-agents apply` ignores them.
- **Manual application only** — mise never writes or loads LaunchAgents
  implicitly; only `mise bootstrap macos launchd-agents apply` and `mise bootstrap` do.
- **User agents only** — mise writes to `~/Library/LaunchAgents`. System
  daemons in `/Library/LaunchDaemons` are not supported.

## Commands

```sh
mise bootstrap macos launchd-agents status            # shows LaunchAgent state
mise bootstrap macos launchd-agents status --json     # machine-readable
mise bootstrap macos launchd-agents status --missing  # exit 1 if any agent is missing, changed, or unloaded

mise bootstrap macos launchd-agents apply           # write and load missing/changed agents
mise bootstrap macos launchd-agents apply --dry-run # print the commands without running them
mise bootstrap macos launchd-agents apply --yes     # skip the confirmation prompt
```

`status` reports each agent as `loaded`, `unloaded`, `differs`, or `missing`.
`apply` rewrites changed plists, unloads the old job if present, loads the new
job, enables it, and runs `kickstart` only when `kickstart = true`.
