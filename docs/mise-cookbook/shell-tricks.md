# Shell tricks

A collection of shell utilities that build on mise.

## Prompt colouring

In Zsh, add a prompt hook after your existing `mise activate zsh` setup. This
example replaces the prompt with a blue marker when mise's environment state
changes and a green marker otherwise. It leaves mise's activation functions intact:

```zsh
# Put this after your existing mise activation in ~/.zshrc.
autoload -Uz add-zsh-hook
typeset -g _mise_prompt_diff="${__MISE_DIFF-}"

function _mise_prompt_colour {
  local previous_status=$?
  if [[ "${__MISE_DIFF-}" != "$_mise_prompt_diff" ]]; then
    PROMPT='%F{blue}❱ %f'
  else
    PROMPT='%F{green}❱ %f'
  fi
  _mise_prompt_diff="${__MISE_DIFF-}"
  return "$previous_status"
}

add-zsh-hook -d precmd _mise_prompt_colour
add-zsh-hook precmd _mise_prompt_colour
```

`__MISE_DIFF` is internal state, so treat this as a customization to maintain when
upgrading mise. To undo it, remove `_mise_prompt_colour` with `add-zsh-hook -d precmd
_mise_prompt_colour` and restore your usual `PROMPT` or prompt theme.

## Current configuration environment in powerline-go prompt

[powerline-go](https://github.com/justjanne/powerline-go)'s
`shell-var` segment can be used to display the value of an environment
variable in the prompt.
The current mise [configuration environment](/configuration/environments),
`MISE_ENV`, is a good candidate for this.

Mostly, it works as you would expect: include `shell-var` in `-modules`,
pass `-shell-var MISE_ENV -shell-var-no-warn-empty` in the arguments,
and make sure `MISE_ENV` is exported so `powerline-go` can see it.

If your version of powerline-go warns when `MISE_ENV` is unset, ensure the variable
is defined while preserving any selection made before the shell started:

```bash
export MISE_ENV="${MISE_ENV-}"
```

This displays the exported `MISE_ENV` value. Environments chosen only for one
command with `mise -E`, or platform environments selected by `auto_env`, are not
persistent changes to that shell variable.

## Inspect what changed after mise hook

For ordinary troubleshooting, start with `mise config`, `mise doctor`, or
`MISE_DEBUG=1 mise env`. If you need to inspect shell bookkeeping itself,
`__MISE_DIFF` and `__MISE_SESSION` currently contain base64-encoded, zlib-compressed
MessagePack data.

The following Bash/Zsh helper requires Python and the `msgpack` package. Create an
isolated Python environment for the decoder:

```sh
python3 -m venv ~/.cache/mise-env-inspect
~/.cache/mise-env-inspect/bin/python -m pip install msgpack
```

```bash
function mise_parse_env {
  printf '%s' "$1" | "$HOME/.cache/mise-env-inspect/bin/python" -c '
import base64, pprint, sys, zlib
import msgpack
value = sys.stdin.read().strip()
if not value:
    raise SystemExit("No mise state was supplied; activate mise first")
payload = zlib.decompress(base64.b64decode(value + "=" * (-len(value) % 4)))
pprint.pprint(msgpack.unpackb(payload, raw=False), sort_dicts=False)
'
}
```

Use it in an activated shell:

```sh
mise_parse_env "$__MISE_DIFF"
mise_parse_env "$__MISE_SESSION"
```

This format is an implementation detail, not an API. The decoded data can contain
environment values, including secrets; inspect it locally and redact it before
sharing diagnostic output.
