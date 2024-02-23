# `mise en`

- **Usage**: `mise en [-s --shell <SHELL>] [DIR]`
- **Source code**: [`src/cli/en.rs`](https://github.com/jdx/mise/blob/main/src/cli/en.rs)

[experimental] starts a new shell with the mise environment built from the current configuration

This is an alternative to `mise activate` that allows you to explicitly start a mise session.
It will have the tools and environment variables in the configs loaded.
Note that changing directories will not update the mise environment.

It's a lightweight alternative to `mise activate` if you don't want to put that into your shell rc but
still don't want to deal with shims. It probably only makes sense for interactive use-cases.

It's sort of akin to manually running `source .venv/bin/activate` for Python virtualenvs if you're
familiar with that workflow.

## Arguments

### `[DIR]`

Directory to start the shell in

**Default:** `.`

## Flags

### `-s --shell <SHELL>`

Shell to start

Defaults to $SHELL

Examples:

    $ mise en .
    $ node -v
    v20.0.0

    Skip loading bashrc:
    $ mise en -s "bash --norc"

    Skip loading zshrc:
    $ mise en -s "zsh -f"
