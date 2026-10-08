---
description: Run databases, message brokers, and development servers for a project with mise and pitchfork.
---

# Daemons <Badge type="warning" text="experimental" />

A daemon is a process that keeps running between commands, such as a database,
a message broker, or a development server. Declare daemons in `mise.toml`: mise
installs their tools and gives them the project's environment, and
[pitchfork](https://pitchfork.jdx.dev/) supervises the processes and their
readiness checks.

::: warning Experimental
Daemon support can change in any release. Enable it with `experimental = true`
under `[settings]`.
:::

## Requirements

- `experimental = true` under `[settings]`, or `MISE_EXPERIMENTAL=1`. If you
  use [start and stop with your shell](#automatic-start-and-stop), set it in
  your global config.
- pitchfork. `mise daemons start`, `restart`, and `register`, and `mise run`
  for a task with `daemons`, install pitchfork when it is missing: the version
  `[tools]` requests, otherwise the latest. The shell hook never installs
  tools, so for automatic start and stop install it first, for example with
  `mise use -g pitchfork`.
- A project config file. `mise daemons` commands run inside a project.
- [Safe mode](/security.html#safe-mode) off. Safe mode ignores project daemon
  declarations, and `mise daemons` refuses to run in it.

mise requires pitchfork 2.25.0 or later. Some features need a newer release:

| Feature                                                                                                       | Minimum pitchfork |
| ------------------------------------------------------------------------------------------------------------- | ----------------- |
| Daemons, service presets, and tasks with `daemons`                                                            | 2.25.0            |
| [Stable URLs](/daemons/worktrees.html#stable-urls-per-worktree) (`proxy`, `proxy_tls`)                        | 2.26.0            |
| [`proxy_idle_timeout`](/daemons/worktrees.html#stop-idle-daemons)                                             | 2.27.0            |
| [Daemons that run a task](#daemons-that-run-a-task)                                                           | 2.28.0            |
| Clean PostgreSQL shutdown on [Windows](/daemons/presets.html#windows)                                         | 2.29.0            |
| <code v-pre>{{ env.NAME }}</code> and <code v-pre>{{ vars.NAME }}</code> in [`run`](#use-env-and-vars-in-run) | 2.30.0            |

## Quick start

This example gives a Node.js project a persistent PostgreSQL database. Add it
to the project's `mise.toml`:

```toml
[settings]
experimental = true

[tools]
node = "24"

[daemons]
postgres = "18"

[tasks.dev]
daemons = "postgres"
run = "npm run dev"
```

Run `mise run dev`. mise installs the missing tools, starts PostgreSQL, waits
until it is ready, and then runs your application's `dev` script. The preset
exports connection variables, including `DATABASE_URL`, and keeps the
database's data between runs.

PostgreSQL keeps running when the task exits, and later runs reuse it. Run
`mise daemons stop postgres` when you no longer need it, or
[start and stop it with your shell](#automatic-start-and-stop).

## Declare a daemon

Each entry under `[daemons]` declares one daemon. Choose the form by what you
want to run:

| Declaration                                | Runs                                                                                            |
| ------------------------------------------ | ----------------------------------------------------------------------------------------------- |
| `postgres = "18"` under `[daemons]`        | The [service preset](/daemons/presets.html) named like the entry, at that version               |
| `preset = "postgres"` and `version = "18"` | A named instance of a preset, with optional overrides                                           |
| `run = "exec npm run dev"`                 | A shell command                                                                                 |
| `task = "dev:core"`                        | A [mise task](#daemons-that-run-a-task), with optional `args`                                   |
| `project = "../services"`                  | A daemon declared in [another project](/daemons/sharing.html#use-a-daemon-from-another-project) |
| `provider = "local-postgres"`              | A database on a [shared server](/daemons/sharing.html#shared-server-providers)                  |

For example, declare a development server and a second PostgreSQL instance:

```toml
[daemons.api]
run = "exec npm run dev"
ready_port = 3000

[daemons.analytics]
preset = "postgres"
version = "18"
port = 5433
```

A `run` command runs in the project's tool environment, so declare the tools
it needs in `[tools]`; a preset installs its own tool. Start the long-running
command with `exec` so it receives stop signals directly.

Set a readiness check that passes when the service can accept work, such as
`ready_port` or `ready_cmd`; the example above waits for port 3000. Tasks and
dependent daemons wait for it. Set `port` to a number for a fixed port. To
[give each worktree its own](/daemons/worktrees.html#automatic-ports), use
`port = { auto = true, base = 3000 }` on a custom daemon; `port = "auto"` alone
works only on a preset, which supplies the base.

Daemon names can contain ASCII letters, numbers, `.`, `_`, and `-`. They cannot
start or end with `-`, or contain `..` or `--`.

### Override a daemon

A daemon declared in a higher-precedence file, such as `mise.local.toml`,
replaces the whole declaration, so repeat every key you need. A daemon declared
in a parent directory's config also applies in child projects, and it keeps the
parent's project root and namespace.

### Use `[env]` and `[vars]` in `run`

::: v-pre
A `run` command can read `{{ env.NAME }}` from [`[env]`](/environments/) and
`{{ vars.NAME }}` from [`[vars]`](/configuration/vars.html), and use mise
[template](/templates.html) filters such as `quote`. pitchfork's own variables,
such as `{{ port }}` and `{{ url }}`, also work. The values are rendered when
the daemon starts, so `[env]` values are never written to the generated
pitchfork file. Only `run` is rendered this way.
:::

```toml
[env]
AUDIENCE = "world"

[vars]
greeting = "hello"

[daemons.hello]
run = "exec echo {{ vars.greeting | quote }} {{ env.AUDIENCE | quote }}"
```

## Daemon keys

| Key                    | Forms                | Meaning                                                                                                                                                                               |
| ---------------------- | -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `run`                  | custom, preset       | Shell command for the long-running process. On a preset, it replaces the preset's command.                                                                                            |
| `task`, `args`         | task                 | A mise task to run as the process, and the arguments passed to it. See [Run a task as a daemon](#daemons-that-run-a-task).                                                            |
| `preset`, `version`    | preset               | A [service preset](/daemons/presets.html) and the version request for its tool.                                                                                                       |
| `tool`                 | preset               | Installs the server from a different tool than the preset's default.                                                                                                                  |
| `options`              | preset               | [Preset options](/daemons/presets.html#preset-options), such as `options.database`.                                                                                                   |
| `port`                 | custom, task, preset | A fixed port, `"auto"`, or `{ auto = true, base = 3000 }`. `"auto"` alone works only on a preset; a custom or task daemon needs a `base`. See [Ports](/daemons/worktrees.html#ports). |
| `ports`                | preset               | Overrides for a preset's [named ports](/daemons/worktrees.html#named-ports).                                                                                                          |
| `init`                 | custom, task, preset | [Setup commands](#setup-before-the-process-starts) that run before the process starts.                                                                                                |
| `mise`                 | custom, task         | `false` runs the process and its setup without the project's tool environment.                                                                                                        |
| `data_dir`             | preset               | Where the preset keeps its data. See [Keep data in the checkout](/daemons/data.html#keep-data-in-the-checkout).                                                                       |
| `proxy`                | custom, task, preset | The daemon's hostname label, or `false` for no hostname. See [Per-daemon proxy settings](/daemons/worktrees.html#per-daemon-proxy-settings).                                          |
| `proxy_tls`            | custom, task, preset | `"terminate"` or `"passthrough"`: whether the proxy or the daemon handles TLS.                                                                                                        |
| `proxy_idle_timeout`   | custom, task, preset | How long a proxy-started daemon may sit idle before it stops. See [Stop idle daemons](/daemons/worktrees.html#stop-idle-daemons).                                                     |
| `project`, `name`      | reference            | Another project's directory, and the daemon's name there. See [Use a daemon from another project](/daemons/sharing.html#use-a-daemon-from-another-project).                           |
| `provider`, `resource` | provider             | A shared server, and the database or account to use on it. See [Shared server providers](/daemons/sharing.html#shared-server-providers).                                              |

Any other key, such as `ready_port`, `ready_cmd`, `depends`, `auto`, or
`boot_start`, is passed to pitchfork. mise adjusts two of them: it rewrites a
`depends` entry that names a [`project` reference](/daemons/sharing.html#use-a-daemon-from-another-project)
to the referenced daemon's full ID, and it runs `ready_cmd` and `health_cmd`
through `mise x`, so they see the project's tools and `[env]`, unless the
daemon sets `mise = false`. Either can also be an argument array, which
pitchfork 2.30.0 and later runs without a shell. See
pitchfork's
[configuration reference](https://pitchfork.jdx.dev/reference/configuration).
A `project` reference accepts only `project` and `name`, and a `provider`
reference accepts only `provider` and `resource`.

## Start daemons before a task {#tasks-that-require-daemons}

Add `daemons` to a task to start its services before any task body runs:

```toml
[daemons]
postgres = "18"
redis = "8"

[tasks.test]
daemons = ["postgres", "redis"]
run = "npm test"
```

`mise run test` starts the requested daemons and waits for pitchfork to report
them ready. Daemons that are already running are reused, and they keep running
after the task exits. This replaces prerequisite tasks that launch a background
process and poll it.

Use a string for one daemon, a list for several, or `true` for every daemon
the task's project declares. A task can also name a daemon
[from another project](/daemons/sharing.html#use-a-daemon-from-another-project) by
its local name or full ID; `true` does not include those. Daemon startup counts
as a dependency, so `--skip-deps` skips it.

Only the tasks resolved before the run starts, the tasks you name and their
`depends`, start their daemons. A task called from `run = [{ task = "..." }]`
does not. See the [`daemons` task option](/tasks/task-configuration.html#daemons)
for name resolution in monorepos, `--dry-run`, and safe mode, and run
`mise tasks info <task>` to see a task's daemons.

## Run a task as a daemon {#daemons-that-run-a-task}

Use `task` when the long-running command is already a mise task:

```toml
[tasks."dev:core"]
run = "cargo run --bin core --"

[daemons.core]
task = "dev:core"
args = ["--verbose"]
ready_port = 8080
```

Start it with `mise daemons start core`. `args` passes arguments to the task;
here Cargo forwards `--verbose` to the `core` program. Configure the readiness
check on `[daemons.core]`, not on the task.

`task` cannot be combined with `run` or `preset`, and `args` requires `task`.
The task must exist when the daemon is registered.

The daemon runs the task with `mise run`, so the task gets its tool environment
and its `depends` run first, as on the command line. mise starts the task
without a shell, so `args` reach it exactly as written on every platform. If
the daemon also has [`init`](#setup-before-the-process-starts), the setup
commands and the task share one shell.

The task's own `daemons` requirement is skipped, because starting it would
start this daemon again. Start the services the task needs with the daemon's
`depends`, or start them separately.

A daemon's task runs without mise [secrets](/tasks/task-configuration.html#secrets).
If that task, or a task it depends on, lists `secrets` or uses
<code v-pre>{{ secrets.* }}</code>, the run fails with an error when the daemon
starts. Run such a task directly with `mise run` instead.

## Run setup commands first {#setup-before-the-process-starts}

Use `init` for setup that must finish before the process starts. It takes one
command or a list, and works with `run`, `task`, and presets:

```toml
[daemons.api]
init = ["npm ci", "npm run migrate"]
run = "exec npm start"
ready_port = 3000
```

Each command must succeed before the next runs. The setup commands and the
long-running command share a shell, so an exported variable or a `cd` carries
through to the commands after it. A task daemon still applies the task's own
environment and working directory. Setup runs in the project's tool
environment; `mise = false` turns that off for both setup and the process.

`init` runs on every start and restart, including automatic restarts, so make
each command safe to repeat: `npm ci`, or a migration tool that skips applied
migrations.

Readiness checks start after setup, so tasks and daemons that wait for this
daemon also wait for `init`. On a preset, the preset's own data initialization
runs before your `init` commands, even when you override `run`.

## Manage daemons {#manage-running-daemons}

Daemon names in these commands can be short (`api`) or full IDs (`shop/api`).
Other flags pass through to pitchfork.

| Command                                                     | What it does                                                                                                                                                                                                                                            |
| ----------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [`mise daemons start [NAME…]`](/cli/daemons/start.html)     | Installs missing tools, registers the project with pitchfork, starts the daemons and their `depends`, and waits until they are ready. With no names, starts the [`default` group](#groups) if the project declares one, otherwise every project daemon. |
| [`mise daemons stop [NAME…]`](/cli/daemons/stop.html)       | Stops daemons. With no names, stops every daemon the project has registered.                                                                                                                                                                            |
| [`mise daemons restart [NAME…]`](/cli/daemons/restart.html) | Restarts daemons, choosing them as `start` does.                                                                                                                                                                                                        |
| [`mise daemons ls [--json]`](/cli/daemons/ls.html)          | Lists daemons and their status without registering anything or starting a supervisor. `mise daemons` alone does the same.                                                                                                                               |
| [`mise daemons status [NAME…]`](/cli/daemons/status.html)   | Shows pitchfork's status for each daemon.                                                                                                                                                                                                               |
| [`mise daemons logs [NAME…]`](/cli/daemons/logs.html)       | Shows daemon output.                                                                                                                                                                                                                                    |
| [`mise daemons urls [--json]`](/cli/daemons/urls.html)      | [Lists hostnames and ports](/daemons/worktrees.html#list-urls).                                                                                                                                                                                         |
| [`mise daemons register`](/cli/daemons/register.html)       | Registers every daemon without starting any, for [on-demand startup](/daemons/worktrees.html#register-for-on-demand-startup).                                                                                                                           |
| [`mise daemons prune`](/cli/daemons/prune.html)             | [Removes state left by deleted projects](/daemons/data.html#clean-up-deleted-projects).                                                                                                                                                                 |
| [`mise daemons tui`](/cli/daemons/tui.html)                 | Opens pitchfork's dashboard.                                                                                                                                                                                                                            |
| [`mise daemons providers`](/cli/daemons/providers.html)     | Manages [shared servers](/daemons/sharing.html#shared-server-providers).                                                                                                                                                                                |

`logs` and `tui` need a running supervisor and fail otherwise; start a daemon
first. A change to a daemon's declaration takes effect on its next start or
restart.

### Environment profiles

Only one [`MISE_ENV`](/configuration/environments.html) profile can run a
project's daemons at a time. Before you switch profiles, stop the project's
daemons and leave its shell sessions.

## Start and stop with your shell {#automatic-start-and-stop}

To start a daemon when you `cd` into the project, and stop it when the last
shell leaves, set pitchfork's `auto` key:

```toml
[daemons.postgres]
preset = "postgres"
version = "18"
auto = ["start", "stop"]
```

This needs:

- [Shell activation](/shell-setup.html) in Bash, Zsh, or Fish.
- pitchfork already installed, for example with `mise use -g pitchfork`. The
  shell hook never installs tools.
- `experimental = true` in your global config, for example with
  `mise settings set experimental true`. mise stops updating daemon sessions
  in any directory where `experimental` is off. With the setting only in the
  project's `mise.toml`, leaving the project does not end the shell's session,
  so `auto = ["stop"]` does not fire until the shell enters a directory where
  the setting is on, or exits.

The `postgres = "18"` shorthand cannot set `auto`; use a table. The hook starts
daemons in the background, so your prompt does not wait for them. A daemon
keeps running while any shell is still in the project. Leaving for another
directory releases this shell's session, even when the new directory has no
daemons. Only the project's own daemons start this way; start a daemon
[from another project](/daemons/sharing.html#use-a-daemon-from-another-project)
with `mise daemons start`.

If a start fails, the hook prints the error and tries again on the next
directory or config change. To retry now, run the hook with your shell's PID:

```sh
eval "$(mise hook-env --force --shell-pid $$ -s zsh)"   # use -s bash in Bash
mise hook-env --force --shell-pid $fish_pid -s fish | source   # Fish
```

A forced hook, like `mise daemons start`, also reattaches a generated config
that was detached with pitchfork. Nothing starts in safe mode or when hooks are
disabled. If mise says that automatic start and stop requires updated shell
activation, restart your shell.

pitchfork daemons with their own `auto` setting share these shell sessions. See
pitchfork's [shell hook guide](https://pitchfork.jdx.dev/guides/shell-hook).

## Group daemons {#groups}

Name a set of daemons in `[daemon_groups]` and use the group name anywhere a
daemon name works. A group named `default` is what `mise daemons start` and
`restart` start when you give no names.

```toml
[daemon_groups]
default = ["postgres", "api"]
full = ["default", "worker", "search"]
```

```sh
mise daemons start           # postgres and api
mise daemons start full      # all four
mise daemons start --all     # every daemon in this project
mise daemons stop            # every daemon this project has registered
mise daemons stop --group full
```

Members are daemons or groups declared in the same project, and a member group
expands in place. mise checks members when it loads the config, so a group
never selects a daemon outside the project, including a same-named daemon in a
parent or child project. A group needs at least one member and cannot share a
name with a daemon in the same project. A table form, matching pitchfork's
syntax, also works:

```toml
[daemon_groups.full]
daemons = ["default", "worker", "search"]
```

`--group NAME` selects only a `[daemon_groups]` group, never a daemon of that
name, and rejects a group defined only in pitchfork's own config, because such
a group can include daemons outside the project. Use pitchfork directly for
those. `--all` on `start`, `stop`, or `restart` covers every daemon in this
project, including those a `default` group leaves out. Unlike pitchfork's own
`--all`, it never reaches other projects' daemons, and it cannot be combined
with names or `--group`.

Removing a group from the config does not stop the daemons it started.
`mise daemons ls` still lists them, and `mise daemons stop` with no names stops
them.

### Groups in nested projects

In nested projects, each project resolves a name itself: its group of that
name if it has one, otherwise its daemon. Each project also applies its own
`default` group, so a `default` group in one project does not limit what
another starts.

When a child project [redefines](#override-a-daemon) a daemon that a parent's
group lists, the group starts the child's definition, so you still get one
process. For example, with a parent declaring `default = ["postgres", "api"]`
and a child redefining `postgres`, starting from the child runs the child's
`postgres` and the parent's `api`.

### Groups in pitchfork

mise writes groups into the generated pitchfork config with full daemon IDs, so
`pitchfork start --group full` works too. pitchfork group names are global to
its configuration, so choose names that differ across projects if you run
pitchfork directly.

## Windows

pitchfork runs `run` and `init` commands with `cmd /C` on Windows. Write them
for cmd: leave out `exec`, which cmd does not have, and expect the `quote`
filter to quote for cmd rather than a POSIX shell. A
[task daemon](#daemons-that-run-a-task) without `init` needs no shell, so its
`args` work unchanged; with `init`, write the setup steps as cmd commands.

[Service presets](/daemons/presets.html#windows) and
[shared server providers](/daemons/sharing.html#shared-server-providers) have
their own Windows limits. Start and stop with your shell needs Bash, Zsh, or
Fish.

## Next steps

- [Set up a development stack](/daemons/development-stack.html): several
  services, one URL per worktree, and a supervisor that starts at login.
- [Service presets](/daemons/presets.html): PostgreSQL, Redis, CockroachDB,
  NATS, and SpiceDB, and their options.
- [Ports, URLs, and worktrees](/daemons/worktrees.html): run the same daemons
  in several Git worktrees at once.
- [Share daemons across projects](/daemons/sharing.html): start another
  repository's daemon, or share one database server.
- [Data and cleanup](/daemons/data.html): find, reset, and prune daemon data.
