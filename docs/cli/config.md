# `mise config`

- **Usage**: `mise config [FLAGS] <SUBCOMMAND>`
- **Aliases**: `cfg`
- **Source code**: [`src/cli/config/mod.rs`](https://github.com/jdx/mise/blob/main/src/cli/config/mod.rs)

Manage config files

## Flags

### `--no-header`

Do not print table header

### `--tracked-configs`

List all tracked config files

### `-J --json`

Output in JSON format

## Subcommands

- [`mise config generate [-t --tool-versions <TOOL_VERSIONS>] [-o --output <OUTPUT>]`](/cli/config/generate.md)
- [`mise config get [-f --file <FILE>] [KEY]`](/cli/config/get.md)
- [`mise config ls [FLAGS]`](/cli/config/ls.md)
- [`mise config set [-f --file <FILE>] [-t --type <TYPE>] <KEY> <VALUE>`](/cli/config/set.md)

Examples:

```
$ mise config ls
Path                        Tools
~/.config/mise/config.toml  pitchfork
~/src/mise/mise.toml        actionlint, bun, cargo-binstall, cargo:cargo-edit, cargo:cargo-insta
```
