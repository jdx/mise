# `mise unuse`

- **Usage**: `mise unuse [--no-prune] [--global] <INSTALLED_TOOL@VERSION>...`
- **Aliases**: `rm`, `remove`
- **Source code**: [`src/cli/unuse.rs`](https://github.com/jdx/mise/blob/main/src/cli/unuse.rs)

Removes installed tool versions from mise.toml

Will also prune the installed version if no other configurations are using it.

## Arguments

### `<INSTALLED_TOOL@VERSION>...`

Tool(s) to remove

## Flags

### `--no-prune`

Do not also prune the installed version

### `--global`

Remove tool from global config

Examples:

```
# will uninstall specific version
$ mise remove node@18.0.0
```
