# `mise upgrade [args] [flags]`

Upgrades outdated tool versions

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
