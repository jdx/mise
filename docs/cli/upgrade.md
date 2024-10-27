# `mise upgrade`

**Usage**: `mise upgrade [FLAGS] [TOOL@VERSION]...`

**Aliases**: `up`

Upgrades outdated tools

By default, this keeps the range specified in mise.toml. So if you have node@20 set, it will
upgrade to the latest 20.x.x version available. See the `--bump` flag to use the latest version
and bump the version in mise.toml.

This will update mise.lock if it is enabled, see <https://mise.jdx.dev/configuration/settings.html#lockfile>

## Arguments

### `[TOOL@VERSION]...`

Tool(s) to upgrade
e.g.: node@20 python@3.10
If not specified, all current tools will be upgraded

## Flags

### `-n --dry-run`

Just print what would be done, don't actually do it

### `-i --interactive`

Display multiselect menu to choose which tools to upgrade

### `-j --jobs <JOBS>`

Number of jobs to run in parallel
[default: 4]

### `-l --bump`

Upgrades to the latest version available, bumping the version in mise.toml

For example, if you have `node = "20.0.0"` in your mise.toml but 22.1.0 is the latest available,
this will install 22.1.0 and set `node = "22.1.0"` in your config.

It keeps the same precision as what was there before, so if you instead had `node = "20"`, it
would change your config to `node = "22"`.

### `--raw`

Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1

Examples:

    # Upgrades node to the latest version matching the range in mise.toml
    $ mise upgrade node

    # Upgrades node to the latest version and bumps the version in mise.toml
    $ mise upgrade node --bump

    # Upgrades all tools to the latest versions
    $ mise upgrade

    # Upgrades all tools to the latest versions and bumps the version in mise.toml
    $ mise upgrade --bump

    # Just print what would be done, don't actually do it
    $ mise upgrade --dry-run

    # Upgrades node and python to the latest versions
    $ mise upgrade node python

    # Show a multiselect menu to choose which tools to upgrade
    $ mise upgrade --interactive
