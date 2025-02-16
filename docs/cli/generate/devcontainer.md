# `mise generate devcontainer`

- **Usage**: `mise generate devcontainer [FLAGS]`
- **Source code**: [`src/cli/generate/devcontainer.rs`](https://github.com/jdx/mise/blob/main/src/cli/generate/devcontainer.rs)

[experimental] Generate a devcontainer to execute mise

## Flags

### `-n --name <NAME>`

The name of the devcontainer

### `-i --image <IMAGE>`

The image to use for the devcontainer

### `-m --mount-mise-data`

Bind the mise-data-volume to the devcontainer

### `-w --write`

write to .devcontainer/devcontainer.json

Examples:

```
mise generate devcontainer
```
