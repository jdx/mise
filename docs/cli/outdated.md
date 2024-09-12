## `mise outdated [OPTIONS] [TOOL@VERSION]...`

```text
Shows outdated tool versions

Usage: outdated [OPTIONS] [TOOL@VERSION]...

Arguments:
  [TOOL@VERSION]...
          Tool(s) to show outdated versions for
          e.g.: node@20 python@3.10
          If not specified, all tools in global and local configs will be shown

Options:
  -J, --json
          Output in JSON format

Examples:

    $ mise outdated
    Plugin  Requested  Current  Latest
    python  3.11       3.11.0   3.11.1
    node    20         20.0.0   20.1.0

    $ mise outdated node
    Plugin  Requested  Current  Latest
    node    20         20.0.0   20.1.0

    $ mise outdated --json
    {"python": {"requested": "3.11", "current": "3.11.0", "latest": "3.11.1"}, ...}
```
