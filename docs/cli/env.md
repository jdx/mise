## `mise env [OPTIONS] [TOOL@VERSION]...`

**Aliases:** `e`

```text
Exports env vars to activate mise a single time

Use this if you don't want to permanently install mise. It's not necessary to
use this if you have `mise activate` in your shell rc file.

Usage: env [OPTIONS] [TOOL@VERSION]...

Arguments:
  [TOOL@VERSION]...
          Tool(s) to use

Options:
  -J, --json
          Output in JSON format

  -s, --shell <SHELL>
          Shell type to generate environment variables for
          
          [possible values: bash, fish, nu, xonsh, zsh]

Examples:

    $ eval "$(mise env -s bash)"
    $ eval "$(mise env -s zsh)"
    $ mise env -s fish | source
    $ execx($(mise env -s xonsh))
```
