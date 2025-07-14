# `mise unuse`

- **Usage**: `mise unuse [FLAGS] <INSTALLED_TOOL@VERSION>…`
- **Aliases**: `rm`, `remove`
- **Source code**: [`src/cli/unuse.rs`](https://github.com/jdx/mise/blob/main/src/cli/unuse.rs)

Removes installed tool versions from mise.toml

By default, this will use the `mise.toml` file that has the tool defined.

In the following order:
- If `--global` is set, it will use the global config file.
- If `--path` is set, it will use the config file at the given path.
- If `--env` is set, it will use `mise.<env>.toml`.
- If `MISE_DEFAULT_CONFIG_FILENAME` is set, it will use that instead.
- If `MISE_OVERRIDE_CONFIG_FILENAMES` is set, it will the first from that list.
- Otherwise just "mise.toml" or global config if cwd is home directory.

Will also prune the installed version if no other configurations are using it.

## Arguments

### `<INSTALLED_TOOL@VERSION>…`

Tool(s) to remove

## Flags

### `-g --global`

Use the global config file (`~/.config/mise/config.toml`) instead of the local one

### `-e --env <ENV>`

Create/modify an environment-specific config file like .mise.&lt;env>.toml

### `-p --path <PATH>`

Specify a path to a config file or directory

If a directory is specified, it will look for a config file in that directory following the rules above.

### `--no-prune`

Do not also prune the installed version

Examples:

```
# will uninstall specific version
$ mise unuse node@18.0.0

# will uninstall specific version from global config
$ mise unuse -g node@18.0.0

# will uninstall specific version from .mise.local.toml
$ mise unuse --env local node@20

# will uninstall specific version from .mise.staging.toml
$ mise unuse --env staging node@20
```
