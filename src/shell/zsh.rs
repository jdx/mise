use std::path::{Path, PathBuf};

use crate::config::Settings;
use crate::shell::bash::Bash;
use crate::shell::{is_dir_in_path, is_dir_not_in_nix, Shell};

#[derive(Default)]
pub struct Zsh {}

impl Shell for Zsh {
    fn activate(&self, exe: &Path, flags: String) -> String {
        let dir = exe.parent().unwrap();
        let exe = exe.to_string_lossy();
        let mut out = String::new();

        // much of this is from direnv
        // https://github.com/direnv/direnv/blob/cb5222442cb9804b1574954999f6073cc636eff0/internal/cmd/shell_zsh.go#L10-L22
        if is_dir_not_in_nix(dir) && !is_dir_in_path(dir) && !dir.is_relative() {
            out.push_str(&format!("export PATH=\"{}:$PATH\"\n", dir.display()));
        }
        out.push_str(&formatdoc! {r#"
            export MISE_SHELL=zsh
            export __MISE_ORIG_PATH="$PATH"

            mise() {{
              local command
              command="${{1:-}}"
              if [ "$#" = 0 ]; then
                command {exe}
                return
              fi
              shift

              case "$command" in
              deactivate|s|shell)
                # if argv doesn't contains -h,--help
                if [[ ! " $@ " =~ " --help " ]] && [[ ! " $@ " =~ " -h " ]]; then
                  eval "$(command {exe} "$command" "$@")"
                  return $?
                fi
                ;;
              esac
              command {exe} "$command" "$@"
            }}

            _mise_hook() {{
              eval "$({exe} hook-env{flags} -s zsh)";
            }}
            typeset -ag precmd_functions;
            if [[ -z "${{precmd_functions[(r)_mise_hook]+1}}" ]]; then
              precmd_functions=( _mise_hook ${{precmd_functions[@]}} )
            fi
            typeset -ag chpwd_functions;
            if [[ -z "${{chpwd_functions[(r)_mise_hook]+1}}" ]]; then
              chpwd_functions=( _mise_hook ${{chpwd_functions[@]}} )
            fi

            "#});
        if Settings::get().not_found_auto_install {
            out.push_str(&formatdoc! {r#"
            if [ -z "${{_mise_cmd_not_found:-}}" ]; then
                _mise_cmd_not_found=1
                [ -n "$(declare -f command_not_found_handler)" ] && eval "${{$(declare -f command_not_found_handler)/command_not_found_handler/_command_not_found_handler}}"

                function command_not_found_handler() {{
                    if {exe} hook-not-found -s zsh -- "$1"; then
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
        precmd_functions=( ${{precmd_functions:#_mise_hook}} )
        chpwd_functions=( ${{chpwd_functions:#_mise_hook}} )
        unset -f _mise_hook
        unset -f mise
        unset MISE_SHELL
        "#}
    }

    fn prepend_path(&self, paths: &[PathBuf]) -> String {
        if paths.is_empty() {
            return String::new();
        }
        let mut path = String::new();
        for p in paths {
            if is_dir_not_in_nix(p) && !is_dir_in_path(p) && !p.is_relative() {
                path = format!("{}:{path}", p.display());
            }
        }
        format!("export PATH=\"{path}$PATH\"\n")
    }

    fn set_env(&self, k: &str, v: &str) -> String {
        Bash::default().set_env(k, v)
    }

    fn unset_env(&self, k: &str) -> String {
        Bash::default().unset_env(k)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::replace_path;

    #[test]
    fn test_activate() {
        let zsh = Zsh::default();
        let exe = Path::new("/some/dir/mise");
        assert_snapshot!(zsh.activate(exe, " --status".into()));
    }

    #[test]
    fn test_activate_nix() {
        let zsh = Zsh::default();
        let exe = Path::new("/nix/store/mise");
        assert_snapshot!(zsh.activate(exe, " --status".into()));
    }

    #[test]
    fn test_prepend_path() {
        let zsh = Zsh::default();
        assert_snapshot!(zsh.prepend_path(&[PathBuf::from("/some/dir")]));
    }

    #[test]
    fn test_set_env() {
        assert_snapshot!(Zsh::default().set_env("FOO", "1"));
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
