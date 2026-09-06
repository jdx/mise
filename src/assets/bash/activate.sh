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
			unset __MISE_BASH_SKIP_FIRST_PROMPT
			return $previous_exit_status
		fi
		if [[ ${__MISE_BASH_SKIP_FIRST_PROMPT:-0} == "1" ]]; then
			unset __MISE_BASH_SKIP_FIRST_PROMPT
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

# `--no-hook-env` means mise does not apply the environment at activation -- zsh, fish and pwsh
# all keep their equivalent call inside this guard, and bash did too until the script moved out
# of bash.rs into this file (#8920), which left the call outside it. The skip flag exists because
# the hook runs here, so the two belong under one condition; it is set first so the `$?` that
# `_mise_hook` saves is not read straight off the guard's condition, where it is the test's
# status rather than a command's (SC2319).
if [ "$__MISE_HOOK_ENABLED" = "1" ]; then
	__MISE_BASH_SKIP_FIRST_PROMPT=1
	_mise_hook
fi
