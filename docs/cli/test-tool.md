# `mise test-tool`

- **Usage**: `mise test-tool [FLAGS] [TOOL]`
- **Source code**: [`src/cli/test_tool.rs`](https://github.com/jdx/mise/blob/main/src/cli/test_tool.rs)

Test a tool installs and executes

## Arguments

### `[TOOL]`

Tool name to test

## Flags

### `-a --all`

Test every tool specified in registry.toml

### `--all-config`

Test all tools specified in config files

### `--include-non-defined`

Also test tools not defined in registry.toml, guessing how to test it

### `-j --jobs <JOBS>`

Number of jobs to run in parallel
[default: 4]

### `--raw`

Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1

Examples:

```
mise test-tool ripgrep
```
