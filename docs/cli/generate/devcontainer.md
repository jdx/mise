# `mise generate devcontainer`

- **Usage**: `mise generate devcontainer [FLAGS]`
- **Source code**: [`src/cli/generate/devcontainer.rs`](https://github.com/jdx/mise/blob/main/src/cli/generate/devcontainer.rs)

[experimental] Generate a DevContainer configuration file

This command generates a DevContainer file that runs `mise`in a self-contained environment.

## Flags

### `--mount-mise-data`

By doing this, you can share the `mise-data-volume` across multiple DevContainers.

Examples:

```shell
mise generate devcontainer
```
