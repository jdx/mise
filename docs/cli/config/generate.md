# `mise config generate`

**Usage**: `mise config generate [-o --output <OUTPUT>]`

**Source code**: [`src/cli/config/generate.rs`](https://github.com/jdx/mise/blob/main/src/cli/config/generate.rs)

**Aliases**: `g`

[experimental] Generate a mise.toml file

## Flags

### `-o --output <OUTPUT>`

Output to file instead of stdout

Examples:

    mise cf generate > mise.toml
    mise cf generate --output=mise.toml
