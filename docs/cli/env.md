# `mise env`

- **Usage**: `mise env [FLAGS] [TOOL@VERSION]...`
- **Aliases**: `e`
- **Source code**: [`src/cli/env.rs`](https://github.com/jdx/mise/blob/main/src/cli/env.rs)

Exports env vars to activate mise a single time

Use this if you don't want to permanently install mise. It's not necessary to
use this if you have `mise activate` in your shell rc file.

## Arguments

### `[TOOL@VERSION]...`

Tool(s) to use

## Flags

### `-J --json`

Output in JSON format

### `--json-extended`

Output in JSON format with additional information (source, tool)

### `-D --dotenv`

Output in dotenv format

### `-s --shell <SHELL>`

Shell type to generate environment variables for

**Choices:**

- `bash`
- `elvish`
- `fish`
- `nu`
- `xonsh`
- `zsh`
- `pwsh`

Examples:

    eval "$(mise env -s bash)"
    eval "$(mise env -s zsh)"
    mise env -s fish | source
    execx($(mise env -s xonsh))
