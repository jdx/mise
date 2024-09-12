## `mise completion [SHELL]`

```text
Generate shell completions

Usage: completion [SHELL]

Arguments:
  [SHELL]
          Shell type to generate completions for
          
          [possible values: bash, fish, zsh]

Examples:

    $ mise completion bash > /etc/bash_completion.d/mise
    $ mise completion zsh  > /usr/local/share/zsh/site-functions/_mise
    $ mise completion fish > ~/.config/fish/completions/mise.fish
```
