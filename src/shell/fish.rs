use std::path::Path;

use crate::config::Settings;
use crate::shell::Shell;
use indoc::formatdoc;
use shell_escape::unix::escape;

#[derive(Default)]
pub struct Fish {}

impl Shell for Fish {
    fn activate(&self, exe: &Path, flags: String) -> String {
        let exe = exe.to_string_lossy();
        let description = "'Update mise environment when changing directories'";
        let mut out = String::new();

        // much of this is from direnv
        // https://github.com/direnv/direnv/blob/cb5222442cb9804b1574954999f6073cc636eff0/internal/cmd/shell_fish.go#L14-L36
        out.push_str(&formatdoc! {r#"
            set -gx MISE_SHELL fish
            set -gx __MISE_ORIG_PATH $PATH

            function mise
              if test (count $argv) -eq 0
                command {exe}
                return
              end

              set command $argv[1]
              set -e argv[1]

              if contains -- --help $argv
                command {exe} "$command" $argv
                return $status
              end

              switch "$command"
              case deactivate shell sh
                # if help is requested, don't eval
                if contains -- -h $argv
                  command {exe} "$command" $argv
                else if contains -- --help $argv
                  command {exe} "$command" $argv
                else
                  source (command {exe} "$command" $argv |psub)
                end
              case '*'
                command {exe} "$command" $argv
              end
            end

            function __mise_env_eval --on-event fish_prompt --description {description};
                {exe} hook-env{flags} -s fish | source;

                if test "$mise_fish_mode" != "disable_arrow";
                    function __mise_cd_hook --on-variable PWD --description {description};
                        if test "$mise_fish_mode" = "eval_after_arrow";
                            set -g __mise_env_again 0;
                        else;
                            {exe} hook-env{flags} -s fish | source;
                        end;
                    end;
                end;
            end;

            function __mise_env_eval_2 --on-event fish_preexec --description {description};
                if set -q __mise_env_again;
                    set -e __mise_env_again;
                    {exe} hook-env{flags} -s fish | source;
                    echo;
                end;

                functions --erase __mise_cd_hook;
            end;

            __mise_env_eval
        "#});
        if Settings::get().not_found_auto_install {
            out.push_str(&formatdoc! {r#"
            if functions -q fish_command_not_found; and not functions -q __mise_fish_command_not_found
                functions -e __mise_fish_command_not_found
                functions -c fish_command_not_found __mise_fish_command_not_found
            end

            function fish_command_not_found
                if {exe} hook-not-found -s fish -- $argv[1]
                    {exe} hook-env{flags} -s fish | source
                else if functions -q __mise_fish_command_not_found
                    __mise_fish_command_not_found $argv
                else
                    __fish_default_command_not_found_handler $argv
                end
            end
            "#});
        }

        out
    }

    fn deactivate(&self) -> String {
        formatdoc! {r#"
          functions --erase __mise_env_eval
          functions --erase __mise_env_eval_2
          functions --erase __mise_cd_hook
          functions --erase mise
          set -e MISE_SHELL
          set -e __MISE_DIFF
          set -e __MISE_WATCH
        "#}
    }

    fn set_env(&self, key: &str, v: &str) -> String {
        let k = escape(key.into());
        let v = escape(v.into());
        format!("set -gx {k} {v}\n")
    }

    fn prepend_env(&self, key: &str, v: &str) -> String {
        let k = escape(key.into());
        let v = escape(v.into());
        format!("set -gx {k} {v} ${k}\n")
    }

    fn unset_env(&self, k: &str) -> String {
        format!("set -e {k}\n", k = escape(k.into()))
    }
}

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;
    use test_log::test;

    use crate::test::replace_path;

    use super::*;

    #[test]
    fn test_activate() {
        let fish = Fish::default();
        let exe = Path::new("/some/dir/mise");
        assert_snapshot!(fish.activate(exe, " --status".into()));
    }

    #[test]
    fn test_set_env() {
        assert_snapshot!(Fish::default().set_env("FOO", "1"));
    }

    #[test]
    fn test_prepend_env() {
        let sh = Fish::default();
        assert_snapshot!(replace_path(&sh.prepend_env("PATH", "/some/dir:/2/dir")));
    }

    #[test]
    fn test_unset_env() {
        assert_snapshot!(Fish::default().unset_env("FOO"));
    }

    #[test]
    fn test_deactivate() {
        let deactivate = Fish::default().deactivate();
        assert_snapshot!(replace_path(&deactivate));
    }
}
