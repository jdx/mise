# Hooks <Badge type="warning" text="experimental" />

You can have mise automatically execute scripts when it runs. The configuration goes into `mise.toml`.

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

## Leave hook (not yet implemented)

This hook is run when the project is left. Changing directories while in the project will not trigger this hook.

```toml
[hooks]
leave = "echo 'I left the project'"
```

## Preinstall/postinstall hook

These hooks are run before tools are installed. Unlike other hooks, these hooks do not require `mise activate`.

```toml
[hooks]
preinstall = "echo 'I am about to install tools'"
postinstall = "echo 'I just installed tools'"
```

## Watch files hook

While using `mise activate` you can have mise watch files for changes and execute a script when a file changes.

```bash
[[watch_files]]
patterns = ["src/**/*.rs"]
script = "cargo fmt"
```

This hook will have the following environment variables set:

- `MISE_WATCH_FILES_MODIFIED`: A colon-separated list of the files that have been modified. Colons are escaped with a backslash.

## Hook execution

Hooks are executed with the following environment variables set:

- `MISE_ORIGINAL_CWD`: The directory that the user is in.
- `MISE_PROJECT_DIR`: The root directory of the project.
- `MISE_PREVIOUS_DIR`: The directory that the user was in before the directory change (only if a directory change occurred).

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

The leave hook (when it's implemented) will give you a way to manually reset the state.
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
