---
description: "Set project environment variables in mise.toml, load them from files and scripts, and add directories to PATH."
---

# Environment variables

Set project environment variables under `[env]` in `mise.toml`. mise applies
them to commands run with `mise exec`, to tasks, to shims, and to shells where
mise is activated.

For separate files per deployment, such as `mise.production.toml`, see
[Config environments](/configuration/environments.html). For values that only
templates need, use [`[vars]`](/configuration/vars.html). To keep secret values
out of plaintext config, see [Secrets](/environments/secrets/).

## Set and unset variables {#set-and-unset}

```toml [mise.toml]
[env]
NODE_ENV = "production"
```

Check the value in a child process without changing your shell:

```sh
mise exec -- sh -c 'echo "$NODE_ENV"'
# production
```

A value can be a string or an integer; `true` becomes the string `"true"`. Set a
variable to `false` to unset it, for example one that your shell or a parent
directory's config sets:

```toml [mise.toml]
[env]
NODE_ENV = false
```

[`mise set`](/cli/set.html) and [`mise unset`](/cli/unset.html) edit `[env]`
from the command line:

```sh
mise set NODE_ENV=development   # writes NODE_ENV = "development" under [env]
mise set NODE_ENV               # prints development
mise set                        # lists each variable, its value, and the file it comes from
mise unset NODE_ENV             # removes it
```

`mise set` writes to the nearest directory that has config. When that directory
has both `mise.toml` and `mise.local.toml`, it writes `mise.toml`; see
[which file a write uses](/configuration.html#target-file-for-write-operations).
Pass `-E staging` to write `mise.staging.toml`, `-g` to write the global config,
and `--prompt` or `--stdin` to keep a value out of your shell history.

To print the resolved environment, including `PATH` entries and variables that
tools set, run [`mise env`](/cli/env.html). Add `--json` or `--dotenv` for other
formats.

## Where variables apply {#using-environment-variables}

| How you run a command                                | Receives `[env]`                                                     |
| ---------------------------------------------------- | -------------------------------------------------------------------- |
| `mise exec -- <command>`                             | Yes                                                                  |
| `mise run <task>`                                    | Yes, plus the task's own `env`                                       |
| A shell where [mise is activated](/shell-setup.html) | Yes, updated each time you change directories                        |
| A [shim](/dev-tools/shims.html)                      | Yes, for the program the shim runs                                   |
| [`mise en`](/cli/en.html)                            | Yes, in a new shell that does not update when you change directories |

Variables combine with [tools](/dev-tools/) in the same file:

```sh
mise use node@24
mise set MY_VAR=123
mise exec -- node -p process.env.MY_VAR
# 123
```

### Task-specific variables {#environment-in-tasks}

A task's own `env` applies only to that task and accepts the same values and
directives as `[env]`:

```mise-toml [mise.toml]
[tasks.print]
env = { MY_VAR = "my variable", _.file = ".env.print" }
run = "echo $MY_VAR"
```

See [task `env`](/tasks/task-configuration.html#env).

## Defaults and required variables {#defaults-and-required}

### Defaults {#defaults}

`default` sets a fallback and keeps a value that is already there:

```toml [mise.toml]
[env]
NODE_ENV = { default = "development" }
```

mise keeps `NODE_ENV` when it is already set to a non-empty value, either in the
environment mise started with or by a lower-precedence config file such as the
global config. Otherwise it sets `NODE_ENV` to `development`. A default must be a
string or an integer.

### Required variables {#required-variables}

`required = true` declares a variable that something else must set. mise checks
that it is set but never assigns it. The check passes when the variable is in
the environment mise started with, or when any other config file sets it, such
as `mise.local.toml`, a parent directory's config or the global config:

```toml [mise.toml]
[env]
DATABASE_URL = { required = true }
```

```toml [mise.local.toml]
[env]
DATABASE_URL = "postgres://localhost/app"
```

Give `required` a string instead of `true` to show help text when the variable
is missing:

```toml [mise.toml]
[env]
DATABASE_URL = { required = "Set DATABASE_URL to your PostgreSQL connection string" }
```

When a required variable is missing, `mise exec`, `mise run`, `mise env` and
`mise set` stop with an error that includes the help text. An activated shell
prints a warning instead and keeps going, so a missing value does not break your
prompt:

```text
mise WARN  Required environment variable 'DATABASE_URL' is not defined. It must be set before mise runs or in a later config file. (Required in: ~/src/app/mise.toml)
Help: Set DATABASE_URL to your PostgreSQL connection string
```

`required` cannot be combined with `value` or `default`.

To mask a value in task output, including one a caller supplies for a
`required` variable, mark it with `redact = true`; see
[Redaction and CI masking](/environments/secrets/#redaction).

## Reference other values {#reference-other-values}

Values are [Tera templates](/templates.html), and they also support shell-style
`$VAR` expansion:

```toml [mise.toml]
[env]
PROJECT_LIB = "{{config_root}}/lib"
LD_LIBRARY_PATH = "{{env.PROJECT_LIB}}:${LD_LIBRARY_PATH:-}"
```

A value can use only variables defined above it in the same file, in a
lower-precedence config file, or in the environment mise started with.

### Templates {#templates}

<span v-pre>`{{config_root}}`</span> is the project directory and
<span v-pre>`{{env.NAME}}`</span> reads a variable. See
[Tera templates](/templates.html) for the full context and filters.

### Shell-style expansion {#shell-style-variable-expansion}

| Syntax            | Result                                                           |
| ----------------- | ---------------------------------------------------------------- |
| `$VAR`            | The value of `VAR`                                               |
| `${VAR}`          | The same; use it before letters or digits, as in `${VAR}_suffix` |
| `${VAR:-default}` | `default` when `VAR` is unset or empty                           |
| `${VAR:-}`        | An empty string when `VAR` is unset, without a warning           |

Expansion runs after template rendering, so a value can mix both. A variable
that is not defined and has no default stays as written, and mise prints a
warning. Turn expansion off with
[`env_shell_expand = false`](/configuration/settings.html#env_shell_expand).

## Use values that tools set {#lazy-eval}

mise resolves `[env]` before it loads tools, so values can configure tool
installation, for example `CFLAGS` for a source build. To use something a tool
provides, such as its version or its `PATH` entries, add `tools = true`. mise
then resolves that entry after tools load:

```toml [mise.toml]
[env]
NODE_VERSION = { value = "{{ tools.node.version }}", tools = true }
_.path = { path = "{{env.GEM_HOME}}/bin", tools = true }
```

`tools = true` works on variables, `_.file`, `_.path`, `_.source` and
[plugin directives](#plugin-directives). `_.python.venv` always resolves after
tools and does not accept the option.

## Precedence {#precedence}

When several config files set the same variable, the more specific file wins:

- `mise.local.toml` wins over `mise.toml`.
- With `MISE_ENV=dev`, `mise.dev.toml` wins over `mise.toml`, `mise.local.toml`
  wins over `mise.dev.toml`, and `mise.dev.local.toml` wins over all three.
- A project's config wins over a parent directory's config.
- Any project config wins over the global config
  (`~/.config/mise/config.toml`).

See [config files](/configuration.html#mise-toml) for the full list of
locations.

mise evaluates config files from the lowest precedence to the highest, and the
entries in each file in the order you write them, so each value can reference
anything set before it. A value in `[env]` replaces a variable of the same name
in the environment mise started with; `default` and `required` are the
exceptions described above.

## Env directives {#env-directives}

Directives load variables from files and scripts and add `PATH` entries. They
live in a table named `_`, because a variable cannot contain a nested table.
`_.file`, `_.path` and `_.source` each take a path, a table with `path` and
options, or an array of either:

```toml [mise.toml]
[env]
_.file = ".env"
_.path = ["bin", "node_modules/.bin"]
_.source = { path = "scripts/env.sh", redact = true }
```

Relative paths resolve against the project directory, even when the config file
is in `.config/mise/` or `.mise/` (see
[`config_root`](/configuration.html#config-root)). Paths can use templates,
`$VAR` and `~`. Directives run in the order you write them, so `_.path` can use
a variable that `_.source` set above it; `_.python.venv` always runs last.

| Option   | `_.file` | `_.source` | `_.path` | Effect                                                                   |
| -------- | -------- | ---------- | -------- | ------------------------------------------------------------------------ |
| `path`   | Yes      | Yes        | Yes      | One path or a list of paths                                              |
| `tools`  | Yes      | Yes        | Yes      | Resolve after tools load ([details](#lazy-eval))                         |
| `redact` | Yes      | Yes        | No       | Mark every loaded value as [sensitive](/environments/secrets/#redaction) |
| `expand` | Yes      | No         | No       | Let the file use values loaded before it                                 |

### `env._.file` {#env-file}

Load variables from a file:

```toml [mise.toml]
[env]
_.file = ".env"
```

- The extension selects the format. `.json`, `.yaml` and `.toml` files are
  parsed as structured files whose top-level keys become variables. Any other
  name, including `.yml` and `.env`, is parsed as dotenv.
- The path can be a glob pattern such as `.env.*`; every matching file is
  loaded.
- A path that matches no file is skipped without an error, so `_.file` can
  point at an optional, untracked `.env`.
- Values from the file replace variables already in the environment.

```toml [mise.toml]
[env]
_.file = [
  ".env.json",
  "~/.config/myapp/.env",
  { path = ".secrets.yaml", redact = true },
]
```

Dotenv files use `KEY=value` lines, with `#` comments and quoted values.
Quoting works as in a shell: single-quoted text is literal, and quoted and
unquoted parts next to each other are joined with their quotes removed, so
`'it'\''s'` is `it's`. A quote with no partner later on its line, as in
`O'Brien`, is kept as written. A partner in a trailing comment still counts, so
write `NAME="O'Brien" # owner's name`. Single-quote a value that must keep its
own quotes, such as JSON: `CONFIG='{"debug": true}'`. A dotenv value can
reference variables assigned earlier in the same file, then variables from the
environment mise started with. JSON, YAML and TOML values are read literally, so a `$` stays a `$`. Set `expand = true` to let a file of
any format reference values loaded before it, from earlier files or earlier
`[env]` entries:

```toml [mise.toml]
[env]
BASE = "/opt/project"
_.file = { path = ".env.json", expand = true }
```

`expand = true` has no effect when
[`env_shell_expand`](/configuration/settings.html#env_shell_expand) is `false`.

mise decrypts SOPS-encrypted JSON, YAML and TOML files as it loads them; see
[SOPS files](/environments/secrets/sops.html).

To load a dotenv file from the current directory and every parent directory, set
the [`env_file`](/configuration/settings.html#env_file) setting, for example
`MISE_ENV_FILE=.env`. Unlike `_.file`, which resolves paths against the config
file that declares it, `env_file` searches upward from the current directory,
and a closer file wins.

### `env._.path` {#env-path}

Add directories to `PATH`, ahead of the directories already in it:

```toml [mise.toml]
[env]
_.path = [
  "bin",
  "{{config_root}}/node_modules/.bin",
  "~/.local/share/mytool/bin",
]
```

Use `_.path` for `PATH`. A `PATH` key in `[env]` is ignored by `mise exec`,
`mise env` and activated shells.

### `env._.source` {#env-source}

Run a bash script and take the variables it exports:

```toml [mise.toml]
[env]
_.source = "scripts/env.sh"
```

mise runs the script with bash, as if you ran `source scripts/env.sh`, and
ignores its shebang. For another language, use a task or an
[environment plugin](/env-plugin-development.html).

- Variables the script exports or changes are added, and variables it unsets
  are removed.
- The path can be a glob pattern, and a path that matches no file is skipped,
  as with `_.file`.
- The script runs each time mise computes the environment, so keep it fast and
  safe to run more than once.
- [Safe mode](/security.html#safe-mode) never runs it, not even from the global
  config.

A script can prepend to `PATH`:

```sh
export PATH="/new/bin:$PATH"
```

mise ignores other `PATH` changes, such as appending, removing or reordering
entries, because it manages `PATH` entries itself so it can remove them cleanly
when you leave the directory. Relative prepended entries resolve against the
project directory, and empty entries are ignored.

On Windows, `_.source` needs a POSIX bash such as
[Git for Windows](https://gitforwindows.org/) or MSYS2. mise finds it the same
way it does for bash tasks, including common install locations that are not on
`PATH`; set `MISE_BASH_PATH` to choose one. mise never uses the WSL launcher at
`C:\Windows\System32\bash.exe`, because WSL cannot read Windows script paths.
`PATH` entries the script prepends in `/c/...` or `/cygdrive/c/...` form are
converted to Windows form.

### `env._.python.venv` {#env-python-venv}

Create and activate a Python virtual environment:

```toml [mise.toml]
[env]
_.python.venv = { path = ".venv", create = true }
```

See [Automatic virtualenv activation](/lang/python.html#automatic-virtualenv-activation)
for all options.

### Directives from plugins {#plugin-directives}

An environment plugin adds its own directive.
[Install the plugin](/plugins.html#environment-plugins), then name it under `_`
with the options the plugin documents:

```toml [mise.toml]
[env]
_.my-env-plugin = { api_url = "https://api.example.com" }
```

`tools` and `redact` also work on plugin directives. If the plugin is not
installed and mise has no source to install it from, mise warns and skips the
directive. To write an environment plugin, see
[Environment plugins](/env-plugin-development.html).

## Variables that configure mise {#mise-variables}

mise reads `MISE_*` variables such as `MISE_DATA_DIR` when it starts, before it
loads `[env]`, so setting one in `[env]` does not change how that mise process
behaves. Set them in your shell or CI environment, or use
[`[settings]`](/configuration/settings.html). See
[`MISE_*` variables](/configuration/environment-variables.html).

The SOPS key variables are the exception: mise reads `MISE_SOPS_AGE_KEY` and
`MISE_SOPS_AGE_KEY_FILE` from `[env]` to decrypt encrypted files listed after
them; see [SOPS files](/environments/secrets/sops.html#environment-variables).

## Turn off config env {#no-env}

`mise --no-env`, `MISE_NO_ENV=1` or the
[`no_env`](/configuration/settings.html#no_env) setting skips `[env]` and env
directives from every config file.

In [safe mode](/security.html#safe-mode) (`MISE_SAFE=1`), mise ignores `[env]`
and env directives from project config, and never runs `_.source`.

## Next steps {#next-steps}

- [Secrets](/environments/secrets/): encrypt values or fetch them from a secret
  manager, and mask them in output.
- [Config environments](/configuration/environments.html): switch sets of values
  with `MISE_ENV`.
- [Hooks](/hooks.html): run commands when you enter or leave a project.
- [Tera templates](/templates.html): the functions and filters values can use.

## Deprecated syntax {#deprecated-syntax}

| Deprecated                                                      | Use instead  | Removed in |
| --------------------------------------------------------------- | ------------ | ---------- |
| `env.mise.*`                                                    | `env._.*`    | 2026.12.0  |
| `value` or `values` in a `_.file`, `_.path` or `_.source` table | `path`       | 2026.12.0  |
| Top-level `env_file` or `dotenv`                                | `env._.file` | 2027.4.0   |
| Top-level `env_path`                                            | `env._.path` | 2027.4.0   |

The `value` and `values` rule also applies to directives under `vars._`. It does
not affect `value` in an ordinary variable table, such as
`SECRET = { value = "...", redact = true }`, or the options of plugin
directives.
