use std::path::Path;

use indoc::formatdoc;

use crate::shell::{is_dir_in_path, Shell};

#[derive(Default)]
pub struct Bash {}

impl Shell for Bash {
    fn activate(&self, exe: &Path, status: bool) -> String {
        let dir = exe.parent().unwrap();
        let exe = exe.display();
        let status = if status { " --status" } else { "" };
        let mut out = String::new();
        if !is_dir_in_path(dir) {
            out.push_str(&format!("export PATH=\"{}:$PATH\"\n", dir.display()));
        }
        out.push_str(&formatdoc! {r#"
            export RTX_SHELL=bash

            rtx() {{
              local command
              command="${{1:-}}"
              if [ "$#" = 0 ]; then
                command {exe}
                return
              fi
              shift

              case "$command" in
              deactivate|shell)
                eval "$({exe} "$command" "$@")"
                ;;
              *)
                command {exe} "$command" "$@"
                ;;
              esac
            }}

            _rtx_hook() {{
              local previous_exit_status=$?;
              trap -- '' SIGINT;
              eval "$("{exe}" hook-env{status} -s bash)";
              trap - SIGINT;
              return $previous_exit_status;
            }};
            if ! [[ "${{PROMPT_COMMAND:-}}" =~ _rtx_hook ]]; then
              PROMPT_COMMAND="_rtx_hook${{PROMPT_COMMAND:+;$PROMPT_COMMAND}}"
            fi

            _rtx_hook
            "#});

        out
    }

    fn deactivate(&self) -> String {
        formatdoc! {r#"
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
    use super::*;
    use crate::test::replace_path;
    use insta::assert_snapshot;

    #[test]
    fn test_hook_init() {
        let bash = Bash::default();
        let exe = Path::new("/some/dir/rtx");
        assert_snapshot!(bash.activate(exe, true));
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
