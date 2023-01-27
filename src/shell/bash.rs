use crate::shell::Shell;
use indoc::formatdoc;
use std::path::Path;

#[derive(Default)]
pub struct Bash {}

impl Shell for Bash {
    fn activate(&self, exe: &Path) -> String {
        let dir = exe.parent().unwrap().display();
        let exe = exe.display();
        formatdoc! {r#"
            export PATH="{dir}:$PATH";
            _rtx_hook() {{
              local previous_exit_status=$?;
              trap -- '' SIGINT;
              eval "$("{exe}" hook-env -s bash)";
              trap - SIGINT;
              return $previous_exit_status;
            }};
            if ! [[ "${{PROMPT_COMMAND:-}}" =~ _rtx_hook ]]; then
              PROMPT_COMMAND="_rtx_hook${{PROMPT_COMMAND:+;$PROMPT_COMMAND}}"
            fi
        "#}
    }

    fn deactivate(&self) -> String {
        formatdoc! {r#"
            unset _rtx_hook;
        "#}
    }

    fn set_env(&self, k: &str, v: &str) -> String {
        format!(
            "export {k}={v}\n",
            k = shell_escape::unix::escape(k.into()),
            v = shell_escape::unix::escape(v.into())
        )
    }

    fn unset_env(&self, k: &str) -> String {
        format!("unset {k}\n", k = shell_escape::unix::escape(k.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hook_init() {
        insta::assert_snapshot!(Bash::default().activate(Path::new("rtx")));
    }

    #[test]
    fn test_set_env() {
        insta::assert_snapshot!(Bash::default().set_env("FOO", "1"));
    }

    #[test]
    fn test_unset_env() {
        insta::assert_snapshot!(Bash::default().unset_env("FOO"));
    }
}
