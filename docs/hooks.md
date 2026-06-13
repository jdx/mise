# Hooks

You can have mise automatically execute scripts during a `mise activate` session. You cannot use these
without the `mise activate` shell hook installed in your shell—except the `preinstall` and `postinstall` hooks.
The configuration goes into `mise.toml`.

## CD hook

This hook is run anytime the directory is changed.

```toml
[hooks]
cd = "echo 'I changed directories'"
```

## Enter hook

This hook is run when the project is entered. Changing directories while in the project will not trigger this hook again.

```toml
[hooks]
enter = "echo 'I entered the project'"
```

## Leave hook

This hook is run when the project is left. Changing directories while in the project will not trigger this hook.

```toml
[hooks]
leave = "echo 'I left the project'"
```

## Preinstall/postinstall hook

These hooks are run before and after tools are installed (respectively). Unlike other hooks, these hooks do not require `mise activate`.

```toml
[hooks]
preinstall = "echo 'I am about to install tools'"
postinstall = "echo 'I just installed tools'"
```

String hooks are shorthand for `run` hooks. Use a hook table when you need to select the inline shell command:

```toml
[hooks]
postinstall = { run = "echo 'installed'", shell = "bash -c" }
```

Like tasks, inline hook tables may define a Windows-specific command with `run_windows`.
On Windows, mise uses `run_windows` when it is set; otherwise it uses `run`. On other
platforms, a hook with only `run_windows` is skipped.

```toml
[hooks]
postinstall = { run = "echo installed", run_windows = "Write-Output installed" }
```

For `preinstall` and `postinstall`, `script = ...` is a legacy alias for `run = ...`. If a `shell` is also set on a `script`/`scripts` hook, mise warns that the shell is ignored and still runs the script with the default inline shell. Use `run = ...` with `shell = "bash -c"` to choose the inline shell command. The `script` alias for install hooks is deprecated.

The `postinstall` hook receives a `MISE_INSTALLED_TOOLS` environment variable containing a JSON array of the tools that were just installed:

```toml
[hooks]
postinstall = '''
echo "Installed: $MISE_INSTALLED_TOOLS"
# Example output: [{"name":"node","version":"20.10.0"},{"name":"python","version":"3.12.0"}]
'''
```

## Tool-level postinstall

Individual tools can define their own postinstall scripts using the `postinstall` option. These run immediately after each tool is installed (before other tools in the same session are installed):

```toml
[tools]
node = { version = "20", postinstall = "npm install -g pnpm" }
python = { version = "3.12", postinstall = "pip install pipx" }
```

Tool-level postinstall scripts receive the following environment variables:

- `MISE_TOOL_NAME`: The short name of the tool (e.g., "node", "python")
- `MISE_TOOL_VERSION`: The version that was installed (e.g., "20.10.0", "3.12.0")
- `MISE_TOOL_INSTALL_PATH`: The path where the tool was installed
- Variables from that tool's `install_env` option

## Task hooks

Instead of inline scripts, hooks can reference mise tasks. The task is executed as a subprocess
via `mise run`, so it reuses the full task system including dependencies, environment variables,
and file-based task definitions.

```toml
[tasks.setup]
run = "echo 'setting up project'"
depends = ["install-deps"]

[hooks]
enter = { task = "setup" }
```

You can mix task references with inline scripts in arrays:

```toml
[hooks]
enter = ["echo 'entering project'", { task = "setup" }]
```

Task hooks work with all hook types (`enter`, `leave`, `cd`, `preinstall`, `postinstall`).

## Watch files hook

While using `mise activate` you can have mise watch files for changes and execute a script or task when a file changes.

```toml
[[watch_files]]
patterns = ["src/**/*.rs"]
run = "cargo fmt"
```

By default, `run` uses the configured inline shell:
[`unix_default_inline_shell_args`](/configuration/settings.html#unix_default_inline_shell_args)
or [`windows_default_inline_shell_args`](/configuration/settings.html#windows_default_inline_shell_args).
Add `shell = "bash -c"` to choose a different inline shell command for a watch file hook:

```toml
[[watch_files]]
patterns = ["*.js"]
run = "eslint --fix ."
shell = "bash -c"
```

`shell` only applies to `run` hooks. You can also reference a mise task instead of an inline script:

```toml
[[watch_files]]
patterns = ["uv.lock"]
task = "sync-deps"
```

Each `[[watch_files]]` entry should have either `run` or `task`, but not both.

This hook will have the following environment variables set:

- `MISE_WATCH_FILES_MODIFIED`: A colon-separated list of the files that have been modified. Colons are escaped with a backslash.

## Hook execution

Hooks are executed with the following environment variables set:

- `MISE_ORIGINAL_CWD`: The directory that the user is in.
- `MISE_PROJECT_ROOT`: The root directory of the project.
- `MISE_PREVIOUS_DIR`: The directory that the user was in before the directory change (only if a directory change occurred).
- `MISE_INSTALLED_TOOLS`: A JSON array of tools that were installed (only for `postinstall` hooks).

Inline `run` hooks can be written as `{ run = "..." }` for any hook type. The string shorthand
(`enter = "echo hi"`) is equivalent to `{ run = "echo hi" }`.

`run` and `run_windows` must be strings. `run = ["echo one", "echo two"]` is not supported.

To run separate spawned inline commands, define multiple hooks. Each hook entry is a separate
execution, so mise starts one subprocess per `run` entry:

```toml
[hooks]
enter = [
  { run = "echo one" },
  { run = "echo two" },
]
```

To run multiple shell lines in one spawned command, use one multiline `run` string. This is one hook
execution and one subprocess:

```toml
[hooks.enter]
run = """
echo one
echo two
"""
```

`run` hooks execute in a subprocess using the default inline shell:
[`unix_default_inline_shell_args`](/configuration/settings.html#unix_default_inline_shell_args)
or [`windows_default_inline_shell_args`](/configuration/settings.html#windows_default_inline_shell_args).
Add `shell = "bash -c"` to a `run` hook table to choose a different inline shell command. Like task
`shell`, the value should include both the program and the argument that evaluates the inline command
such as `bash -c`, `zsh -c`, or `pwsh -Command`.

## Shell hooks

`enter`, `leave`, and `cd` hooks can be executed in the current shell, for example if you'd like to add bash completions when entering a directory:

```toml
[hooks.enter]
shell = "bash"
script = "source completions.sh"
```

Current-shell hooks may use `script`/`scripts` arrays:

```toml
[hooks.enter]
shell = "bash"
script = [
  "source completions.sh",
  "export PROJECT_READY=1",
]

[hooks.leave]
shell = "bash"
scripts = [
  "unset PROJECT_READY",
]
```

`script` with `shell` is for current-shell hooks. Here, `shell` is a shell-name selector such as
`bash`, `zsh`, or `fish`, not an inline shell command like `bash -c`. mise only prints the script
when the active `mise activate` shell matches.

Use `run` when the hook should execute as an inline command in a subprocess. `preinstall` and
`postinstall` do not have a current shell, so `script` is only kept there as a legacy alias for `run`;
if `shell` is set with `script`/`scripts` on those hooks, it is ignored.

::: warning
I feel this should be obvious but in case it's not, this isn't going to do any sort of cleanup
when you _leave_ the directory like using `[env]` does in `mise.toml`. You're literally just
executing shell code when you enter the directory which mise has no way to track at all.
I don't think there is a solution to this problem and it's likely the reason direnv has never
implemented something similar.

I think in most situations this is probably fine, though worth keeping in mind.

:::

## Multiple hooks syntax

You can use arrays to define multiple hooks in the same file:

```toml
[hooks]
enter = [
  "echo 'I entered the project'",
  { run = "echo 'I am in the project'" }
]

[[hooks.cd]]
run = "echo 'I changed directories'"
[[hooks.cd]]
run = "echo 'I also changed directories'"
```
