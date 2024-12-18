# `mise generate config`

- **Usage**: `mise generate config [-t --tool-versions <TOOL_VERSIONS>] [-o --output <OUTPUT>]`
- **Aliases**: `g`
- **Source code**: [`src/cli/generate/config.rs`](https://github.com/jdx/mise/blob/main/src/cli/generate/config.rs)

[experimental] Generate a mise.toml file

## Flags

### `-t --tool-versions <TOOL_VERSIONS>`

Path to a .tool-versions file to import tools from

### `-o --output <OUTPUT>`

Output to file instead of stdout

Examples:

```
mise cf generate > mise.toml
mise cf generate --output=mise.toml
```
