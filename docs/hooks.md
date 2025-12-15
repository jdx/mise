# Hooks <Badge type="warning" text="experimental" />

You can have mise automatically execute scripts during a `mise activate` session. You cannot use these
without the `mise activate` shell hook installed in your shell—except the `preinstall` and `postinstall` hooks.
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

These hooks are run before and after each tool is installed (respectively). Unlike other hooks, these hooks do not require `mise activate`.

The hooks run once per tool being installed, with `MISE_TOOL_NAME` and `MISE_TOOL_VERSION` environment variables set to the tool being processed.

```toml
[hooks]
preinstall = "echo 'About to install $MISE_TOOL_NAME@$MISE_TOOL_VERSION'"
postinstall = "echo 'Finished installing $MISE_TOOL_NAME@$MISE_TOOL_VERSION'"
```

You can use these variables to run conditional logic based on the tool:

```toml
[hooks]
postinstall = '''
if [ "$MISE_TOOL_NAME" = "node" ]; then
  echo "Node.js $MISE_TOOL_VERSION installed, running npm setup..."
  npm config set prefix ~/.npm-global
fi
'''
```

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
- `MISE_PREVIOUS_DIR`: The directory that the user was in before the directory change (only if a directory change occurred).

For `preinstall` and `postinstall` hooks, the following additional environment variables are set:

- `MISE_TOOL_NAME`: The short name of the tool being installed (e.g., `node`, `python`, `go`).
- `MISE_TOOL_VERSION`: The version of the tool being installed (e.g., `20.10.0`, `3.12.0`).

For tool-level postinstall hooks (defined on the tool itself), an additional variable is available:

- `MISE_TOOL_INSTALL_PATH`: The installation path of the tool.

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
