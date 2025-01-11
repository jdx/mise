# `mise config ls`

- **Usage**: `mise config ls [FLAGS]`
- **Aliases**: `list`
- **Source code**: [`src/cli/config/ls.rs`](https://github.com/jdx/mise/blob/main/src/cli/config/ls.rs)

List config files currently in use

## Flags

### `--no-header`

Do not print table header

### `--tracked-configs`

List all tracked config files

### `-J --json`

Output in JSON format

Examples:

```
$ mise config ls
Path                        Tools
~/.config/mise/config.toml  pitchfork
~/src/mise/mise.toml        actionlint, bun, cargo-binstall, cargo:cargo-edit, cargo:cargo-insta
```
