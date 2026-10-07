---
description: "Use fnox as a project's secrets source: tasks receive only the secrets they list, mise run --secrets and mise x --secrets grant one-off secrets, and mise secrets ls shows what is available."
---

# mise secrets with fnox

<Badge type="warning" text="experimental" />

[fnox](https://fnox.jdx.dev) can be the secrets source for a project. A task receives exactly the
secrets it lists in `secrets = [...]`, as environment variables, and only while it runs. mise asks
fnox for a value only when a task that lists it is about to run, keeps it in memory for that
process only, and redacts it from the task's output. `mise run --secrets` and `mise x --secrets`
grant a secret for one command instead (see [Grant for one command](#grant-for-one-command)).
`mise secrets ls` shows which keys exist and which tasks list them; it never shows values.

mise secrets is experimental. Enable it with:

```sh
mise settings experimental=true
```

## Quick start

Add fnox to the project and name it as the secrets source:

```toml [mise.toml]
[tools]
fnox = "latest"

[secrets.fnox]          # this project's secrets come from fnox, discovered from this directory
profile = "dev"         # optional; passed as fnox -P
```

```sh
mise use fnox
mise secrets ls
```

```
fnox · profile dev · ~/src/app (mise.toml) · fnox 1.39.0 · daemon: running (protocol 6)
KEY                ENV    FILE  SCOPES     TASKS   DESCRIPTION
AWS_ACCESS_KEY_ID  -      no    run, exec          (lease aws)
DATABASE_URL       true   no    run, exec  deploy  app database
DEPLOY_KEY         exec   no    run, exec  deploy
GCP_SA_JSON        exec   yes   run
SIGNING_KEY        false  no    -                  release signing key
```

The header goes to stderr and the table to stdout. mise needs fnox 1.39.0 or newer. The header ends
with the state of fnox's daemon (see [Caching with the fnox daemon](#caching-with-the-fnox-daemon)):
`daemon: running (protocol N)`, `daemon: not running`, `daemon: disabled`, `daemon: not responding`,
`daemon: socket <path> is not owned by you`, or `daemon: not used (...)` for an fnox too old to
report it. Only an enabled daemon is asked, and that never starts one.

## Granting secrets to tasks

```toml [mise.toml]
min_version = "2026.10.4"   # older mise rejects `secrets` on a task

[secrets.fnox]
profile = "prod"

[tasks.build]
run = "cargo build"                         # receives nothing; fnox never runs for it

[tasks.deploy]
depends = ["build"]
secrets = ["DEPLOY_KEY", "DATABASE_URL"]    # only these keys, only this task
run = './deploy.sh'                          # reads $DEPLOY_KEY
```

`mise run deploy` does the following:

1. Validates every grant up front with one `fnox env --json --describe` call. Nothing runs if a
   key is unknown or cannot be injected.
2. Runs `build` with nothing.
3. Makes one fnox call for the two keys.
4. Starts `deploy` with them in its environment.
5. Prints `[redacted]` wherever a value appears in the task's output.

fnox is not run for a task without a grant, a task skipped because its sources are fresh, a
declined confirmation, `mise run -n`, `mise env`, hook-env or a shim.

The `secrets` field is also available in file task headers (`#MISE secrets=["DEPLOY_KEY"]`).
`secrets = []` in a `[tasks.<name>]` block clears the list of a file task. `secrets` is not
allowed in `[task_templates.*]` or `monorepo.task_defaults`: every grant is written on the task
that receives it.

### Who gets what

| Process                                               | Receives the secret            |
| ----------------------------------------------------- | ------------------------------ |
| A task that lists the key                             | Yes                            |
| Its dependencies and post-dependencies                | No, only their own lists       |
| Tasks started by `run = [{ task = "..." }]`           | No, only their own lists       |
| A nested `mise run` inside the task                   | Only tasks that list the key   |
| Shims, `mise x` and tools the task calls              | Yes, they inherit the variable |
| A task named with `mise run --secrets KEY`            | Yes, for that run only         |
| `mise x` without `--secrets`                          | No                             |
| Hooks, `mise env`, hook-env, `mise activate`, daemons | No                             |

Inherited values travel like any environment variable: a script that grants itself a key can
pass it to every program it starts. List only what a task needs.

### Grant for one command {#grant-for-one-command}

A one-off grant from the command line follows the same rule as a `secrets` list: only the process
you name receives the value.

```sh
mise run --secrets STRIPE_KEY deploy        # deploy gets it; its dependencies do not
mise run --secrets-all deploy ::: smoke     # every injectable key, to those two tasks only
mise x --secrets GH_TOKEN -- gh release list
mise x -- gh release list                   # nothing; fnox never runs
```

- `mise run --secrets KEY[,KEY...]` gives the keys to the tasks named on the command line. That
  includes tasks selected by a glob, by `default` or by `--all`, but never their dependencies,
  post-dependencies or tasks started by a `run` entry. A task's own `secrets` list still applies.
- `--secrets-all` gives every key the project can inject (fnox `env = true` or `"exec"`, never
  `env = false`), including keys produced by dynamic leases, to the same named tasks. A key that
  mise itself sets for the task, or that the task's sandbox would drop, is skipped with a warning
  instead of failing the run.
- `mise run` uses `unknown_flags = "value"`, so a flag after the task name goes to the task:
  `mise run deploy --secrets X` passes `--secrets` to `deploy` and prints a warning. Put mise
  flags first. Arguments after `--` are always the task's.
- `mise x --secrets` and `--secrets-all` give the command the values and nothing else; a plain
  `mise x` never starts fnox. There is no setting or environment variable that turns these flags
  on, and shims, `mise en` and pitchfork daemon probes never pass them.
- `mise x` cannot give file secrets (`as_file = true`), because mise hands the process over with
  `exec` and cannot delete the file afterwards. `--secrets GCP_SA_JSON` fails and `--secrets-all`
  skips file keys (use a task, or `fnox exec -- <command>`). `--secrets-all` for `mise x` also asks
  fnox for each key by name, so it never includes keys that only a dynamic lease would produce.
- mise does not redact the output of `mise x`, because `exec` replaces mise. This is the same as
  `fnox exec`.
- The flags need a `[secrets.fnox]` source in the project, are experimental, and are refused in
  safe mode.

### Compose values {#compose-values}

A task's own `env` values may reference a secret with <span v-pre>`{{ secrets.NAME }}`</span>. The
reference is the grant: you do not also list the key in `secrets`.

```toml
[tasks.migrate]
env.PGURL = "postgres://app:{{ secrets.DB_PASSWORD }}@db.internal/app"
run = 'psql "$PGURL" -f schema.sql'
```

`PGURL` is composed just before the task starts and redacted in its output. `DB_PASSWORD` itself
is not exported unless the task also lists it in `secrets` (`secrets = ["DB_PASSWORD"]` exports it
alongside `PGURL`). An env value cannot take the name of a key the task exports: `secrets =
["DB_PASSWORD"]` with <span v-pre>`env.DB_PASSWORD = "{{ secrets.DB_PASSWORD }}x"`</span> is an
error, because both would claim the name. `--secrets-all` still gives the task every injectable key
under its own name, `DB_PASSWORD` included; a reference only keeps a key from going through the
checks for listed keys, so a name mise sets or the sandbox drops is skipped with a warning instead
of failing.

- Only literal text and <span v-pre>`{{ secrets.NAME }}`</span> (spaces inside the braces are
  optional) may appear in such a value. Filters, other variables, <span v-pre>`{{-`</span>,
  `secrets["NAME"]` and `{% raw %}` are an error; compose anything fancier in fnox or in the
  task's script.
- When `env_shell_expand` is on (the default), `$NAME`, `${...}` and `$$` anywhere in the literal
  text of such a value are an error, because mise does not shell-expand a value it composes from
  secrets. A `$` right before a reference, <span v-pre>`${{ secrets.X }}`</span>, is always an
  error.
- A secret that fnox delivers as a file (`as_file = true`) cannot be composed into a value.
- Allowed only in a task's own `env` values (and a file task's `#MISE env=` header). Not in
  `run` (it becomes `sh -c` arguments, which other local users can read through `ps`; read
  `$NAME` instead), not in `[env]` or `[vars]`, and not in `depends`, their `env`, run-entry
  `env`, `[task_templates]`, `task_defaults`, hooks or `[tools]`.
- A composed value takes part in the usual env precedence. It overrides a parent task's env
  (through a run entry), a template's or a lower config block's value, and defaults. A
  dependency's or run entry's env replaces a value from the task's own definition, but a value
  from a `[tasks.<name>]` block layered over the task still wins, as plain values do. A higher
  block's value or `env.NAME = false` replaces it, and then nothing is fetched. Env that reaches
  a task from a dependency or a run entry is never a grant, so it cannot smuggle a reference in.
  `[env]`, tools and settings must not set the same name.
- Other env values cannot read the composed variable, with <span v-pre>`{{ env.PGURL }}`</span> or
  `$PGURL`; build them from secrets directly. mise checks the env values, defaults and path
  directives in the config, and also the values it decrypts (age). What a directive only reads
  while it renders, such as the contents of a `_.file` dotenv file or a `_.source` script, is not
  checked and sees the value from before the secret.
- The same rules apply as for listed keys: remote and non-project tasks cannot use references,
  and the sandbox must keep the variable.

`mise tasks info` shows the template, never a value, and `mise secrets ls` lists the task as
`migrate (env.PGURL)`.

### Where values go, and where they never go

Values go only into the environment of the process mise starts for the task, after mise has
rendered every template. They are added after `__MISE_DIFF` is computed and never go into
`__MISE_DIFF` or `__MISE_SESSION`, the env cache, template contexts, hook-env, shims, `mise env`
output, the task cache, or the command line of `sh -c`. mise sets `__MISE_SECRET_KEYS` (names
only) in the task's environment so a nested mise knows not to pass them on. Keys fnox provides as
files (`as_file = true`) are written to a 0700 directory with 0600 permissions, `KEY` is set to
the path, and the files are deleted when the task ends.

Tasks run as the same operating system user as mise. Grants keep a secret out of tasks that were
not granted it, but they are not isolation from other processes of that user: such a process can
read a granted task's environment and its secret files while it runs.

<span v-pre>`{{ env.DEPLOY_KEY }}`</span> in `run` cannot see a granted secret, because the secret is added after
`run` is rendered. Read it from the environment instead (`"$DEPLOY_KEY"`). mise reports this
before running anything.

A key that mise also sets for the task (`[env]`, the task's `env`, a tool or a setting) is an
error: tools started through shims recompute it and would replace the secret. A key the task's
sandbox would drop is an error too; add it to `allow_env`.

### Signing in, CI and the fnox daemon

When mise runs in a terminal, fnox may prompt you to sign in (a password manager, for example).
mise gives fnox the terminal and waits for other running tasks to finish first, as it does for an
`interactive = true` task. fnox follows its own `[daemon]` setting and may start its daemon; mise
never starts it.

In CI, without a TTY, or when stdin is not a terminal, mise runs fnox with `--non-interactive
--no-daemon`. fnox cannot prompt there, so sign in first (for example `op signin`) or give CI the
provider's credentials.

### Caching with the fnox daemon

When fnox's daemon is running and has your values cached, mise reads them straight from the
daemon's socket. A hit skips the resolving `fnox env --json` call: no provider calls and no
prompt. mise still runs `fnox env --json --describe` once per run to check the grants.

```toml
# fnox.toml
root = true

[daemon]
enabled = true
```

| fnox says                                                                                            | mise does                                                                                                                                                                                                           |
| ---------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `[daemon] enabled = true` in the project's fnox config, a daemon is running, and every key is cached | One socket round trip instead of a resolving fnox call. Debug output (`MISE_DEBUG=1`) shows `secrets: fnox daemon hit`.                                                                                             |
| A key is not cached, or needs a lease                                                                | Runs `fnox env --json` on your terminal. fnox resolves, prompts if it must, and stores the values in the daemon, so the next run hits.                                                                              |
| `[daemon] enabled = false`, or `FNOX_DAEMON=0`                                                       | Runs the fnox CLI and never contacts the daemon. mise reads fnox's own decision from `fnox env --json --describe`, so an fnox too old to report it also means CLI only.                                             |
| No daemon is running                                                                                 | Runs the fnox CLI. If your fnox config enables the daemon, fnox may start it; mise never runs `fnox daemon start`.                                                                                                  |
| The daemon speaks another protocol, or its socket is not owned by you                                | Runs the fnox CLI.                                                                                                                                                                                                  |
| A daemon that accepts the connection but does not answer in time                                     | Skipped for the rest of the run: the fnox CLI runs with `--no-daemon` (still interactive, so prompts work).                                                                                                         |
| CI, no TTY, or stdin is not a terminal                                                               | Runs `fnox env --json --non-interactive --no-daemon`. CI never resolves through the daemon. Only the `mise secrets ls` table asks a running daemon for its protocol, and only when fnox reports the daemon enabled. |

Optional keys that resolve to nothing, and keys that need a lease, always go through the CLI.
Windows has no fnox daemon, so mise always uses the CLI there. fnox protocol 6 moved the daemon's
socket, so a daemon from an older fnox is ignored until it exits. This needs an fnox release with
daemon protocol 6; with an older fnox mise keeps using the CLI.

### Redaction, raw output and stdin

A task that receives secrets always has its output redacted, so mise ignores `--raw` and the
`raw` setting for it and does not connect stdin. Set `raw = true` or `interactive = true` on the
task to hand it the terminal; its output is then not redacted. The one-time hint and the warning
explain this when it comes up.

### Artifact cache

A task that receives secrets does not use the [task artifact cache](/tasks/task-configuration.html),
because cached output could hold a secret. The plain `sources`/`outputs` freshness check still
applies.

### Launchers that refuse grants

A task that lists secrets does not run when it was started by a mise hook, `watch_files`, a
pitchfork daemon or `mise bootstrap`. Run it directly with `mise run`.

A hook with `shell = ...` runs in your own shell, so mise marks that shell (`__MISE_SECRETS_DENIED`)
while the script runs. If the script returns early or is interrupted, the mark stays and `mise run`
refuses secrets in that shell until you open a new one or `unset __MISE_SECRETS_DENIED`. This
guards against a task receiving secrets nobody asked for; a script can unset the variable itself. Tasks from remote sources
(`git::`, `oci::` or URL includes) and tasks defined in global or system config cannot list
secrets either. A task file or a `[tasks]` entry that lists secrets also requires the config to
be trusted, even though plain task definitions do not.

### Trust

`mise run` implicitly trusts the active config outside CI and `paranoid` mode, so cloning a
repository and running a task in it runs that repository's tasks, including any that list
secrets. On machines where AI agents or untrusted repositories are common, set `paranoid = true`
so every config must be trusted explicitly.

### Troubleshooting

| Message                                                       | What to do                                                                                                                    |
| ------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| `unknown secret X`                                            | The key is not in fnox's profile. Check `mise secrets ls`, the spelling, and the profile.                                     |
| `--secrets after the task name ...`                           | Put mise flags before the task name: `mise run --secrets X deploy`.                                                           |
| `X is a file secret` (`mise x`)                               | `mise x` cannot give files. Use a task (`mise run`) or `fnox exec -- <command>`.                                              |
| `X cannot be injected`                                        | fnox sets `env = false` for it. Read it with `fnox get X`, or set `env = "exec"` in `fnox.toml`.                              |
| `"x y" is not a valid environment variable`                   | Use names matching `[A-Za-z_][A-Za-z0-9_]*`.                                                                                  |
| `secrets = true is not supported`                             | List key names: `secrets = ["DEPLOY_KEY"]`.                                                                                   |
| per-task source options                                       | `secrets = { fnox = ... }` is not supported yet; list key names.                                                              |
| wildcards are not supported                                   | List each key.                                                                                                                |
| `secrets is not allowed in [task_templates]`                  | Move `secrets` to each task.                                                                                                  |
| comes from a remote source                                    | Remote tasks cannot list secrets. Copy the task into your project.                                                            |
| not project config                                            | Global and system config cannot list secrets. Move the task into the project.                                                 |
| started by a mise hook, shell hook, watch_files, ...          | Run the task directly: `mise run <task>`. After an interrupted shell hook, open a new shell or `unset __MISE_SECRETS_DENIED`. |
| `X is both a secret and a mise env var`                       | Keep one: move the default into `fnox.toml` or rename the mise variable.                                                      |
| secret name `PATH` is reserved                                | Names mise uses itself cannot be granted.                                                                                     |
| its sandbox denies env vars                                   | Add `allow_env = ["X"]` to the task or pass `--allow-env X`.                                                                  |
| `fnox could not resolve X`                                    | fnox's provider failed. Sign in, or give CI the provider's credentials. The message is fnox's.                                |
| `not retrying X`                                              | fnox failed for it earlier in this run; fix the first error.                                                                  |
| <span v-pre>`{{ env.X }} in run cannot see the secret`</span> | Read `"$X"` from the environment instead.                                                                                     |

## Where the source is declared

`[secrets.fnox]` is read from project `mise.toml` files only.

- The nearest file wins per field. `mise.local.toml` and `mise.<env>.toml` can override
  `profile`. fnox runs in the directory of the nearest file that declares `[secrets.fnox]`, and
  finds its `fnox.toml` from there.
- Global config, system config, and files in or above your home directory are ignored, because
  they apply to every project. `mise secrets ls` and `mise doctor` name any ignored file.
- The file must be [trusted](/cli/trust.html), as with the rest of project config.
- Safe mode (`MISE_SAFE=1`) refuses to use secrets sources.

## `mise secrets ls`

| Column      | Meaning                                                                   |
| ----------- | ------------------------------------------------------------------------- |
| KEY         | The name of the secret or lease key                                       |
| ENV         | fnox's `env` setting: `true`, `exec`, `false`, or `-` for lease keys      |
| FILE        | `yes` if fnox provides the value as a file                                |
| SCOPES      | `run`, `exec`: where the key can be granted, `-` if fnox never injects it |
| TASKS       | Tasks whose `secrets` list names the key (and whose source is this)       |
| DESCRIPTION | fnox's description, or `(lease <name>)`                                   |

Grant problems, such as a task listing a key fnox does not have, print to stderr as warnings
with a suggestion; the exit code stays 0.

`-J`/`--json` prints the same information as JSON:

```json
{
  "source": {
    "kind": "fnox",
    "root": "/home/me/src/app",
    "declared_in": ["/home/me/src/app/mise.toml"],
    "profile": "dev",
    "tool": {
      "path": "/home/me/.local/share/mise/installs/fnox/1.39.0/fnox",
      "version": "1.39.0"
    }
  },
  "keys": [
    {
      "key": "DEPLOY_KEY",
      "kind": "secret",
      "env": "exec",
      "as_file": false,
      "lease": null,
      "description": null,
      "scopes": ["run", "exec"],
      "tasks": [
        {
          "task": "deploy",
          "via": "list",
          "file": "/home/me/src/app/mise.toml"
        }
      ]
    }
  ],
  "dynamic_leases": [],
  "ignored": [],
  "problems": []
}
```

`env` is `true`, `"exec"`, `false`, or `null` for lease keys. `scopes` is `["run", "exec"]` for
keys tasks and `mise x` can receive, `["run"]` for file keys (tasks only) and `[]` for keys fnox
never injects.
`problems` lists grants that name an unknown or non-injectable key, each with `task`, `key`,
`kind` and an optional `suggestion`. `profile` is the profile fnox used,
including `FNOX_PROFILE` or `default` when `mise.toml` sets none.

mise looks for the fnox CLI in the project's tools first, then on `PATH`.

## Migrating from mise-env-fnox {#migrating}

`mise-env-fnox` and `_.fnox-env` put every fnox secret into mise's environment, where every
task, hook, shim and `mise env` sees it. To move to mise secrets:

1. Remove `_.fnox-env` and the `mise-env-fnox` plugin from `mise.toml`.
2. Add `[secrets.fnox]` (and `profile`, if you used one).
3. Add `secrets = [...]` to each task that needs a key, listing only that key. Read it from the
   environment in the script (`$DEPLOY_KEY`).
4. Check the result with `mise secrets ls` and `mise tasks validate`.

Values in your shell and in `mise env` no longer appear, which is the point; if a tool you run
outside `mise run` needs a secret, use `fnox exec -- <command>`. `mise doctor` flags the plugin as
deprecated. For a one-off command, use `mise x --secrets KEY -- <command>`.
