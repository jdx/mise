use std::path::Path;

use indoc::formatdoc;

use crate::shell::bash::Bash;
use crate::shell::{is_dir_in_path, Shell};

#[derive(Default)]
pub struct Zsh {}

impl Shell for Zsh {
    fn activate(&self, exe: &Path) -> String {
        let dir = exe.parent().unwrap();
        let exe = exe.display();
        let mut out = String::new();

        // much of this is from direnv
        // https://github.com/direnv/direnv/blob/cb5222442cb9804b1574954999f6073cc636eff0/internal/cmd/shell_zsh.go#L10-L22
        if !is_dir_in_path(dir) {
            out.push_str(&format!("export PATH=\"{}:$PATH\"\n", dir.display()));
        }
        out.push_str(&formatdoc! {r#"
            _rtx_hook() {{
              trap -- '' SIGINT;
              eval "$("{exe}" hook-env -s zsh)";
              trap - SIGINT;
            }}
            typeset -ag precmd_functions;
            if [[ -z "${{precmd_functions[(r)_rtx_hook]+1}}" ]]; then
              precmd_functions=( _rtx_hook ${{precmd_functions[@]}} )
            fi
            typeset -ag chpwd_functions;
            if [[ -z "${{chpwd_functions[(r)_rtx_hook]+1}}" ]]; then
              chpwd_functions=( _rtx_hook ${{chpwd_functions[@]}} )
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
        Bash::default().set_env(k, v)
    }

    fn unset_env(&self, k: &str) -> String {
        Bash::default().unset_env(k)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hook_init() {
        insta::assert_snapshot!(Zsh::default().activate(Path::new("/some/dir/rtx")));
    }

    #[test]
    fn test_set_env() {
        insta::assert_snapshot!(Zsh::default().set_env("FOO", "1"));
    }

    #[test]
    fn test_unset_env() {
        insta::assert_snapshot!(Zsh::default().unset_env("FOO"));
    }
}
