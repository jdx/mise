# `mise config`

- **Usage**: `mise config [--no-header] [-J --json] <SUBCOMMAND>`
- **Aliases**: `cfg`
- **Source code**: [`src/cli/config.rs`](https://github.com/jdx/mise/blob/main/src/cli/config.rs)

Manage config files

## Flags

### `--no-header`

Do not print table header

### `-J --json`

Output in JSON format

## Subcommands

- [`mise config generate [-o --output <OUTPUT>]`](/cli/config/generate.md)
- [`mise config get [-f --file <FILE>] [KEY]`](/cli/config/get.md)
- [`mise config ls [--no-header] [-J --json]`](/cli/config/ls.md)
- [`mise config set [-f --file <FILE>] [-t --type <TYPE>] <KEY> <VALUE>`](/cli/config/set.md)
