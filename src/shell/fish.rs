use std::path::Path;

use indoc::formatdoc;

use crate::shell::{is_dir_in_path, Shell};

#[derive(Default)]
pub struct Fish {}

impl Shell for Fish {
    fn activate(&self, exe: &Path, status: bool) -> String {
        let dir = exe.parent().unwrap();
        let exe = exe.display();
        let status = if status { " --status" } else { "" };
        let description = "'Update rtx environment when changing directories'";
        let mut out = String::new();

        if !is_dir_in_path(dir) {
            out.push_str(&format!("fish_add_path -g {dir}\n", dir = dir.display()));
        }

        // much of this is from direnv
        // https://github.com/direnv/direnv/blob/cb5222442cb9804b1574954999f6073cc636eff0/internal/cmd/shell_fish.go#L14-L36
        out.push_str(&formatdoc! {r#"
            function __rtx_env_eval --on-event fish_prompt --description {description};
                {exe} hook-env{status} -s fish | source;

                if test "$rtx_fish_mode" != "disable_arrow";
                    function __rtx_cd_hook --on-variable PWD --description {description};
                        if test "$rtx_fish_mode" = "eval_after_arrow";
                            set -g __rtx_env_again 0;
                        else;
                            {exe} hook-env{status} -s fish | source;
                        end;
                    end;
                end;
            end;

            function __rtx_env_eval_2 --on-event fish_preexec --description {description};
                if set -q __rtx_env_again;
                    set -e __rtx_env_again;
                    {exe} hook-env{status} -s fish | source;
                    echo;
                end;

                functions --erase __rtx_cd_hook;
            end;

            __rtx_env_eval
        "#});

        out
    }

    fn deactivate(&self) -> String {
        formatdoc! {r#"
          functions --erase __rtx_env_eval;
          functions --erase __rtx_env_eval_2;
          functions --erase __rtx_cd_hook;
        "#}
    }

    fn set_env(&self, k: &str, v: &str) -> String {
        format!(
            "set -gx {k} {v}\n",
            k = shell_escape::unix::escape(k.into()),
            v = shell_escape::unix::escape(v.into())
        )
    }

    fn unset_env(&self, k: &str) -> String {
        format!("set -e {k}\n", k = shell_escape::unix::escape(k.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hook_init() {
        let fish = Fish::default();
        let exe = Path::new("/some/dir/rtx");
        insta::assert_snapshot!(fish.activate(exe, true));
    }

    #[test]
    fn test_set_env() {
        insta::assert_snapshot!(Fish::default().set_env("FOO", "1"));
    }

    #[test]
    fn test_unset_env() {
        insta::assert_snapshot!(Fish::default().unset_env("FOO"));
    }
}
