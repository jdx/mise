# `mise prune [flags] [PLUGIN]...`

Delete unused versions of tools

mise tracks which config files have been used in ~/.local/share/mise/tracked_config_files
Versions which are no longer the latest specified in any of those configs are deleted.
Versions installed only with environment variables `MISE_<PLUGIN>_VERSION` will be deleted,
as will versions only referenced on the command line `mise exec <PLUGIN>@<VERSION>`.

## Arguments

### `[PLUGIN]...`

Prune only versions from this plugin(s)

## Flags

### `-n --dry-run`

Do not actually delete anything

### `--configs`

Prune only tracked and trusted configuration links that point to non-existent configurations

### `--tools`

Prune only unused versions of tools

Examples:

    $ mise prune --dry-run
    rm -rf ~/.local/share/mise/versions/node/20.0.0
    rm -rf ~/.local/share/mise/versions/node/20.0.1
