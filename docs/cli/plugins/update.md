# `mise plugins update`

**Usage**: `mise plugins update [-j --jobs <JOBS>] [PLUGIN]...`

**Aliases**: up, upgrade

Updates a plugin to the latest version

note: this updates the plugin itself, not the runtime versions

## Arguments

### `[PLUGIN]...`

Plugin(s) to update

## Flags

### `-j --jobs <JOBS>`

Number of jobs to run in parallel
Default: 4

Examples:

    mise plugins update            # update all plugins
    mise plugins update node       # update only node
    mise plugins update node#beta  # specify a ref
