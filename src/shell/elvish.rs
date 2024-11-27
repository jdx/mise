use std::path::Path;

use indoc::formatdoc;

use crate::shell::Shell;

#[derive(Default)]
pub struct Elvish {}

impl Shell for Elvish {
    fn activate(&self, exe: &Path, flags: String) -> String {
        let exe = exe.to_string_lossy();

        formatdoc! {r#"
            var hook-enabled = $false

            fn hook-env {{
              if $hook-enabled {{
                eval ({exe} hook-env{flags} -s elvish | slurp)
              }}
            }}

            set after-chdir = (conj $after-chdir {{|_| hook-env }})
            set edit:before-readline = (conj $edit:before-readline $hook-env~)

            fn activate {{
              set-env MISE_SHELL elvish
              set hook-enabled = $true
              hook-env
            }}

            fn deactivate {{
              set hook-enabled = $false
              eval ({exe} deactivate | slurp)
            }}

            fn mise {{|@a|
              if (== (count $a) 0) {{
                {exe}
                return
              }}

              if (not (or (has-value $a -h) (has-value $a --help))) {{
                var command = $a[0]
                if (==s $command shell) {{
                  try {{ eval ({exe} $@a) }} catch {{ }}
                  return
                }} elif (==s $command deactivate) {{
                  deactivate
                  return
                }} elif (==s $command activate) {{
                  activate
                  return
                }}
              }}
              {exe} $@a
            }}
            "#}
    }

    fn deactivate(&self) -> String {
        formatdoc! {r#"
            unset-env MISE_SHELL
            unset-env __MISE_DIFF
            unset-env __MISE_WATCH
        "#}
    }

    fn set_env(&self, k: &str, v: &str) -> String {
        let k = shell_escape::unix::escape(k.into());
        let v = shell_escape::unix::escape(v.into());
        let v = v.replace("\\n", "\n");
        format!("set-env {k} {v}\n")
    }

    fn prepend_env(&self, k: &str, v: &str) -> String {
        let k = shell_escape::unix::escape(k.into());
        let v = shell_escape::unix::escape(v.into());
        format!("set-env {k} {v}(get-env {k})\n")
    }

    fn unset_env(&self, k: &str) -> String {
        format!("unset-env {k}\n", k = shell_escape::unix::escape(k.into()))
    }
}

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;
    use test_log::test;

    use crate::test::replace_path;

    use super::*;

    #[test]
    fn test_hook_init() {
        let elvish = Elvish::default();
        let exe = Path::new("/some/dir/mise");
        assert_snapshot!(elvish.activate(exe, " --status".into()));
    }

    #[test]
    fn test_set_env() {
        assert_snapshot!(Elvish::default().set_env("FOO", "1"));
    }

    #[test]
    fn test_prepend_env() {
        let sh = Elvish::default();
        assert_snapshot!(replace_path(&sh.prepend_env("PATH", "/some/dir:/2/dir")));
    }

    #[test]
    fn test_unset_env() {
        assert_snapshot!(Elvish::default().unset_env("FOO"));
    }

    #[test]
    fn test_deactivate() {
        let deactivate = Elvish::default().deactivate();
        assert_snapshot!(replace_path(&deactivate));
    }
}
