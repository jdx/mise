# shellcheck shell=bash
if [[ "$(declare -p PROMPT_COMMAND 2>/dev/null)" == "declare -a"* ]]; then
	_mise_prompt_command=()
	for _mise_pc in "${PROMPT_COMMAND[@]}"; do
		if [[ $_mise_pc != "_mise_hook_prompt_command" && $_mise_pc != "_mise_hook" ]]; then
			_mise_prompt_command+=("$_mise_pc")
		fi
	done
	PROMPT_COMMAND=("${_mise_prompt_command[@]}")
	unset _mise_prompt_command _mise_pc
elif [[ ${PROMPT_COMMAND-} == *_mise_hook_prompt_command* ]]; then
	_mise_prompt_command_value="${PROMPT_COMMAND-}"
	_mise_prompt_command_value="${_mise_prompt_command_value//_mise_hook_prompt_command;/}"
	_mise_prompt_command_value="${_mise_prompt_command_value//;_mise_hook_prompt_command/}"
	_mise_prompt_command_value="${_mise_prompt_command_value//_mise_hook_prompt_command/}"
	printf -v PROMPT_COMMAND '%s' "$_mise_prompt_command_value"
	unset _mise_prompt_command_value
elif [[ ${PROMPT_COMMAND-} == *_mise_hook* ]]; then
	_mise_prompt_command_value="${PROMPT_COMMAND-}"
	_mise_prompt_command_value="${_mise_prompt_command_value//_mise_hook;/}"
	_mise_prompt_command_value="${_mise_prompt_command_value//;_mise_hook/}"
	_mise_prompt_command_value="${_mise_prompt_command_value//_mise_hook/}"
	printf -v PROMPT_COMMAND '%s' "$_mise_prompt_command_value"
	unset _mise_prompt_command_value
fi

if declare -p chpwd_functions >/dev/null 2>&1; then
	_mise_chpwd_functions=()
	for _mise_f in "${chpwd_functions[@]}"; do
		if [[ $_mise_f != "_mise_hook_chpwd" && $_mise_f != "_mise_hook" ]]; then
			_mise_chpwd_functions+=("$_mise_f")
		fi
	done
	chpwd_functions=("${_mise_chpwd_functions[@]}")
	unset _mise_chpwd_functions _mise_f
fi

declare -F _mise_hook_prompt_command >/dev/null && unset -f _mise_hook_prompt_command
declare -F _mise_add_prompt_command >/dev/null && unset -f _mise_add_prompt_command
declare -F _mise_hook_chpwd >/dev/null && unset -f _mise_hook_chpwd
declare -F _mise_hook >/dev/null && unset -f _mise_hook
if [ -n "${_mise_cmd_not_found_handle:-}" ]; then
	eval "$_mise_cmd_not_found_handle"
	unset _mise_cmd_not_found_handle
	declare -F _command_not_found_handle >/dev/null && unset -f _command_not_found_handle
elif [[ "$(declare -f command_not_found_handle 2>/dev/null)" == *"hook-not-found"* ]]; then
	declare -F command_not_found_handle >/dev/null && unset -f command_not_found_handle
fi
declare -F mise >/dev/null && unset -f mise
unset MISE_SHELL
unset __MISE_DIFF
unset __MISE_SESSION
unset __MISE_EXE
unset __MISE_FLAGS
unset __MISE_HOOK_ENABLED
unset __MISE_BASH_CHPWD_RAN
unset _mise_cmd_not_found
