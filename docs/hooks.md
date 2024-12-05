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
