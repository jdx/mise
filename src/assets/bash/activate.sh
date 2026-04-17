# shellcheck shell=bash
export __MISE_EXE=__MISE_EXE_VALUE__
__MISE_FLAGS=(__MISE_FLAGS_VALUE__)
__MISE_HOOK_ENABLED=__MISE_HOOK_ENABLED_VALUE__

export MISE_SHELL=bash

# On first activation, save the original PATH
# On re-activation, we keep the saved original
if [ -z "${__MISE_ORIG_PATH:-}" ]; then
	export __MISE_ORIG_PATH="$PATH"
fi
__MISE_BASH_CHPWD_RAN=0

mise() {
	local command
	command="${1:-}"
	if [ "$#" = 0 ]; then
		command "$__MISE_EXE"
		return
	fi
	shift

	case "$command" in
	deactivate | shell | sh)
		# if argv doesn't contains -h,--help
		if [[ ! " $* " =~ " --help " ]] && [[ ! " $* " =~ " -h " ]]; then
			eval "$(command "$__MISE_EXE" "$command" "$@")"
			return $?
		fi
		;;
	esac
	command "$__MISE_EXE" "$command" "$@"
}

_mise_hook() {
	local previous_exit_status=$?
	eval "$(mise hook-env ${__MISE_FLAGS[@]+"${__MISE_FLAGS[@]}"} -s bash)"
	return $previous_exit_status
}

if [ "$__MISE_HOOK_ENABLED" = "1" ]; then
	_mise_hook_prompt_command() {
		local previous_exit_status=$?
		if [[ ${__MISE_BASH_CHPWD_RAN:-0} == "1" ]]; then
			__MISE_BASH_CHPWD_RAN=0
			return $previous_exit_status
		fi
		eval "$(mise hook-env ${__MISE_FLAGS[@]+"${__MISE_FLAGS[@]}"} -s bash --reason precmd)"
		return $previous_exit_status
	}

	_mise_hook_chpwd() {
		local previous_exit_status=$?
		__MISE_BASH_CHPWD_RAN=1
		eval "$(mise hook-env ${__MISE_FLAGS[@]+"${__MISE_FLAGS[@]}"} -s bash --reason chpwd)"
		return $previous_exit_status
	}

	_mise_add_prompt_command() {
		if [[ "$(declare -p PROMPT_COMMAND 2>/dev/null)" == "declare -a"* ]]; then
			if [[ " ${PROMPT_COMMAND[*]} " != *" _mise_hook_prompt_command "* ]]; then
				PROMPT_COMMAND=("_mise_hook_prompt_command" "${PROMPT_COMMAND[@]}")
			fi
		elif [[ ";${PROMPT_COMMAND:-};" != *";_mise_hook_prompt_command;"* ]]; then
			local _mise_prompt_command_value="${PROMPT_COMMAND-}"
			printf -v PROMPT_COMMAND '%s' "_mise_hook_prompt_command${_mise_prompt_command_value:+;$_mise_prompt_command_value}"
		fi
	}

	_mise_add_prompt_command
	__MISE_CHPWD_FUNCTIONS__
	__MISE_CHPWD_LOAD__
	chpwd_functions+=(_mise_hook_chpwd)
fi

_mise_hook
