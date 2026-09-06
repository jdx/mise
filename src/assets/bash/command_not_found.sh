# shellcheck shell=bash
if [ -z "${_mise_cmd_not_found:-}" ]; then
	_mise_cmd_not_found=1
	if [ -n "$(declare -f command_not_found_handle)" ]; then
		_mise_cmd_not_found_handle=$(declare -f command_not_found_handle)
		eval "${_mise_cmd_not_found_handle/command_not_found_handle/_command_not_found_handle}"
	fi

	command_not_found_handle() {
		if [[ $1 != "mise" && $1 != "mise-"* ]] && __MISE_EXE__ hook-not-found -s bash -- "$1"; then
			_mise_hook
			"$@"
		elif [ -n "$(declare -f _command_not_found_handle)" ]; then
			_command_not_found_handle "$@"
		else
			echo "bash: command not found: $1" >&2
			return 127
		fi
	}
fi
