---
description: "Use fnox as a project's secrets source: tasks receive only the secrets they list, and --secrets grants a secret for one command."
---

# fnox <Badge type="warning" text="experimental" />

[fnox](https://fnox.jdx.dev) can supply a project's secrets. A task lists the
keys it needs in `secrets = [...]`. Just before the task starts, mise fetches
those values from fnox, passes them to that task only as environment variables,
and redacts them from its output. Your shell, other tasks and `mise env` never
see them.

::: warning Experimental
mise secrets is experimental. Enable it with `experimental = true` under
`[settings]`, or run `mise settings experimental=true`.
:::

## Set up {#quick-start}

### 1. Enable the feature and add fnox

```sh
mise settings experimental=true
mise use fnox
```

mise needs fnox 1.39.0 or newer. It looks for the fnox CLI in the project's
tools first, then on `PATH`.

### 2. Name fnox as the source and grant a key

```mise-toml [mise.toml]
min_version = "2026.10.4"   # older mise rejects `secrets` on a task

[secrets.fnox]

[tasks.deploy]
secrets = ["DATABASE_URL"]
run = './deploy.sh'         # reads $DATABASE_URL
```

To use an fnox profile, add `profile = "dev"` under `[secrets.fnox]`; mise
passes it to fnox as `-P dev`.

### 3. Define the secret in fnox

```toml [fnox.toml]
[secrets]
DATABASE_URL = { default = "postgres://localhost/app" }
```

A `default` is a plaintext value, which is enough to try the flow. For real
secrets, configure an fnox provider such as age, 1Password or AWS Secrets
Manager; see the [fnox quick start](https://fnox.jdx.dev/guide/quick-start.html).

### 4. Check what mise sees

```sh
mise secrets ls
```

```text
KEY           ENV   FILE  SCOPES     TASKS   DESCRIPTION
DATABASE_URL  true  no    run, exec  deploy
```

A header line on stderr names the source, the profile, the fnox version and the
state of fnox's daemon. See [`mise secrets ls`](#mise-secrets-ls) for every
column.

### 5. Run the task

```sh
mise run deploy
```

`deploy` receives `DATABASE_URL`, and any place the value appears in its output
prints `[redacted]`.

### Where the source is declared {#where-the-source-is-declared}

mise reads `[secrets.fnox]` from project `mise.toml` files only.

- The nearest file wins for each field. `mise.local.toml` and `mise.<env>.toml`
  can override `profile`. fnox runs in the directory of the nearest file that
  declares `[secrets.fnox]`, and finds its `fnox.toml` from there.
- Global config, system config, and files in or above your home directory are
  ignored, because they apply to every project. `mise secrets ls` and
  `mise doctor` name any file they ignore.
- The file must be [trusted](/cli/trust.html), as with the rest of project
  config.
- Safe mode (`MISE_SAFE=1`) refuses to use secrets sources.

## Grant secrets to a task {#granting-secrets-to-tasks}

```mise-toml [mise.toml]
min_version = "2026.10.4"

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

1. Checks every key the run needs. Nothing runs if a key is unknown or cannot
   be injected.
2. Runs `build` with nothing.
3. Makes one fnox call for the two keys.
4. Starts `deploy` with them in its environment.
5. Prints `[redacted]` wherever a value appears in the task's output.

fnox does not run for a task without a grant, a task skipped because its sources
are fresh, a declined confirmation, `mise run -n`, `mise env`, an activated
shell or a shim.

File tasks take the same field in their header:
`#MISE secrets=["DEPLOY_KEY"]`. `secrets = []` in a `[tasks.<name>]` block
clears the list of a file task. `secrets` is not allowed in `[task_templates.*]`
or `monorepo.task_defaults`, so every grant is written on the task that
receives it.

### Who gets what {#who-gets-what}

| Process                                                         | Receives the secret            |
| --------------------------------------------------------------- | ------------------------------ |
| A task that lists the key                                       | Yes                            |
| Its dependencies and post-dependencies                          | No, only their own lists       |
| Tasks started by `run = [{ task = "..." }]`                     | No, only their own lists       |
| A nested `mise run` inside the task                             | Only tasks that list the key   |
| Programs the task starts, including shims and a nested `mise x` | Yes, they inherit the variable |
| A task named with `mise run --secrets KEY`                      | Yes, for that run only         |
| `mise x` from your shell, without `--secrets`                   | No                             |
| Hooks, `mise env`, `mise activate`, daemons                     | No                             |

Inherited values travel like any environment variable: a script that receives a
key can pass it to every program it starts. List only what a task needs.

## Grant for one command {#grant-for-one-command}

A one-off grant from the command line follows the same rule as a `secrets`
list: only the process you name receives the value.

```sh
mise run --secrets STRIPE_KEY deploy        # deploy gets it; its dependencies do not
mise run --secrets-all deploy ::: smoke     # every injectable key, to those two tasks only
mise x --secrets GH_TOKEN -- gh release list
mise x -- gh release list                   # nothing; fnox never runs
```

- `mise run --secrets KEY[,KEY...]` gives the keys to the tasks named on the
  command line. That includes tasks selected by a glob, by `default` or by
  `--all`, but never their dependencies, post-dependencies or tasks started by a
  `run` entry. A task's own `secrets` list still applies.
- `--secrets-all` gives every key the project can inject (fnox `env = true` or
  `"exec"`, never `env = false`), including keys produced by dynamic leases, to
  the same named tasks. A key that mise itself sets for the task, or that the
  task's sandbox would drop, is skipped with a warning instead of failing the
  run.
- `mise run` passes flags that come after the task name to the task, so
  `mise run deploy --secrets X` gives `--secrets` to `deploy` and prints a
  warning. Put mise flags first. Arguments after `--` always go to the task.
- `mise x --secrets` and `--secrets-all` give the command the values and nothing
  else. No setting or environment variable turns these flags on, and shims,
  `mise en` and pitchfork daemon probes never pass them.
- `mise x` cannot give file secrets (`as_file = true`), because mise hands the
  process over with `exec` and cannot delete the file afterwards.
  `--secrets GCP_SA_JSON` fails and `--secrets-all` skips file keys; use a task,
  or `fnox exec -- <command>`. `--secrets-all` for `mise x` also asks fnox for
  each key by name, so it never includes keys that only a dynamic lease would
  produce.
- mise does not redact the output of `mise x`, because `exec` replaces mise.
  `fnox exec` behaves the same way.
- The flags need a `[secrets.fnox]` source in the project, and safe mode refuses
  them.

## Compose values {#compose-values}

A task's own `env` values may reference a secret with
<span v-pre>`{{ secrets.NAME }}`</span>. The reference is the grant: you do not
also list the key in `secrets`.

```mise-toml [mise.toml]
[tasks.migrate]
env.PGURL = "postgres://app:{{ secrets.DB_PASSWORD }}@db.internal/app"
run = 'psql "$PGURL" -f schema.sql'
```

mise composes `PGURL` just before the task starts and redacts it in the task's
output. `DB_PASSWORD` itself is not exported unless the task also lists it in
`secrets`. `mise tasks info` shows the template, never a value, and
`mise secrets ls` lists the task as `migrate (env.PGURL)`.

- The value may contain only literal text and
  <span v-pre>`{{ secrets.NAME }}`</span> references. Filters, other variables,
  <span v-pre>`{{-`</span>, `secrets["NAME"]` and `{% raw %}` are an error;
  compose anything more complex in fnox or in the task's script.
- References are allowed only in a task's own `env` values, including a file
  task's `#MISE env=` header. They are not allowed in `run` (it becomes `sh -c`
  arguments, which other local users can read with `ps`; read `$NAME` instead),
  in `[env]` or `[vars]`, in `depends`, their `env` or run-entry `env`, in
  `[task_templates]` or `task_defaults`, in hooks, or in `[tools]`.
- Other env values cannot read the composed variable through
  <span v-pre>`{{ env.PGURL }}`</span> or `$PGURL`; build them from secrets
  directly.
- A secret that fnox delivers as a file (`as_file = true`) cannot be composed.
- Remote and non-project tasks cannot use references, and the task's sandbox
  must keep the variable, as for listed keys.

### Composition rules {#composition-rules}

- When `env_shell_expand` is on (the default), `$NAME`, `${...}` and `$$`
  anywhere in the literal text of a composed value are an error, because mise
  does not shell-expand a value it builds from secrets. A `$` right before a
  reference, <span v-pre>`${{ secrets.X }}`</span>, is always an error.
- An env value cannot take the name of a key the task exports: `secrets =
["DB_PASSWORD"]` with <span v-pre>`env.DB_PASSWORD = "{{ secrets.DB_PASSWORD }}x"`</span>
  is an error, because both would claim the name.
- `--secrets-all` still gives the task every injectable key under its own name,
  `DB_PASSWORD` included. A reference only keeps a key out of the checks for
  listed keys, so a name that mise sets or the sandbox drops is skipped with a
  warning instead of failing.
- A composed value takes part in the usual env precedence. It overrides a
  parent task's env (through a run entry), a template's or a lower config
  block's value, and defaults. A dependency's or run entry's env replaces a
  value from the task's own definition, but a value from a `[tasks.<name>]`
  block layered over the task still wins, as plain values do. A higher block's
  value or `env.NAME = false` replaces it, and then nothing is fetched. Env that
  reaches a task from a dependency or a run entry is never a grant, so it cannot
  carry a reference in. `[env]`, tools and settings must not set the same name.
- mise checks env values, defaults and path directives in the config, and the
  values it decrypts (age), for reads of the composed variable. What a directive
  reads only while it renders, such as the contents of a `_.file` dotenv file or
  a `_.source` script, is not checked and sees the value from before the secret.

## How mise protects values {#where-values-go-and-where-they-never-go}

### Where values go {#where-values-go}

mise adds the values to the task's process after it renders every template.
They never reach your shell, `mise env`, shims you run yourself, the task cache,
mise's env cache, or the task's command line. Keys that fnox provides as files
(`as_file = true`) are written to a directory with owner-only permissions, `KEY`
is set to the file's path, and the files are deleted when the task ends.

Tasks run as the same operating system user as mise. Grants keep a secret away
from tasks that were not granted it, but they do not isolate it from other
processes of that user: such a process can read a granted task's environment
and its secret files while it runs.

<span v-pre>`{{ env.DEPLOY_KEY }}`</span> in `run` cannot see a granted secret,
because mise adds the secret after it renders `run`. Read it from the
environment instead (`"$DEPLOY_KEY"`). mise reports this before it runs
anything.

A key that mise also sets for the task, through `[env]`, the task's `env`, a
tool or a setting, is an error: tools started through shims recompute it and
would replace the secret. A key that the task's sandbox would drop is an error
too; add it to `allow_env`.

### Redaction, raw output and stdin {#redaction-raw-output-and-stdin}

A task that receives secrets always has its output redacted, so mise ignores
`--raw` and the `raw` setting for it and does not connect stdin. Set
`raw = true` or `interactive = true` on the task to hand it the terminal; its
output is then not redacted.

### Artifact cache {#artifact-cache}

A task that receives secrets does not use the
[task artifact cache](/tasks/caching.html), because cached output could hold a
secret. The plain `sources`/`outputs` freshness check still applies.

### Where secrets cannot be used {#launchers-that-refuse-grants}

- A task that lists secrets does not run when a mise hook, `watch_files`, a
  pitchfork daemon or `mise bootstrap` starts it. Run it directly with
  `mise run`.
- Tasks from remote sources (`git::`, `oci::` or URL includes) and tasks
  defined in global or system config cannot list secrets.
- A task file or a `[tasks]` entry that lists secrets requires its config to be
  trusted, even though plain task definitions do not.

A hook with `shell = ...` runs in your own shell, so mise marks that shell with
`__MISE_SECRETS_DENIED` while the script runs. If the script returns early or is
interrupted, the mark stays, and `mise run` refuses secrets in that shell until
you open a new one or run `unset __MISE_SECRETS_DENIED`. The mark keeps a task
from receiving secrets that nobody asked for; a script can unset it itself.

### Trust {#trust}

Outside [`paranoid`](/paranoid.html) mode, `mise run` implicitly trusts the
active config, and in CI mise trusts every config file, so cloning a repository
and running a task in it runs that repository's tasks, including any that list
secrets. On machines where AI agents or untrusted repositories are common, set
`paranoid = true` so every config must be trusted explicitly.

## Sign-in, CI and the fnox daemon {#signing-in-ci-and-the-fnox-daemon}

When mise runs in a terminal, fnox may prompt you to sign in, for example to a
password manager. mise gives fnox the terminal and waits for other running tasks
to finish first, as it does for an `interactive = true` task. fnox follows its
own `[daemon]` setting and may start its daemon; mise never starts it.

In CI, without a TTY, or when stdin is not a terminal, mise runs fnox with
`--non-interactive --no-daemon`. fnox cannot prompt there, so sign in first (for
example `op signin`) or give CI the provider's credentials.

### Caching with the fnox daemon {#caching-with-the-fnox-daemon}

When fnox's daemon is running and has your values cached, mise reads them
straight from the daemon's socket:

```toml [fnox.toml]
root = true

[daemon]
enabled = true
```

- A hit replaces the resolving `fnox env --json` call with one socket round
  trip: no provider calls and no prompt. `MISE_DEBUG=1` shows
  `secrets: fnox daemon hit`. mise still runs `fnox env --json --describe` once
  per run to check the grants.
- On a miss, when a key is not cached or needs a lease, mise runs
  `fnox env --json` on your terminal. fnox resolves the values, prompts if it
  must, and stores them in the daemon, so the next run hits.
- mise runs the fnox CLI and does not use the daemon when fnox's config sets
  `[daemon] enabled = false` or `FNOX_DAEMON=0` is set, when no daemon is
  running, when the daemon speaks a different protocol or its socket is not
  owned by you, in CI or without a terminal, and on Windows, which has no fnox
  daemon. mise reads fnox's own decision from `fnox env --json --describe`, so an
  fnox too old to report it also means CLI only.
- A daemon that accepts the connection but does not answer in time is skipped
  for the rest of the run; the fnox CLI then runs with `--no-daemon`, still
  interactively.
- Optional keys that resolve to nothing, and keys that need a lease, always go
  through the CLI.

## Troubleshooting {#troubleshooting}

| Message contains                                              | What to do                                                                                                                    |
| ------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| `unknown secret X`                                            | The key is not in fnox's profile. Check `mise secrets ls`, the spelling, and the profile.                                     |
| `--secrets after the task name`                               | Put mise flags before the task name: `mise run --secrets X deploy`.                                                           |
| `X is a file secret (as_file = true)`                         | `mise x` cannot give files. Use a task (`mise run`) or `fnox exec -- <command>`.                                              |
| `X cannot be injected`                                        | fnox sets `env = false` for it. Read it with `fnox get X`, or set `env = "exec"` in `fnox.toml`.                              |
| `is not a valid environment variable name`                    | Use names matching `[A-Za-z_][A-Za-z0-9_]*`.                                                                                  |
| `secrets = true is not supported`                             | List key names: `secrets = ["DEPLOY_KEY"]`.                                                                                   |
| `per-task source options`                                     | `secrets = { fnox = ... }` is not supported yet; list key names.                                                              |
| `wildcards are not supported`                                 | List each key.                                                                                                                |
| `secrets is not allowed in [task_templates.<t>]`              | Move `secrets` to each task.                                                                                                  |
| `comes from a remote source`                                  | Remote tasks cannot list secrets. Copy the task into your project.                                                            |
| `which is not project config`                                 | Global and system config cannot list secrets. Move the task into the project.                                                 |
| `but it was started by`                                       | Run the task directly: `mise run <task>`. After an interrupted shell hook, open a new shell or `unset __MISE_SECRETS_DENIED`. |
| `X is both a secret and a mise env var`                       | Keep one: move the default into `fnox.toml` or rename the mise variable.                                                      |
| `secret name PATH is reserved by mise`                        | Names mise uses itself cannot be granted.                                                                                     |
| `but its sandbox denies env vars`                             | Add `allow_env = ["X"]` to the task or pass `--allow-env X`.                                                                  |
| `fnox could not resolve X`                                    | fnox's provider failed. Sign in, or give CI the provider's credentials. The rest of the message comes from fnox.              |
| `not retrying X`                                              | fnox failed for it earlier in this run; fix the first error.                                                                  |
| <span v-pre>`{{ env.X }} in run cannot see the secret`</span> | Read `"$X"` from the environment instead.                                                                                     |

## `mise secrets ls` {#mise-secrets-ls}

[`mise secrets ls`](/cli/secrets/ls.html) shows which keys exist and which tasks
list them. It never shows values.

```text
fnox · profile dev · ~/src/app (mise.toml) · fnox 1.39.0 · daemon: running (protocol 6)
KEY                ENV    FILE  SCOPES     TASKS   DESCRIPTION
AWS_ACCESS_KEY_ID  -      no    run, exec          (lease aws)
DATABASE_URL       true   no    run, exec  deploy  app database
DEPLOY_KEY         exec   no    run, exec  deploy
GCP_SA_JSON        exec   yes   run
SIGNING_KEY        false  no    -                  release signing key
```

The header goes to stderr and the table to stdout. The header ends with the
state of fnox's daemon: `daemon: running (protocol N)`, `daemon: not running`,
`daemon: disabled`, `daemon: not responding`,
`daemon: socket <path> is not owned by you`, or `daemon: not used (...)` for an
fnox too old to report it. mise asks the daemon only when fnox reports it
enabled, and never starts one.

| Column      | Meaning                                                                   |
| ----------- | ------------------------------------------------------------------------- |
| KEY         | The name of the secret or lease key                                       |
| ENV         | fnox's `env` setting: `true`, `exec`, `false`, or `-` for lease keys      |
| FILE        | `yes` if fnox provides the value as a file                                |
| SCOPES      | `run`, `exec`: where the key can be granted, `-` if fnox never injects it |
| TASKS       | Tasks whose `secrets` list names the key (and whose source is this)       |
| DESCRIPTION | fnox's description, or `(lease <name>)`                                   |

Grant problems, such as a task that lists a key fnox does not have, print to
stderr as warnings with a suggestion; the exit code stays 0.

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

- `env` is `true`, `"exec"`, `false`, or `null` for lease keys.
- `scopes` is `["run", "exec"]` for keys that tasks and `mise x` can receive,
  `["run"]` for file keys (tasks only), and `[]` for keys fnox never injects.
- `problems` lists grants that name an unknown or non-injectable key, each with
  `task`, `key`, `kind` and an optional `suggestion`.
- `profile` is the profile fnox used, including `FNOX_PROFILE` or `default` when
  `mise.toml` sets none.

## Migrating from mise-env-fnox {#migrating}

`mise-env-fnox` and `_.fnox-env` put every fnox secret into mise's environment,
where every task, hook, shim and `mise env` sees it. To move to mise secrets:

1. Remove `_.fnox-env` and the `mise-env-fnox` plugin from `mise.toml`.
2. Add `[secrets.fnox]`, and `profile` if you used one.
3. Add `secrets = [...]` to each task that needs a key, listing only that key.
   Read it from the environment in the script (`$DEPLOY_KEY`).
4. Check the result with `mise secrets ls` and `mise tasks validate`.

Values no longer appear in your shell or in `mise env`, which is the point. If a
tool you run outside `mise run` needs a secret, use
`mise x --secrets KEY -- <command>` or `fnox exec -- <command>`. `mise doctor`
flags the plugin as deprecated.
