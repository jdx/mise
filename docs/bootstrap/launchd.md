---
description: "Declare macOS LaunchAgents in mise.toml, then write and load them into your login session with launchctl."
socialDescription: "Declare macOS LaunchAgents in mise.toml and load them with launchctl."
---

# macOS LaunchAgents

Declare LaunchAgents for your macOS user in `[bootstrap.macos.launchd.agents]`.
[`mise bootstrap`](/bootstrap.html), or
[`mise bootstrap macos launchd-agents apply`](/cli/bootstrap/macos/launchd-agents/apply.html),
writes each agent to `~/Library/LaunchAgents/dev.mise.<name>.plist` and loads
it into your login session.

To run one program in the background on macOS, Linux, and Windows from a
single declaration, use a [user service](/bootstrap/services.html#user-services)
instead. Use a LaunchAgent when you need launchd features such as calendar
schedules, queue directories, or a process type. A name cannot be declared in
both places, because both write the same plist.

## Example

Install the program and create any log directories first. `my-sync` stands in
for your own program:

```toml
[bootstrap.macos.launchd.agents.my-sync]
program = "~/.local/bin/my-sync"
args = ["--watch"]
run_at_load = true
keep_alive_on_failure = true
environment = { PATH = "/opt/homebrew/bin:/usr/bin:/bin" }
working_directory = "~"
stdout_path = "~/Library/Logs/my-sync.log"
stderr_path = "~/Library/Logs/my-sync.err.log"
```

Run apply as the user who owns the agent, while that user is logged in to the
Mac's desktop: agents load into the `gui/<uid>` launchd domain, which exists
only for a logged-in user.

```sh
mise bootstrap macos launchd-agents apply --dry-run
mise bootstrap macos launchd-agents apply
```

mise writes `~/Library/LaunchAgents/dev.mise.my-sync.plist` and loads it with
`launchctl bootstrap gui/<uid>`. The agent starts now and at each login, and
launchd restarts it after it fails.

The agent gets launchd's environment, not your shell's, so mise activation and
tools from `[tools]` are not on its `PATH`. Use absolute paths and declare the
environment variables it needs. `program` and `args` form the argument list
directly, without a shell: for pipes or redirection, run `/bin/sh -c '...'` or
a wrapper script.

## Run on a schedule

`start_interval` starts the agent every so many seconds.
`start_calendar_interval` starts it at calendar times, using any of `minute`
(0-59), `hour` (0-23), `day` (1-31), `weekday` (0-7, where 0 and 7 are
Sunday), and `month` (1-12). A field you leave out matches every value. For
several schedules, use an array:

```toml
[bootstrap.macos.launchd.agents.daily-sync]
program = "~/.local/bin/my-sync"
start_calendar_interval = [{ hour = 3, minute = 0 }, { hour = 12, minute = 0, weekday = 1 }]
```

This runs at 03:00 every day and at 12:00 on Mondays. If you set both
`start_interval` and `start_calendar_interval`, either one can start the agent.

`queue_directories` starts the agent whenever one of the listed directories is
not empty, and launchd expects the agent to empty them. Each entry must be an
absolute path, `~`, or a path starting with `~/`.

## Keep a program running

- `run_at_load = true` starts the agent when it is loaded, at apply and at
  login.
- `keep_alive = true` restarts the program after any exit.
- `keep_alive_on_failure = true` restarts it only after it exits with an error.
  Set at most one of `keep_alive` and `keep_alive_on_failure`.
- `throttle_interval` is the minimum number of seconds between launches
  (launchd's default is 10).
- `kickstart = true` runs `launchctl kickstart` after loading, which starts the
  program if it is not already running.

## Set the agent's priority

`process_type` sets launchd's scheduling band: `Background`, `Standard`,
`Adaptive`, or `Interactive`. Use `Background` for work that should yield to
you; launchd limits its CPU and disk I/O. Values are case-sensitive. mise warns
about any other value and skips the agent, because launchd silently ignores a
`ProcessType` it does not recognize and the job would run in the default band.

`nice` sets the process's niceness, from -20 to 20. Higher values run at lower
priority.

```toml
[bootstrap.macos.launchd.agents.indexer]
program = "~/.local/bin/indexer"
start_interval = 3600
process_type = "Background"
nice = 10
```

## Use templates {#templates}

String values are [Tera templates](/templates.html), rendered with the
declaring config's context. That includes entries in `args`, `environment`, and
`queue_directories`:

```toml
[bootstrap.macos.launchd.agents.my-sync]
program = "{{ config_root }}/bin/sync"
working_directory = "{{ config_root }}"
stdout_path = "{{ config_root }}/log/sync.log"
```

Here <code v-pre>{{ config_root }}</code> is the declaring config's root: the
project directory for a project config, or `MISE_GLOBAL_CONFIG_ROOT` (default
`$HOME`) for the global config. It does not change with the directory you run
mise from. A value without template syntax is written unchanged, and `~`
expansion still applies after rendering. `exec()` is not available; see
[templates in bootstrap](/bootstrap.html#templates).

## How configs combine

Agents merge by name across the
[config hierarchy](/configuration.html#configuration-hierarchy). A more local
config replaces the whole declaration for that name; mise does not merge keys
across files. An agent that fails validation or whose template fails to render
is reported with a warning and skipped, and the other agents still apply.

mise manages only plists in `~/Library/LaunchAgents` whose label starts with
`dev.mise.`. It does not touch other agents, and system daemons in
`/Library/LaunchDaemons` are not supported.

## Remove an agent

Deleting a declaration leaves the agent installed and loaded. To unload it and
delete its plist, run
[`mise bootstrap services remove`](/cli/bootstrap/services/remove.html):

```sh
mise bootstrap services remove my-sync
```

The command works whether or not the agent is still declared; if it is, the
next `mise bootstrap` installs it again. Without mise,
run `launchctl bootout gui/$(id -u)/dev.mise.my-sync` and delete
`~/Library/LaunchAgents/dev.mise.my-sync.plist`.

## Preview and apply

Check each agent with
[`mise bootstrap macos launchd-agents status`](/cli/bootstrap/macos/launchd-agents/status.html),
and preview the commands before you apply them:

```sh
mise bootstrap macos launchd-agents status            # state of each agent
mise bootstrap macos launchd-agents status --json     # the same, as JSON
mise bootstrap macos launchd-agents status --missing  # exit 1 if any agent is not loaded
mise bootstrap macos launchd-agents apply --dry-run   # print the commands
mise bootstrap macos launchd-agents apply             # apply after a confirmation prompt
mise bootstrap macos launchd-agents apply --yes       # apply without prompting
```

`launchd` is a shorter alias for `launchd-agents`.

Status reports each agent as `loaded`, `unloaded`, `differs` (the plist on disk
does not match the declaration), or `missing`. Apply changes every agent that
is not `loaded`: it writes the plist, unloads the old job if one is loaded,
loads the new one with `launchctl bootstrap`, enables it with
`launchctl enable`, and runs `launchctl kickstart` when `kickstart = true`.
Unloading stops the agent's running process.

## Reference

| Key                       | plist key                                | Notes                                                                     |
| ------------------------- | ---------------------------------------- | ------------------------------------------------------------------------- |
| `program`                 | `ProgramArguments[0]`                    | Required; `~` expanded                                                    |
| `args`                    | `ProgramArguments[1..]`                  | Passed as written                                                         |
| `run_at_load`             | `RunAtLoad`                              | Start when loaded                                                         |
| `keep_alive`              | `KeepAlive`                              | Restart after any exit                                                    |
| `keep_alive_on_failure`   | `KeepAlive = { SuccessfulExit = false }` | Restart after a failure; not with `keep_alive`                            |
| `start_interval`          | `StartInterval`                          | Seconds                                                                   |
| `start_calendar_interval` | `StartCalendarInterval`                  | Table or array of tables with `minute`, `hour`, `day`, `weekday`, `month` |
| `queue_directories`       | `QueueDirectories`                       | Absolute paths or `~/` paths                                              |
| `throttle_interval`       | `ThrottleInterval`                       | Minimum seconds between launches                                          |
| `process_type`            | `ProcessType`                            | `Background`, `Standard`, `Adaptive`, or `Interactive`                    |
| `nice`                    | `Nice`                                   | -20 to 20                                                                 |
| `environment`             | `EnvironmentVariables`                   | Table of variables                                                        |
| `working_directory`       | `WorkingDirectory`                       | `~` expanded                                                              |
| `stdout_path`             | `StandardOutPath`                        | `~` expanded                                                              |
| `stderr_path`             | `StandardErrorPath`                      | `~` expanded                                                              |
| `kickstart`               | Not written to the plist                 | Run `launchctl kickstart` after loading                                   |

Agent names may contain letters, numbers, `.`, `_`, and `-`. Each agent's label
is `dev.mise.<name>`.

## On Linux and Windows

The section is ignored on other platforms, so one config can serve several
machines. `status` lists each agent as skipped, `apply` does nothing, and the
full `mise bootstrap` notes the skipped agents in its follow-up summary.

## Troubleshooting

To see why an agent is loaded but not running, inspect it with launchd and read
its output files:

```sh
launchctl print gui/$(id -u)/dev.mise.my-sync
tail ~/Library/Logs/my-sync.err.log
```

`launchctl print` shows the job's state, its last exit status, and the program
and arguments launchd runs. A program that works in your terminal but fails
here usually needs an absolute path or a variable in `environment`.

## See also

- [Bootstrap](/bootstrap.html#how-it-runs) for where agents fall in the run
  order.
- [Services](/bootstrap/services.html) for background programs declared once
  for every platform.
- [macOS defaults](/bootstrap/macos-defaults.html) for user preferences.
