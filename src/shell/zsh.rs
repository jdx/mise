#![allow(unknown_lints)]
#![allow(clippy::literal_string_with_formatting_args)]
use std::fmt::Display;

use indoc::formatdoc;
use shell_escape::unix::escape;

use crate::config::Settings;
use crate::shell::bash::Bash;
use crate::shell::{self, ActivateOptions, Shell};

#[derive(Default)]
pub struct Zsh {}

impl Zsh {}

impl Shell for Zsh {
    fn activate(&self, opts: ActivateOptions) -> String {
        let exe = opts.exe;
        let flags = opts.flags;

        let exe = escape(exe.to_string_lossy());
        let mut out = String::new();

        out.push_str(&shell::build_deactivation_script(self));

        out.push_str(&self.format_activate_prelude(&opts.prelude));

        // much of this is from direnv
        // https://github.com/direnv/direnv/blob/cb5222442cb9804b1574954999f6073cc636eff0/internal/cmd/shell_zsh.go#L10-L22
        out.push_str(&formatdoc! {r#"
            export MISE_SHELL=zsh
            if [ -z "${{__MISE_ORIG_PATH:-}}" ]; then
              export __MISE_ORIG_PATH="$PATH"
            fi
            export __MISE_ZSH_PRECMD_RUN=0

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
                if [[ ! " $@ " =~ " --help " ]] && [[ ! " $@ " =~ " -h " ]]; then
                  eval "$(command {exe} "$command" "$@")"
                  return $?
                fi
                ;;
              esac
              command {exe} "$command" "$@"
            }}
        "#});

        if !opts.no_hook_env {
            out.push_str(&formatdoc! {r#"

            _mise_hook() {{
              eval "$({exe} hook-env{flags} -s zsh)";
            }}
            _mise_hook_precmd() {{
              eval "$({exe} hook-env{flags} -s zsh --reason precmd)";
            }}
            _mise_hook_chpwd() {{
              eval "$({exe} hook-env{flags} -s zsh --reason chpwd)";
            }}
            typeset -ag precmd_functions;
            if [[ -z "${{precmd_functions[(r)_mise_hook_precmd]+1}}" ]]; then
              precmd_functions=( _mise_hook_precmd ${{precmd_functions[@]}} )
            fi
            typeset -ag chpwd_functions;
            if [[ -z "${{chpwd_functions[(r)_mise_hook_chpwd]+1}}" ]]; then
              chpwd_functions=( _mise_hook_chpwd ${{chpwd_functions[@]}} )
            fi

            _mise_hook
            "#});
        }
        if Settings::get().not_found_auto_install {
            out.push_str(&formatdoc! {r#"
            if [ -z "${{_mise_cmd_not_found:-}}" ]; then
                _mise_cmd_not_found=1
                [ -n "$(declare -f command_not_found_handler)" ] && eval "${{$(declare -f command_not_found_handler)/command_not_found_handler/_command_not_found_handler}}"

                function command_not_found_handler() {{
                    if [[ "$1" != "mise" && "$1" != "mise-"* ]] && {exe} hook-not-found -s zsh -- "$1"; then
                      _mise_hook
                      "$@"
                    elif [ -n "$(declare -f _command_not_found_handler)" ]; then
                        _command_not_found_handler "$@"
                    else
                        echo "zsh: command not found: $1" >&2
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
        precmd_functions=( ${{precmd_functions:#_mise_hook_precmd}} )
        chpwd_functions=( ${{chpwd_functions:#_mise_hook_chpwd}} )
        (( $+functions[_mise_hook_precmd] )) && unset -f _mise_hook_precmd
        (( $+functions[_mise_hook_chpwd] )) && unset -f _mise_hook_chpwd
        (( $+functions[_mise_hook] )) && unset -f _mise_hook
        (( $+functions[mise] )) && unset -f mise
        unset MISE_SHELL
        unset __MISE_DIFF
        unset __MISE_SESSION
        unset __MISE_ZSH_PRECMD_RUN
        "#}
    }

    fn set_env(&self, k: &str, v: &str) -> String {
        Bash::default().set_env(k, v)
    }

    fn prepend_env(&self, k: &str, v: &str) -> String {
        format!("export {k}=\"{v}:${k}\"\n")
    }

    fn unset_env(&self, k: &str) -> String {
        Bash::default().unset_env(k)
    }

    fn set_alias(&self, name: &str, cmd: &str) -> String {
        Bash::default().set_alias(name, cmd)
    }

    fn unset_alias(&self, name: &str) -> String {
        Bash::default().unset_alias(name)
    }
}

impl Display for Zsh {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "zsh")
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
        // Unset __MISE_ORIG_PATH to avoid PATH restoration logic in output
        unsafe {
            std::env::remove_var("__MISE_ORIG_PATH");
            std::env::remove_var("__MISE_DIFF");
        }

        let zsh = Zsh::default();
        let exe = Path::new("/some/dir/mise");
        let opts = ActivateOptions {
            exe: exe.to_path_buf(),
            flags: " --status".into(),
            no_hook_env: false,
            prelude: vec![],
        };
        assert_snapshot!(zsh.activate(opts));
    }

    #[test]
    fn test_set_env() {
        assert_snapshot!(Zsh::default().set_env("FOO", "1"));
    }

    #[test]
    fn test_prepend_env() {
        let sh = Bash::default();
        assert_snapshot!(replace_path(&sh.prepend_env("PATH", "/some/dir:/2/dir")));
    }

    #[test]
    fn test_unset_env() {
        assert_snapshot!(Zsh::default().unset_env("FOO"));
    }

    #[test]
    fn test_deactivate() {
        let deactivate = Zsh::default().deactivate();
        assert_snapshot!(replace_path(&deactivate));
    }
}
