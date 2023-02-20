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
            "#});

        out
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
        let bash = Bash::default();
        let exe = Path::new("/some/dir/rtx");
        insta::assert_snapshot!(bash.activate(exe, true));
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
