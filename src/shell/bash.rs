#![allow(unknown_lints)]
#![allow(clippy::literal_string_with_formatting_args)]
use std::fmt::Display;

use shell_escape::unix::escape;

use crate::config::Settings;
use crate::shell::{self, ActivateOptions, Shell};

#[derive(Default)]
pub struct Bash {}

impl Bash {}

fn render_template(template: &str, replacements: &[(&str, &str)]) -> String {
    let mut out = template.to_owned();
    for (needle, value) in replacements {
        out = out.replace(needle, value);
    }
    out
}

fn render_flags_array(value: &str) -> String {
    shell_words::split(value)
        .expect("failed to split activation flags")
        .into_iter()
        .map(|word| escape(word.into()).to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

impl Shell for Bash {
    fn activate(&self, opts: ActivateOptions) -> String {
        let exe = opts.exe;
        let settings = Settings::get();

        let exe = escape(exe.to_string_lossy());
        let flags = render_flags_array(&opts.flags);

        let mut out = String::new();

        out.push_str(&shell::build_deactivation_script(self));

        out.push_str(&self.format_activate_prelude(&opts.prelude));
        let activate = render_template(
            include_str!("../assets/bash/activate.sh"),
            &[
                ("__MISE_EXE_VALUE__", &exe),
                ("__MISE_FLAGS_VALUE__", &flags),
                (
                    "__MISE_HOOK_ENABLED_VALUE__",
                    if opts.no_hook_env { "0" } else { "1" },
                ),
                (
                    "__MISE_CHPWD_FUNCTIONS__",
                    include_str!("../assets/bash_zsh_support/chpwd/function.sh"),
                ),
                (
                    "__MISE_CHPWD_LOAD__",
                    include_str!("../assets/bash_zsh_support/chpwd/load.sh"),
                ),
            ],
        );
        out.push_str(&activate);
        out.push('\n');
        if settings.not_found_auto_install {
            let not_found = render_template(
                include_str!("../assets/bash/command_not_found.sh"),
                &[("__MISE_EXE__", &exe)],
            );
            out.push_str(&not_found);
            out.push('\n');
        }

        out
    }

    fn deactivate(&self) -> String {
        include_str!("../assets/bash/deactivate.sh").to_string()
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

    fn set_alias(&self, name: &str, cmd: &str) -> String {
        let name = shell_escape::unix::escape(name.into());
        let cmd = shell_escape::unix::escape(cmd.into());
        format!("alias {name}={cmd}\n")
    }

    fn unset_alias(&self, name: &str) -> String {
        let name = shell_escape::unix::escape(name.into());
        format!("unalias {name} 2>/dev/null || true\n")
    }
}

impl Display for Bash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "bash")
    }
}

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;
    use std::path::Path;
    use test_log::test;

    use crate::test::replace_path;

    use super::*;

    #[test]
    fn test_activate() {
        unsafe {
            std::env::remove_var("__MISE_ORIG_PATH");
            std::env::remove_var("__MISE_DIFF");
        }

        let bash = Bash::default();
        let exe = Path::new("/some/dir/mise");
        let opts = ActivateOptions {
            exe: exe.to_path_buf(),
            flags: " --status".into(),
            no_hook_env: false,
            prelude: vec![],
        };
        assert_snapshot!(bash.activate(opts));
    }

    #[test]
    fn test_set_env() {
        assert_snapshot!(Bash::default().set_env("FOO", "1"));
    }

    #[test]
    fn test_prepend_env() {
        let bash = Bash::default();
        assert_snapshot!(replace_path(&bash.prepend_env("PATH", "/some/dir:/2/dir")));
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
