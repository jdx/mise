# Hooks <Badge type="warning" text="experimental" />

You can have mise automatically execute scripts during a `mise activate` session. You cannot use these
without the `mise activate` shell hook installed in your shell—except the `preinstall`, `postinstall`,
`pre_task`, and `post_task` hooks.
The configuration goes into `mise.toml`.

## CD hook

This hook is run anytimes the directory is changed.

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

The `postinstall` hook receives a `MISE_INSTALLED_TOOLS` environment variable containing a JSON array of the tools that were just installed:

```toml
[hooks]
postinstall = '''
echo "Installed: $MISE_INSTALLED_TOOLS"
# Example output: [{"name":"node","version":"20.10.0"},{"name":"python","version":"3.12.0"}]
'''
```

## Pre-task/post-task hooks

These hooks run before and after task execution (respectively). Unlike shell lifecycle hooks, these do not require `mise activate`—they run during `mise run`.

```toml
[hooks]
# Run before every task
pre_task = "echo 'about to run a task'"
# Run after every task
post_task = "echo 'task finished'"
```

### Targeting specific tasks

You can target specific tasks using the `task` field. Glob patterns are supported:

```toml
# Run only before the "deploy" task
[[hooks.pre_task]]
script = "echo 'preparing environment...'"
task = "deploy"

# Run after any task in the "build" namespace
[[hooks.post_task]]
script = "echo 'build step done'"
task = "build:*"
```

### Running a task as a hook

Instead of a shell script, you can use the `run` field to execute another mise task as a hook:

```toml
# Run the "setup" task before every "test" task
[[hooks.pre_task]]
run = "setup"
task = "test"

# Run "notify" after any deploy task
[[hooks.post_task]]
run = "notify"
task = "deploy:*"
```

::: tip
This is especially useful with [remote tasks](/tasks/remote-tasks)—you can define hooks in your local `mise.toml` that run before or after shared remote tasks without modifying the original task definitions.
:::

### Environment variables

Pre-task and post-task hooks are executed with the task's environment and working directory. In addition to the [standard hook environment variables](#hook-execution), the following is also available:

- `MISE_TASK_NAME`: The name of the task being executed.

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

## Watch files hook

While using `mise activate` you can have mise watch files for changes and execute a script when a file changes.

```bash
[[watch_files]]
patterns = ["src/**/*.rs"]
run = "cargo fmt"
```

This hook will have the following environment variables set:

- `MISE_WATCH_FILES_MODIFIED`: A colon-separated list of the files that have been modified. Colons are escaped with a backslash.

## Hook execution

Hooks are executed with the following environment variables set:

- `MISE_ORIGINAL_CWD`: The directory that the user is in.
- `MISE_PROJECT_ROOT`: The root directory of the project.
- `MISE_CONFIG_ROOT`: The root directory of the config file that defined the hook.
- `MISE_PREVIOUS_DIR`: The directory that the user was in before the directory change (only for `enter`/`leave`/`cd` hooks).
- `MISE_INSTALLED_TOOLS`: A JSON array of tools that were installed (only for `postinstall` hooks).
- `MISE_TASK_NAME`: The name of the task being executed (only for `pre_task`/`post_task` hooks).

## Shell hooks

Hooks can be executed in the current shell, for example if you'd like to add bash completions when entering a directory:

```toml
[hooks.enter]
shell = "bash"
script = "source completions.sh"
```

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
  "echo 'I am in the project'"
]

[[hooks.cd]]
script = "echo 'I changed directories'"
[[hooks.cd]]
script = "echo 'I also directories'"
```
