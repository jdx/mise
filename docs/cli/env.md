# `mise env`

**Usage**: `mise env [-J --json] [-s --shell <SHELL>] [TOOL@VERSION]...`

**Aliases**: `e`

Exports env vars to activate mise a single time

Use this if you don't want to permanently install mise. It's not necessary to
use this if you have `mise activate` in your shell rc file.

## Arguments

### `[TOOL@VERSION]...`

Tool(s) to use

## Flags

### `-J --json`

Output in JSON format

### `-s --shell <SHELL>`

Shell type to generate environment variables for

**Choices:**

- `bash`
- `fish`
- `nu`
- `xonsh`
- `zsh`

Examples:

    eval "$(mise env -s bash)"
    eval "$(mise env -s zsh)"
    mise env -s fish | source
    execx($(mise env -s xonsh))
