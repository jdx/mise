---
description: "Run commands when an activated shell changes directory or sees a watched file change, or when mise installs tools."
---

# Hooks

Hooks run commands when something happens: a shell with `mise activate` changes
directory (including entering or leaving a project) or detects a change to a
watched file, or mise installs tools. Define them under `[hooks]` in
`mise.toml`; file-change hooks use a separate `[[watch_files]]` table.

```toml [mise.toml]
[hooks]
enter = "echo 'entered the project'"
postinstall = "npm install"
```

| Event             | Runs when                                                | Needs `mise activate` |
| ----------------- | -------------------------------------------------------- | --------------------- |
| `enter`           | The shell moves into the project from outside it         | Yes                   |
| `leave`           | The shell moves out of the project                       | Yes                   |
| `cd`              | The shell changes to a directory inside the project      | Yes                   |
| `preinstall`      | mise is about to install tools                           | No                    |
| `postinstall`     | mise has installed tools                                 | No                    |
| `[[watch_files]]` | The shell sees a change to a file that matches a pattern | Yes                   |

Use [tasks](/tasks/) for commands you run on demand, and
[`mise watch`](/cli/watch.html) to rerun a task while you edit. `watch_files`
hooks are checked when the activated shell shows a prompt, not by a background
watcher.

## Define a hook {#define-a-hook}

A hook is a command string, a table, a task reference, or an array of these:

```mise-toml [mise.toml]
[hooks]
enter = "echo hi"                                 # short for { run = "echo hi" }
leave = { run = "echo bye", shell = "bash -c" }
cd = { task = "check-tools" }
postinstall = ["npm install", { task = "codegen" }]
```

- `run` runs the command in a new process with your default inline shell,
  [`unix_default_inline_shell_args`](/configuration/settings.html#unix_default_inline_shell_args)
  or
  [`windows_default_inline_shell_args`](/configuration/settings.html#windows_default_inline_shell_args).
  Set `shell` to use another program, giving both the program and the argument
  that runs an inline command, such as `bash -c`, `zsh -c` or `pwsh -Command`.
- `run_windows` replaces `run` on Windows. On other platforms, a hook with only
  `run_windows` is skipped.
- `task` runs a mise task; see [Run a task](#task-hooks).

Hook commands are [Tera templates](/templates.html), so
<span v-pre>`enter = "echo {{config_root}}"`</span> prints the project
directory. A spawned hook runs with the project's environment, `[env]` values
and tools on `PATH`, and its output goes to stderr. When a hook fails, mise
prints a warning and runs the remaining hooks.

`run` must be a single string. To run several lines in one process, use a
multiline string:

```mise-toml [mise.toml]
[hooks.enter]
run = """
echo one
echo two
"""
```

To run them as separate processes, define several hooks with an array or an
array of tables:

```mise-toml [mise.toml]
[[hooks.cd]]
run = "echo 'I changed directories'"

[[hooks.cd]]
run = "echo 'I also changed directories'"
```

### Run a task {#task-hooks}

A hook can run a mise task instead of an inline command:

```mise-toml [mise.toml]
[tasks.install-deps]
run = "echo 'install project dependencies here'"

[tasks.setup]
run = "echo 'setting up project'"
depends = ["install-deps"]

[hooks]
enter = { task = "setup" }
```

mise runs the task with `mise run` in a subprocess, so dependencies, the task's
environment and file tasks work as usual. Task hooks work with every event.

As a `preinstall` hook, a task runs without installing its missing tools first,
because the install it prepares has not happened yet. The commands it needs must
already be available from the system or an earlier installation.

A task that lists `secrets` does not run from a hook; see
[fnox](/environments/secrets/fnox.html#launchers-that-refuse-grants).

## Directory hooks {#directory-hooks}

With [mise activated](/shell-setup.html), mise runs these hooks when the shell's
directory changes, including at the first prompt after activation:

```toml [mise.toml]
[hooks]
enter = "echo 'I entered the project'"
leave = "echo 'I left the project'"
cd = "echo 'I changed directories'"
```

- `enter` runs when you move into the project from outside it. Moving between
  directories inside the project does not run it again.
- `leave` runs when you move out of the project.
- `cd` runs on every directory change that ends inside the project, including
  the one that enters it.

The project is the root of the config file that defines the hook: the
directory that holds `mise.toml`, or the parent of `.config/mise/` or `.mise/`.
When one directory change triggers several of these hooks, `leave` runs first,
then `cd`, then `enter`. `enter` also runs when a project config is loaded for
the first time without a directory change, for example after you create or
trust `mise.toml` in the current directory.

Hooks in the global config (`~/.config/mise/config.toml`) have no project, so
global `enter`, `leave` and `cd` hooks run on every directory change.

## Run in the current shell {#shell-hooks}

`enter`, `leave` and `cd` hooks can run in your current shell instead of a new
process, for example to load completions or set a shell option. `shell` means a
different thing in each form:

| Form                                 | Where it runs                                        |
| ------------------------------------ | ---------------------------------------------------- |
| `{ run = "...", shell = "bash -c" }` | A new process started with `bash -c`                 |
| `{ script = "...", shell = "bash" }` | Your current shell, and only when that shell is bash |

With `script`, `shell` names a shell, such as `bash`, `zsh` or `fish`. mise adds
the script to the code the activated shell evaluates at the prompt, and skips
the hook when the active shell is a different one:

```toml [mise.toml]
[hooks.enter]
shell = "bash"
script = "source completions.sh"
```

`script` takes a string or an array of lines. `scripts` takes only the array
form:

```toml [mise.toml]
[hooks.enter]
shell = "bash"
script = [
  "source completions.sh",
  "export PROJECT_READY=1",
]

[hooks.leave]
shell = "bash"
scripts = ["unset PROJECT_READY"]
```

mise does not track or undo what a script changes. An `enter` script that
exports a variable needs a `leave` script that unsets it. To let mise manage a
value's lifetime, use [`[env]`](/environments/) instead.

## Install hooks {#preinstall-postinstall-hook}

`preinstall` and `postinstall` run when mise installs tools, for example during
`mise install`, `mise use`, or an automatic install before `mise exec`. They do
not need `mise activate`:

```toml [mise.toml]
[hooks]
preinstall = "echo 'about to install tools'"
postinstall = "echo 'installed tools'"
```

- They run with the project root as the working directory, even when you run
  `mise install` from a subdirectory. `MISE_ORIGINAL_CWD` holds the directory
  you ran it from.
- A project's install hooks run only when the current directory is inside that
  project.
- `mise install` runs `postinstall` even when every tool is already installed;
  [`MISE_INSTALLED_TOOLS`](#mise-installed-tools) is then `[]`.
- `preinstall` sees the environment without `[env]` entries that set
  [`tools = true`](/environments/#lazy-eval), because those tools are not
  installed yet.
- `mise install --dry-run` prints the install hooks it would run without running
  them.
- `postinstall` can run before the command that started the install finishes
  its own work, such as writing config. Do not use it to restart services after
  `mise upgrade`; run that step after the upgrade command instead.

To run a command after one specific tool installs, use that tool's
[`postinstall` option](/dev-tools/#tool-postinstall-commands). It runs as soon
as that tool finishes, while `[hooks].postinstall` runs once for the whole
install.

## Watch files {#watch-files-hook}

With mise activated, a `[[watch_files]]` entry runs a command or task when a
file that matches one of its patterns changes:

```mise-toml [mise.toml]
[[watch_files]]
patterns = ["src/**/*.rs"]
run = "cargo fmt"
```

- Patterns are globs relative to the project root. `*` does not match `/`; use
  `**` to cross directories.
- `run` uses your default inline shell. Add `shell = "bash -c"` to choose
  another one; `shell` applies only to `run`.
- `task = "sync-deps"` runs a mise task instead. Set either `run` or `task`;
  when both are set, mise warns and runs the task.
- mise reads `[[watch_files]]` from project config only. Entries in the global
  config are ignored.

```toml [mise.toml]
[[watch_files]]
patterns = ["uv.lock"]
task = "sync-deps"
```

## Environment variables {#hook-execution}

| Variable                    | Set for                                                     | Value                                                                       |
| --------------------------- | ----------------------------------------------------------- | --------------------------------------------------------------------------- |
| `MISE_PROJECT_ROOT`         | All hooks and `watch_files`                                 | The project root                                                            |
| `MISE_CONFIG_ROOT`          | `[hooks]`                                                   | The root of the config file that defines the hook                           |
| `MISE_ORIGINAL_CWD`         | All hooks and `watch_files`                                 | The directory you were in when mise ran                                     |
| `MISE_PREVIOUS_DIR`         | `enter`, `leave`, `cd`                                      | The directory before the change                                             |
| `MISE_INSTALLED_TOOLS`      | `postinstall`                                               | A JSON array of the installed tools; see [below](#mise-installed-tools)     |
| `MISE_WATCH_FILES_MODIFIED` | `watch_files`                                               | The changed files, separated by `:`, with any `:` in a name escaped as `\:` |
| `MISE_NO_HOOKS`             | Spawned `[hooks]` commands, task hooks, `watch_files` tasks | `1`, so a `mise` command run from the hook does not run hooks again         |

Global hooks get the active project's root as `MISE_PROJECT_ROOT`, or the global
config root when no project is active, and the global config root as
`MISE_CONFIG_ROOT`. For operations on the global config only, such as
`mise use --global`, both variables are the global config root and project
hooks do not run.

## Hooks from several config files {#several-config-files}

When the same event is defined in several loaded config files, mise runs every
matching hook rather than letting one file override another. Hooks run from the
highest-precedence config file to the lowest. Within one file, an array of hooks
runs in the order listed.

For example, hooks in `conf.d/a.toml`, `conf.d/b.toml` and `conf.d/c.toml` run
as `c`, `b`, then `a`, because later fragments in alphabetical order have higher
precedence. Put hooks that depend on each other's order in one array.

## Turn hooks off {#disable-hooks}

Pass `--no-hooks`, set `MISE_NO_HOOKS=1`, or set
[`no_hooks = true`](/configuration/settings.html#no_hooks) to skip the
`[hooks]` entries: `enter`, `leave`, `cd`, `preinstall` and `postinstall`.
[Safe mode](/security.html#safe-mode) (`MISE_SAFE=1`) skips them too.

The flag, the variable and the setting do not affect `[[watch_files]]` entries
or a tool's own `postinstall` option.

## `MISE_INSTALLED_TOOLS` {#mise-installed-tools}

`postinstall` hooks receive `MISE_INSTALLED_TOOLS`, a JSON array with one entry
per tool that mise installed:

```toml [mise.toml]
[hooks]
postinstall = '''
echo "Installed: $MISE_INSTALLED_TOOLS"
'''
```

```text
Installed: [{"name":"node","version":"24.4.1","requested_version":"24","backend":"core:node","install_path":"/home/user/.local/share/mise/installs/node/24.4.1"}]
```

| Field               | Meaning                                                                                 | Example                                    |
| ------------------- | --------------------------------------------------------------------------------------- | ------------------------------------------ |
| `name`              | The tool's short name                                                                   | `node`                                     |
| `version`           | The version that was installed                                                          | `24.4.1`                                   |
| `requested_version` | What was asked for, before resolution; the same as `version` for a fully pinned request | `latest`, `24`, `lts`, `ref:main`          |
| `backend`           | The backend that installed it, without options or URL credentials                       | `core:node`, `npm:prettier`                |
| `install_path`      | The directory of this installation, never a floating link such as `latest`              | `~/.local/share/mise/installs/node/24.4.1` |

`requested_version` is normally the string from the config or the command line.
Ref selectors are normalized to their `:` form, so `ref-main` is reported as
`ref:main`. A hook can branch on it without reading config files, which a
`postinstall` hook cannot reliably do for the install that just finished:

```toml [mise.toml]
[hooks]
postinstall = '''
echo "$MISE_INSTALLED_TOOLS" | jq -r '
  .[] | select(.requested_version == "latest") | "\(.name) floats on latest, got \(.version)"'
'''
```

To act only on real installs, check for `[]` first. When some installs fail,
the array lists the tools that did install, and mise reports the failure after
the hook runs.

## Deprecated syntax {#deprecated-syntax}

A `script` or `scripts` table runs as a spawned command, the same as `run`, on
`preinstall` and `postinstall`, and on `enter`, `leave` or `cd` when no `shell`
is set. That form is deprecated: use `run`. It will be removed in mise
2027.3.0.

On `preinstall` and `postinstall`, a `shell` set next to `script` or `scripts`
is ignored, and mise warns. `script` or `scripts` with `shell` (for example
`shell = "bash"`) on `enter`, `leave` or `cd` is the
[current-shell form](#shell-hooks) and is not deprecated.
