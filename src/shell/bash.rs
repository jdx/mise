use crate::config::Settings;
use std::path::Path;

use crate::shell::{is_dir_in_path, is_dir_not_in_nix, Shell};

#[derive(Default)]
pub struct Bash {}

impl Shell for Bash {
    fn activate(&self, exe: &Path, flags: String) -> String {
        let dir = exe.parent().unwrap();
        let exe = exe.to_string_lossy();
        let mut out = String::new();
        if is_dir_not_in_nix(dir) && !is_dir_in_path(dir) {
            out.push_str(&format!("export PATH=\"{}:$PATH\"\n", dir.display()));
        }
        out.push_str(&formatdoc! {r#"
            export RTX_SHELL=bash
            export __RTX_ORIG_PATH="$PATH"

            rtx() {{
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

            _rtx_hook() {{
              local previous_exit_status=$?;
              eval "$(rtx hook-env{flags} -s bash)";
              return $previous_exit_status;
            }};
            if [[ ";${{PROMPT_COMMAND:-}};" != *";_rtx_hook;"* ]]; then
              PROMPT_COMMAND="_rtx_hook${{PROMPT_COMMAND:+;$PROMPT_COMMAND}}"
            fi
            "#});
        if Settings::get().not_found_auto_install {
            out.push_str(&formatdoc! {r#"
            eval 'command_not_found_handle() {{
            local ret
              if (( ! IN_CNFH++)) && {exe} hook-not-found -s bash "$1"; then
                _rtx_hook
                "$@"
                ret=$?
                (( IN_CNFH-- ))
                return "$ret"
              fi
              (( IN_CNFH-- ))
              '"$(typeset -f command_not_found_handle | tail -n +2)"'
            }}'
            "#});
        }

        out
    }

    fn deactivate(&self) -> String {
        formatdoc! {r#"
            PROMPT_COMMAND="${{PROMPT_COMMAND//_rtx_hook;/}}"
            PROMPT_COMMAND="${{PROMPT_COMMAND//_rtx_hook/}}"
            unset _rtx_hook
            unset rtx
            unset RTX_SHELL
        "#}
    }

    fn set_env(&self, k: &str, v: &str) -> String {
        let k = shell_escape::unix::escape(k.into());
        let v = shell_escape::unix::escape(v.into());
        let v = v.replace("\\n", "\n");
        format!("export {k}={v}\n")
    }

    fn unset_env(&self, k: &str) -> String {
        format!("unset {k}\n", k = shell_escape::unix::escape(k.into()))
    }
}

#[cfg(test)]
mod tests {

    use crate::test::replace_path;

    use super::*;

    #[test]
    fn test_hook_init() {
        let bash = Bash::default();
        let exe = Path::new("/some/dir/rtx");
        assert_snapshot!(bash.activate(exe, " --status".into()));
    }

    #[test]
    fn test_hook_init_nix() {
        let bash = Bash::default();
        let exe = Path::new("/nix/store/rtx");
        assert_snapshot!(bash.activate(exe, " --status".into()));
    }

    #[test]
    fn test_set_env() {
        assert_snapshot!(Bash::default().set_env("FOO", "1"));
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
