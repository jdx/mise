# `mise use`

- **Usage**: `mise use [FLAGS] [TOOL@VERSION]...`
- **Aliases**: `u`
- **Source code**: [`src/cli/use.rs`](https://github.com/jdx/mise/blob/main/src/cli/use.rs)

Installs a tool and adds the version to mise.toml.

This will install the tool version if it is not already installed.
By default, this will use a `mise.toml` file in the current directory.

In the following order:

- If `MISE_DEFAULT_CONFIG_FILENAME` is set, it will use that instead.
- If `MISE_OVERRIDE_CONFIG_FILENAMES` is set, it will the first from that list.
- If `MISE_ENV` is set, it will use a `mise.<env>.toml` instead.
- Otherwise just "mise.toml"

Use the `--global` flag to use the global config file instead.

## Arguments

### `[TOOL@VERSION]...`

Tool(s) to add to config file

e.g.: node@20, cargo:ripgrep@latest npm:prettier@3
If no version is specified, it will default to @latest

Tool options can be set with this syntax:

```
mise use ubi:BurntSushi/ripgrep[exe=rg]
```

## Flags

### `-f --force`

Force reinstall even if already installed

### `--fuzzy`

Save fuzzy version to config file

e.g.: `mise use --fuzzy node@20` will save 20 as the version
this is the default behavior unless `MISE_PIN=1`

### `-g --global`

Use the global config file (`~/.config/mise/config.toml`) instead of the local one

### `-e --env <ENV>`

Create/modify an environment-specific config file like .mise.&lt;env>.toml

### `-j --jobs <JOBS>`

Number of jobs to run in parallel
[default: 4]

### `--raw`

Directly pipe stdin/stdout/stderr from plugin to user Sets `--jobs=1`

### `--remove... <PLUGIN>`

Remove the plugin(s) from config file

### `-p --path <PATH>`

Specify a path to a config file or directory

If a directory is specified, it will look for a config file in that directory following the rules above.

### `--pin`

Save exact version to config file
e.g.: `mise use --pin node@20` will save 20.0.0 as the version
Set `MISE_PIN=1` to make this the default behavior

Consider using mise.lock as a better alternative to pinning in mise.toml:
<https://mise.jdx.dev/configuration/settings.html#lockfile>

Examples:

```

# run with no arguments to use the interactive selector
$ mise use
```

```
# set the current version of node to 20.x in mise.toml of current directory
# will write the fuzzy version (e.g.: 20)
$ mise use node@20
```

```
# set the current version of node to 20.x in ~/.config/mise/config.toml
# will write the precise version (e.g.: 20.0.0)
$ mise use -g --pin node@20
```

```
# sets .mise.local.toml (which is intended not to be committed to a project)
$ mise use --env local node@20
```

```
# sets .mise.staging.toml (which is used if MISE_ENV=staging)
$ mise use --env staging node@20
```
