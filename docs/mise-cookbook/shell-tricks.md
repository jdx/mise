# Shell tricks

A collection of shell utities leveraging mise.

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

## Current configuration environment in powerline-go prompt

[powerline-go](https://github.com/justjanne/powerline-go)'s
`shell-var` segment can be used to display the value of an environment
variable in the prompt.
The current mise [configuration environment](/configuration/environments),
`MISE_ENV` is a good candidate for this.

Mostly, it is as one would expect: include `shell-var` in `-modules`,
and `-shell-var MISE_ENV -shell-var-no-warn-empty` in arguments,
and make sure `MISE_ENV` is exported so `powerline-go` can "see" it.

A gotcha as of February 2025 is that the `shell-var` module does not
tolerate _unset_ (as opposed to empty) environment variables.
To work around this, set `MISE_ENV` to an empty value early in the shell
startup scripts, and avoid manually `unset`ing it.
For example for bash, typically in `~/.bashrc`:

```bash
export MISE_ENV=
```

## Inspect what changed after mise hook

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
