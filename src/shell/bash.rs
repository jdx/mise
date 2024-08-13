use std::path::Path;

use indoc::formatdoc;

use crate::config::Settings;
use crate::shell::Shell;

#[derive(Default)]
pub struct Bash {}

impl Shell for Bash {
    fn activate(&self, exe: &Path, flags: String) -> String {
        let settings = Settings::get();
        let exe = exe.to_string_lossy();
        let mut out = formatdoc! {r#"
            export MISE_SHELL=bash
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
              local previous_exit_status=$?;
              eval "$(mise hook-env{flags} -s bash)";
              return $previous_exit_status;
            }};
            if [[ ";${{PROMPT_COMMAND:-}};" != *";_mise_hook;"* ]]; then
              PROMPT_COMMAND="_mise_hook${{PROMPT_COMMAND:+;$PROMPT_COMMAND}}"
            fi
            "#};
        if settings.not_found_auto_install {
            out.push_str(&formatdoc! {r#"
            if [ -z "${{_mise_cmd_not_found:-}}" ]; then
                _mise_cmd_not_found=1
                if [ -n "$(declare -f command_not_found_handle)" ]; then
                    _mise_cmd_not_found_handle=$(declare -f command_not_found_handle)
                    eval "${{_mise_cmd_not_found_handle/command_not_found_handle/_command_not_found_handle}}"
                fi

                command_not_found_handle() {{
                    if {exe} hook-not-found -s bash -- "$1"; then
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
            PROMPT_COMMAND="${{PROMPT_COMMAND//_mise_hook;/}}"
            PROMPT_COMMAND="${{PROMPT_COMMAND//_mise_hook/}}"
            unset _mise_hook
            unset mise
            unset MISE_SHELL
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
}

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;
    use test_log::test;

    use crate::test::{replace_path, reset};

    use super::*;

    #[test]
    fn test_activate() {
        reset();
        let bash = Bash::default();
        let exe = Path::new("/some/dir/mise");
        assert_snapshot!(bash.activate(exe, " --status".into()));
    }

    #[test]
    fn test_set_env() {
        reset();
        assert_snapshot!(Bash::default().set_env("FOO", "1"));
    }

    #[test]
    fn test_prepend_env() {
        reset();
        let bash = Bash::default();
        assert_snapshot!(replace_path(&bash.prepend_env("PATH", "/some/dir:/2/dir")));
    }

    #[test]
    fn test_unset_env() {
        reset();
        assert_snapshot!(Bash::default().unset_env("FOO"));
    }

    #[test]
    fn test_deactivate() {
        reset();
        let deactivate = Bash::default().deactivate();
        assert_snapshot!(replace_path(&deactivate));
    }
}
