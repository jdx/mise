# `mise completion`

- **Usage**: `mise completion [SHELL]`
- **Source code**: [`src/cli/completion.rs`](https://github.com/jdx/mise/blob/main/src/cli/completion.rs)

Generate shell completions

## Arguments

### `[SHELL]`

Shell type to generate completions for

**Choices:**

- `bash`
- `fish`
- `zsh`

Examples:

    mise completion bash > /etc/bash_completion.d/mise
    mise completion zsh  > /usr/local/share/zsh/site-functions/_mise
    mise completion fish > ~/.config/fish/completions/mise.fish
