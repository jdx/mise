## `mise plugins update [OPTIONS] [PLUGIN]...`

**Aliases:** `up, upgrade`

```text
Updates a plugin to the latest version

note: this updates the plugin itself, not the runtime versions

Usage: plugins update [OPTIONS] [PLUGIN]...

Arguments:
  [PLUGIN]...
          Plugin(s) to update

Options:
  -j, --jobs <JOBS>
          Number of jobs to run in parallel
          Default: 4

Examples:

    $ mise plugins update            # update all plugins
    $ mise plugins update node       # update only node
    $ mise plugins update node#beta  # specify a ref
```
