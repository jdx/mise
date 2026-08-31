#![allow(unknown_lints)]
use std::fmt::{Display, Formatter};

use crate::config::Settings;
use crate::env::{self};
use crate::shell::{self, ActivateOptions, Shell};
use indoc::formatdoc;
use itertools::Itertools;
use shell_escape::unix::escape;

#[derive(Default)]
pub(super) struct Fish {}

impl Fish {}

impl Shell for Fish {
    fn activate(&self, opts: ActivateOptions) -> String {
        let exe = opts.exe;
        let flags = opts.flags;

        let exe = escape(exe.to_string_lossy());
        let description = "'Update mise environment when changing directories'";
        let mut out = String::new();

        out.push_str(&shell::build_deactivation_script(self));

        out.push_str(&self.format_activate_prelude(&opts.prelude));

        // much of this is from direnv
        // https://github.com/direnv/direnv/blob/cb5222442cb9804b1574954999f6073cc636eff0/internal/cmd/shell_fish.go#L14-L36
        out.push_str(&formatdoc! {r#"
            set -gx MISE_SHELL fish
            if not set -q __MISE_ORIG_PATH
                set -gx __MISE_ORIG_PATH $PATH
            end

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
        "#});

        if !opts.no_hook_env {
            out.push_str(&formatdoc! {r#"

            function __mise_env_eval --description {description};
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

            function __mise_env_eval_on_prompt --on-event fish_prompt --description {description};
                if set -q __mise_skip_first_prompt_pwd;
                    set -l activate_pwd "$__mise_skip_first_prompt_pwd";
                    set -e __mise_skip_first_prompt_pwd;
                    if test "$PWD" = "$activate_pwd";
                        return;
                    end;
                end;

                __mise_env_eval;
            end;

            function __mise_env_eval_2 --on-event fish_preexec --description {description};
                if set -q __mise_env_again;
                    set -e __mise_env_again;
                    {exe} hook-env{flags} -s fish | source;
                    echo;
                end;

                if test "$mise_fish_mode" = "eval_after_arrow";
                    functions --erase __mise_cd_hook;
                end;
            end;

            __mise_env_eval
            set -g __mise_skip_first_prompt_pwd "$PWD"
        "#});
        }
        if Settings::get().not_found_auto_install {
            out.push_str(&formatdoc! {r#"
            if functions -q fish_command_not_found; and not functions -q __mise_fish_command_not_found
                functions -e __mise_fish_command_not_found
                functions -c fish_command_not_found __mise_fish_command_not_found
            end

            function fish_command_not_found
                if string match -qrv -- '^(?:mise$|mise-)' $argv[1] &&
                    {exe} hook-not-found -s fish -- $argv[1]
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
          functions --erase __mise_env_eval_on_prompt
          functions --erase __mise_env_eval_2
          functions --erase __mise_cd_hook
          functions --erase mise
          set -e __mise_skip_first_prompt_pwd
          set -e MISE_SHELL
          set -e __MISE_DIFF
          set -e __MISE_SESSION
        "#}
    }

    fn set_env(&self, key: &str, v: &str) -> String {
        let k = escape(key.into());
        // Fish keeps PATH as a list, so the value is split on the host's separator -- `;` on
        // Windows, `:` on unix -- the way `prepend_env` below already does it. Splitting on `:`
        // unconditionally severed every Windows drive letter from its path, and matching on the
        // literal name missed `Path`, which is how Windows itself spells it.
        //
        // Empty entries are dropped, as they are in `prepend_env` and in `env::split_colon_list`:
        // an empty element of a fish list is the current directory, and a Windows PATH ending in
        // `;` -- the usual shape -- yields one from `split_paths`.
        if env::is_path_key(key) {
            let paths = env::split_paths(v)
                .filter_map(|p| {
                    let p = p.to_string_lossy().into_owned();
                    if p.is_empty() {
                        None
                    } else {
                        Some(escape(p.into()))
                    }
                })
                .join(" ");
            format!("set -gx PATH {paths}\n")
        } else {
            let v = escape(v.into());
            format!("set -gx {k} {v}\n")
        }
    }

    fn prepend_env(&self, key: &str, value: &str) -> String {
        let k = escape(key.into());

        match key {
            env_key if env_key == *env::PATH_KEY => env::split_paths(value)
                .filter_map(|path| {
                    let path_str = path.to_str()?;
                    if path_str.is_empty() {
                        None
                    } else {
                        Some(format!(
                            "fish_add_path --global --path {}\n",
                            escape(path_str.into())
                        ))
                    }
                })
                .collect::<String>(),
            _ => {
                let v = escape(value.into());
                format!("set -gx {k} {v} ${k}\n")
            }
        }
    }

    fn move_prepend_env(&self, key: &str, value: &str) -> String {
        match key {
            env_key if env_key == *env::PATH_KEY => env::split_paths(value)
                .filter_map(|path| {
                    let path_str = path.to_str()?;
                    if path_str.is_empty() {
                        None
                    } else {
                        Some(format!(
                            "fish_add_path --global --move --path {}\n",
                            escape(path_str.into())
                        ))
                    }
                })
                .collect::<String>(),
            _ => self.prepend_env(key, value),
        }
    }

    fn supports_move_path(&self) -> bool {
        true
    }

    fn unset_env(&self, k: &str) -> String {
        format!("set -e {k}\n", k = escape(k.into()))
    }

    fn set_alias(&self, name: &str, cmd: &str) -> String {
        let name = escape(name.into());
        let cmd = escape(cmd.into());
        format!("complete -e {name}\nalias {name} {cmd}\n")
    }

    fn unset_alias(&self, name: &str) -> String {
        let name = escape(name.into());
        format!("complete -e {name}\nfunctions -e {name}\n")
    }
}

impl Display for Fish {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "fish")
    }
}

#[cfg(all(test, not(windows)))]
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

        let fish = Fish::default();
        let exe = Path::new("/some/dir/mise");
        let opts = ActivateOptions {
            exe: exe.to_path_buf(),
            flags: " --status".into(),
            no_hook_env: false,
            prelude: vec![],
        };
        assert_snapshot!(fish.activate(opts));
    }

    #[test]
    fn test_set_env() {
        assert_snapshot!(Fish::default().set_env("FOO", "1"));
    }

    /// The regression guard for the Windows fix. On unix `env::split_paths` splits on `:` and
    /// `env::is_path_key` is `key == "PATH"`, so both are the identity on what this did before --
    /// the only unix output that moves is a value with an empty segment, pinned below. Written
    /// out rather than snapshotted so the expected string is next to the claim.
    #[test]
    fn a_colon_list_still_becomes_one_fish_element_per_directory() {
        assert_eq!(
            Fish::default().set_env("PATH", "/some/dir:/2/dir"),
            "set -gx PATH /some/dir /2/dir\n"
        );
    }

    /// `Path` is PATH on Windows and a variable of its own on unix -- which is the distinction
    /// `env::is_path_key` exists to make. Without this, folding every spelling everywhere would
    /// look like a fix.
    #[test]
    fn another_spelling_of_path_is_not_path_here() {
        assert_eq!(
            Fish::default().set_env("Path", "/a:/b"),
            "set -gx Path '/a:/b'\n"
        );
    }

    /// The one place unix output does move. An empty element of a fish list is the current
    /// directory, so `PATH=/a::/b` used to put the cwd on PATH; `prepend_env` below and
    /// `env::split_colon_list` already drop these.
    #[test]
    fn an_empty_segment_does_not_become_the_current_directory() {
        assert_eq!(
            Fish::default().set_env("PATH", "/a::/b"),
            "set -gx PATH /a /b\n"
        );
    }

    #[test]
    fn test_prepend_env() {
        let sh = Fish::default();
        assert_snapshot!(replace_path(&sh.prepend_env("PATH", "/some/dir:/2/dir")));
    }

    #[test]
    fn test_move_prepend_env() {
        let sh = Fish::default();
        assert_snapshot!(replace_path(
            &sh.move_prepend_env("PATH", "/some/dir:/2/dir")
        ));
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

/// A sibling of the module above rather than tests inside it: that one is `not(windows)` whole,
/// and Windows is the platform these are about.
#[cfg(all(test, windows))]
mod windows_tests {
    use test_log::test;

    use super::*;

    /// The reproduction. `mise env -s fish` on Windows printed
    /// `set -gx PATH C '\a\bin;C' '\b\bin'` -- every drive letter severed from its path, because
    /// the split was on `:` while a Windows PATH is separated by `;`.
    #[test]
    fn a_windows_path_becomes_one_fish_element_per_directory() {
        assert_eq!(
            Fish::default().set_env("PATH", r"C:\a\bin;C:\b\bin"),
            concat!(r"set -gx PATH 'C:\a\bin' 'C:\b\bin'", "\n")
        );
    }

    /// The other half, and the reason for `env::is_path_key` rather than `key == "PATH"`:
    /// `Path` is how Windows itself writes it, and it names the same variable. Matching on the
    /// literal name left it in the scalar branch as `set -gx Path 'C:\a\bin;C:\b\bin'`.
    #[test]
    fn the_windows_spelling_of_path_takes_the_list_branch_too() {
        assert_eq!(
            Fish::default().set_env("Path", r"C:\a\bin;C:\b\bin"),
            concat!(r"set -gx PATH 'C:\a\bin' 'C:\b\bin'", "\n")
        );
    }

    /// A Windows PATH usually ends in `;`, and `split_paths` yields an empty entry for it --
    /// measured, not assumed. An empty element of a fish list is the current directory.
    #[test]
    fn a_trailing_separator_does_not_add_the_current_directory() {
        assert_eq!(
            Fish::default().set_env("PATH", r"C:\a\bin;"),
            concat!(r"set -gx PATH 'C:\a\bin'", "\n")
        );
    }

    /// The control. Only PATH is a list; any other variable holding a `;` has to survive whole,
    /// or this would read as "mise now splits everything on the path separator".
    #[test]
    fn a_semicolon_in_any_other_variable_is_left_alone() {
        assert_eq!(Fish::default().set_env("FOO", "a;b"), "set -gx FOO 'a;b'\n");
    }
}
