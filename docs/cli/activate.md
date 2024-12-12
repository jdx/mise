# `mise activate`

- **Usage**: `mise activate [FLAGS] [SHELL_TYPE]`
- **Source code**: [`src/cli/activate.rs`](https://github.com/jdx/mise/blob/main/src/cli/activate.rs)

Initializes mise in the current shell session

This should go into your shell's rc file or login shell.
Otherwise, it will only take effect in the current session.
(e.g. ~/.zshrc, ~/.zprofile, ~/.zshenv, ~/.bashrc, ~/.bash_profile, ~/.profile, ~/.config/fish/config.fish)

Typically, this can be added with something like the following:

    echo 'eval "$(mise activate zsh)"' >> ~/.zshrc

However, this requires that "mise" is in your PATH. If it is not, you need to
specify the full path like this:

    echo 'eval "$(/path/to/mise activate zsh)"' >> ~/.zshrc

Customize status output with `status` settings.

## Arguments

### `[SHELL_TYPE]`

Shell type to generate the script for

**Choices:**

- `bash`
- `elvish`
- `fish`
- `nu`
- `xonsh`
- `zsh`
- `pwsh`

## Flags

### `--shims`

Use shims instead of modifying PATH
Effectively the same as:

    PATH="$HOME/.local/share/mise/shims:$PATH"

### `-q --quiet`

Suppress non-error messages

### `--no-hook-env`

Do not automatically call hook-env

This can be helpful for debugging mise. If you run `eval "$(mise activate --no-hook-env)"`, then you can call `mise hook-env` manually which will output the env vars to stdout without actually modifying the environment. That way you can do things like `mise hook-env --trace` to get more information or just see the values that hook-env is outputting.

Examples:

    eval "$(mise activate bash)"
    eval "$(mise activate zsh)"
    mise activate fish | source
    execx($(mise activate xonsh))
