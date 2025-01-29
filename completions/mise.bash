_mise() {
    if ! command -v usage &> /dev/null; then
        echo >&2
        echo "Error: usage CLI not found. This is required for completions to work in mise." >&2
        echo "See https://usage.jdx.dev for more information." >&2
        return 1
    fi

    if [[ -z ${_usage_spec_mise_2025_1_16:-} ]]; then
        _usage_spec_mise_2025_1_16="$(mise usage)"
    fi

	local cur prev words cword was_split comp_args
    _comp_initialize -n : -- "$@" || return
    # shellcheck disable=SC2207
	_comp_compgen -- -W "$(usage complete-word --shell bash -s "${_usage_spec_mise_2025_1_16}" --cword="$cword" -- "${words[@]}")"
	_comp_ltrim_colon_completions "$cur"
    # shellcheck disable=SC2181
    if [[ $? -ne 0 ]]; then
        unset COMPREPLY
    fi
    return 0
}

if [[ "${BASH_VERSINFO[0]}" -eq 4 && "${BASH_VERSINFO[1]}" -ge 4 || "${BASH_VERSINFO[0]}" -gt 4 ]]; then
    shopt -u hostcomplete && complete -o nospace -o bashdefault -o nosort -F _mise mise
else
    shopt -u hostcomplete && complete -o nospace -o bashdefault -F _mise mise
fi
# vim: noet ci pi sts=0 sw=4 ts=4 ft=sh
