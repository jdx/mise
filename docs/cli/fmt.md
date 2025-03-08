# `mise fmt`

- **Usage**: `mise fmt [-a --all]`
- **Source code**: [`src/cli/fmt.rs`](https://github.com/jdx/mise/blob/main/src/cli/fmt.rs)

Formats mise.toml

Sorts keys and cleans up whitespace in mise.toml

## Flags

### `-a --all`

Format all files from the current directory

### `-s --stdin`

Read config from `stdin` and write its formatted version into `stdout`

Examples:

```
mise fmt
```
