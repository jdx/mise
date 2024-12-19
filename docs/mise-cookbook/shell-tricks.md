# Shell tricks

## Prompt colouring

In ZSH to set the prompt colour whenever mise updates the environment (e.g. on cd into a project, or due to modification of the .mise\*.toml):

```shell
# activate mise like normal
source <(command mise activate zsh)

typeset -i _mise_updated

# replace default mise hook
function _mise_hook {
  local diff=${__MISE_DIFF}
  source <(command mise hook-env -s zsh)
  [[ ${diff} == ${__MISE_DIFF} ]]
  _mise_updated=$?
}

_PROMPT="❱ "  # or _PROMPT=${PROMPT} to keep the default

function _prompt {
  if (( ${_mise_updated} )); then
    PROMPT='%F{blue}${_PROMPT}%f'
  else
    PROMPT='%(?.%F{green}${_PROMPT}%f.%F{red}${_PROMPT}%f)'
  fi
}

add-zsh-hook precmd _prompt
```

Now, when mise makes any updates to the environment the prompt will go blue.

## Inspect what mise hook is doing

Using record-query you can inspect the `__MISE_DIFF` and `__MISE_SESSION` variables to see what's changing in your environment due to the mise hook.

```toml [~/.config/mise/config.toml]
[tools]
"cargo:record-query" = "latest"
```

```shell
function mise_parse_env {
  rq -m < <(
    zcat -q < <(
      printf $'\x1f\x8b\x08\x00\x00\x00\x00\x00'
      base64 -d <<< "$1"
    )
  )
}
```

```shell
$ mise_parse_env "${__MISE_DIFF}"
{
  "new": {
    ...
  },
  "old": {
    ...
  },
  "path": [
    ...
  ]
}
```
