## `mise latest [OPTIONS] <TOOL@VERSION>`

```text
Gets the latest available version for a plugin

Usage: latest [OPTIONS] <TOOL@VERSION>

Arguments:
  <TOOL@VERSION>
          Tool to get the latest version of

Options:
  -i, --installed
          Show latest installed instead of available version

Examples:

    $ mise latest node@20  # get the latest version of node 20
    20.0.0

    $ mise latest node     # get the latest stable version of node
    20.0.0
```
