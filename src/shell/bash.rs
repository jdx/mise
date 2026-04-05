#![allow(unknown_lints)]
#![allow(clippy::literal_string_with_formatting_args)]
use std::fmt::Display;

use indoc::formatdoc;
use shell_escape::unix::escape;

use crate::config::Settings;
use crate::shell::{self, ActivateOptions, Shell};

#[derive(Default)]
pub struct Bash {}

impl Bash {}

impl Shell for Bash {
    fn activate(&self, opts: ActivateOptions) -> String {
        let exe = opts.exe;
        let flags = opts.flags;
        let settings = Settings::get();

        let exe = escape(exe.to_string_lossy());

        let mut out = String::new();

        out.push_str(&shell::build_deactivation_script(self));

        out.push_str(&self.format_activate_prelude(&opts.prelude));
        out.push_str(&formatdoc! {r#"
            export MISE_SHELL=bash

            # On first activation, save the original PATH
            # On re-activation, we keep the saved original
            if [ -z "${{__MISE_ORIG_PATH:-}}" ]; then
              export __MISE_ORIG_PATH="$PATH"
            fi
            __MISE_BASH_CHPWD_RAN=0

            mise() {{
              local command
              command="${{1:-}}"
              if [ "$#" = 0 ]; then
                command {exe}
                return
              fi
              shift

              case "$command" in
              deactivate|shell|sh)
                # if argv doesn't contains -h,--help
                if [[ ! " $* " =~ " --help " ]] && [[ ! " $* " =~ " -h " ]]; then
                  eval "$(command {exe} "$command" "$@")"
                  return $?
                fi
                ;;
              esac
              command {exe} "$command" "$@"
            }}

            _mise_hook() {{
              local previous_exit_status=$?;
              eval "$(mise hook-env{flags} -s bash)";
              return $previous_exit_status;
            }};
            "#});
        if !opts.no_hook_env {
            out.push_str(&formatdoc! {r#"
            _mise_hook_prompt_command() {{
              local previous_exit_status=$?;
              if [[ "${{__MISE_BASH_CHPWD_RAN:-0}}" == "1" ]]; then
                __MISE_BASH_CHPWD_RAN=0
                return $previous_exit_status;
              fi
              eval "$(mise hook-env{flags} -s bash --reason precmd)";
              return $previous_exit_status;
            }};
            _mise_hook_chpwd() {{
              local previous_exit_status=$?;
              __MISE_BASH_CHPWD_RAN=1
              eval "$(mise hook-env{flags} -s bash --reason chpwd)";
              return $previous_exit_status;
            }};
            _mise_add_prompt_command() {{
              if [[ "$(declare -p PROMPT_COMMAND 2>/dev/null)" == "declare -a"* ]]; then
                if [[ " ${{PROMPT_COMMAND[*]}} " != *" _mise_hook_prompt_command "* ]]; then
                  PROMPT_COMMAND=("_mise_hook_prompt_command" "${{PROMPT_COMMAND[@]}}")
                fi
              elif [[ ";${{PROMPT_COMMAND:-}};" != *";_mise_hook_prompt_command;"* ]]; then
                PROMPT_COMMAND="_mise_hook_prompt_command${{PROMPT_COMMAND:+;$PROMPT_COMMAND}}"
              fi
            }};
            _mise_add_prompt_command
            {chpwd_functions}
            {chpwd_load}
            chpwd_functions+=(_mise_hook_chpwd)
            _mise_hook
            "#,
            chpwd_functions = include_str!("../assets/bash_zsh_support/chpwd/function.sh"),
            chpwd_load = include_str!("../assets/bash_zsh_support/chpwd/load.sh")
            });
        }
        if settings.not_found_auto_install {
            out.push_str(&formatdoc! {r#"
            if [ -z "${{_mise_cmd_not_found:-}}" ]; then
                _mise_cmd_not_found=1
                if [ -n "$(declare -f command_not_found_handle)" ]; then
                    _mise_cmd_not_found_handle=$(declare -f command_not_found_handle)
                    eval "${{_mise_cmd_not_found_handle/command_not_found_handle/_command_not_found_handle}}"
                fi

                command_not_found_handle() {{
                    if [[ "$1" != "mise" && "$1" != "mise-"* ]] && {exe} hook-not-found -s bash -- "$1"; then
                      _mise_hook
                      "$@"
                    elif [ -n "$(declare -f _command_not_found_handle)" ]; then
                        _command_not_found_handle "$@"
                    else
                        echo "bash: command not found: $1" >&2
                        return 127
                    fi
                }}
            fi
            "#});
        }

        out
    }

    fn deactivate(&self) -> String {
        formatdoc! {r#"
            if [[ "$(declare -p PROMPT_COMMAND 2>/dev/null)" == "declare -a"* ]]; then
                _mise_prompt_command=()
                for _mise_pc in "${{PROMPT_COMMAND[@]}}"; do
                    if [[ "$_mise_pc" != "_mise_hook_prompt_command" ]]; then
                        _mise_prompt_command+=("$_mise_pc")
                    fi
                done
                PROMPT_COMMAND=("${{_mise_prompt_command[@]}}")
                unset _mise_prompt_command _mise_pc
            elif [[ ${{PROMPT_COMMAND-}} == *_mise_hook_prompt_command* ]]; then
                PROMPT_COMMAND="${{PROMPT_COMMAND//_mise_hook_prompt_command;/}}"
                PROMPT_COMMAND="${{PROMPT_COMMAND//;_mise_hook_prompt_command/}}"
                PROMPT_COMMAND="${{PROMPT_COMMAND//_mise_hook_prompt_command/}}"
            elif [[ ${{PROMPT_COMMAND-}} == *_mise_hook* ]]; then
                PROMPT_COMMAND="${{PROMPT_COMMAND//_mise_hook;/}}"
                PROMPT_COMMAND="${{PROMPT_COMMAND//_mise_hook/}}"
            fi
            if declare -p chpwd_functions >/dev/null 2>&1; then
                _mise_chpwd_functions=()
                for _mise_f in "${{chpwd_functions[@]}}"; do
                    if [[ "$_mise_f" != "_mise_hook_chpwd" ]]; then
                        _mise_chpwd_functions+=("$_mise_f")
                    fi
                done
                chpwd_functions=("${{_mise_chpwd_functions[@]}}")
                unset _mise_chpwd_functions _mise_f
            fi
            declare -F _mise_hook_prompt_command >/dev/null && unset -f _mise_hook_prompt_command
            declare -F _mise_add_prompt_command >/dev/null && unset -f _mise_add_prompt_command
            declare -F _mise_hook_chpwd >/dev/null && unset -f _mise_hook_chpwd
            declare -F _mise_hook >/dev/null && unset -f _mise_hook
            declare -F mise >/dev/null && unset -f mise
            unset MISE_SHELL
            unset __MISE_DIFF
            unset __MISE_SESSION
            unset __MISE_BASH_CHPWD_RAN
        "#}
    }

    fn set_env(&self, k: &str, v: &str) -> String {
        let k = shell_escape::unix::escape(k.into());
        let v = shell_escape::unix::escape(v.into());
        format!("export {k}={v}\n")
    }

    fn prepend_env(&self, k: &str, v: &str) -> String {
        format!("export {k}=\"{v}:${k}\"\n")
    }

    fn unset_env(&self, k: &str) -> String {
        format!("unset {k}\n", k = shell_escape::unix::escape(k.into()))
    }

    fn set_alias(&self, name: &str, cmd: &str) -> String {
        let name = shell_escape::unix::escape(name.into());
        let cmd = shell_escape::unix::escape(cmd.into());
        format!("alias {name}={cmd}\n")
    }

    fn unset_alias(&self, name: &str) -> String {
        let name = shell_escape::unix::escape(name.into());
        format!("unalias {name} 2>/dev/null || true\n")
    }
}

impl Display for Bash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "bash")
    }
}

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;
    use std::path::Path;
    use test_log::test;

    use crate::test::replace_path;

    use super::*;

    #[test]
    fn test_activate() {
        unsafe {
            std::env::remove_var("__MISE_ORIG_PATH");
            std::env::remove_var("__MISE_DIFF");
        }

        let bash = Bash::default();
        let exe = Path::new("/some/dir/mise");
        let opts = ActivateOptions {
            exe: exe.to_path_buf(),
            flags: " --status".into(),
            no_hook_env: false,
            prelude: vec![],
        };
        assert_snapshot!(bash.activate(opts));
    }

    #[test]
    fn test_set_env() {
        assert_snapshot!(Bash::default().set_env("FOO", "1"));
    }

    #[test]
    fn test_prepend_env() {
        let bash = Bash::default();
        assert_snapshot!(replace_path(&bash.prepend_env("PATH", "/some/dir:/2/dir")));
    }

    #[test]
    fn test_unset_env() {
        assert_snapshot!(Bash::default().unset_env("FOO"));
    }

    #[test]
    fn test_deactivate() {
        let deactivate = Bash::default().deactivate();
        assert_snapshot!(replace_path(&deactivate));
    }
}
