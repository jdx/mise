_mise() {
    if ! command -v usage &> /dev/null; then
        echo "Error: usage not found. This is required for completions to work in mise." >&2
        return 1
    fi

    if [[ -z ${_USAGE_SPEC_MISE:-} ]]; then
        _USAGE_SPEC_MISE="$(mise usage)"
    fi
    
    COMPREPLY=( $(usage complete-word -s "${_USAGE_SPEC_MISE}" --cword="$COMP_CWORD" -- "${COMP_WORDS[@]}" ) )
    if [[ $? -ne 0 ]]; then
        unset COMPREPLY
    fi
    return 0
}

shopt -u hostcomplete && complete -o nospace -o bashdefault -o nosort -F _mise mise
# vim: noet ci pi sts=0 sw=4 ts=4 ft=sh
