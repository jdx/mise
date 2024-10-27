# `mise use`

**Usage**: `mise use [FLAGS] [TOOL@VERSION]...`

**Source code**: [`src/cli/use.rs`](https://github.com/jdx/mise/blob/main/src/cli/use.rs)

**Aliases**: `u`

Installs a tool and adds the version it to mise.toml.

This will install the tool version if it is not already installed.
By default, this will use a `mise.toml` file in the current directory.

Use the `--global` flag to use the global config file instead.

## Arguments

### `[TOOL@VERSION]...`

Tool(s) to add to config file

e.g.: node@20, cargo:ripgrep@latest npm:prettier@3
If no version is specified, it will default to @latest

## Flags

### `-f --force`

Force reinstall even if already installed

### `--fuzzy`

Save fuzzy version to config file

e.g.: `mise use --fuzzy node@20` will save 20 as the version
this is the default behavior unless `MISE_PIN=1` or `MISE_ASDF_COMPAT=1`

### `-g --global`

Use the global config file (`~/.config/mise/config.toml`) instead of the local one

### `-e --env <ENV>`

Modify an environment-specific config file like .mise.&lt;env>.toml

### `-j --jobs <JOBS>`

Number of jobs to run in parallel
[default: 4]

### `--raw`

Directly pipe stdin/stdout/stderr from plugin to user Sets `--jobs=1`

### `--remove... <PLUGIN>`

Remove the plugin(s) from config file

### `-p --path <PATH>`

Specify a path to a config file or directory

If a directory is specified, it will look for `mise.toml` (default) or `.tool-versions` if `MISE_ASDF_COMPAT=1`

### `--pin`

Save exact version to config file
e.g.: `mise use --pin node@20` will save 20.0.0 as the version
Set `MISE_PIN=1` or `MISE_ASDF_COMPAT=1` to make this the default behavior

Examples:

    # set the current version of node to 20.x in mise.toml of current directory
    # will write the fuzzy version (e.g.: 20)
    $ mise use node@20

    # set the current version of node to 20.x in ~/.config/mise/config.toml
    # will write the precise version (e.g.: 20.0.0)
    $ mise use -g --pin node@20

    # sets .mise.local.toml (which is intended not to be committed to a project)
    $ mise use --env local node@20

    # sets .mise.staging.toml (which is used if MISE_ENV=staging)
    $ mise use --env staging node@20
