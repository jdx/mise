use std::path::Path;

use indoc::formatdoc;

use crate::shell::{is_dir_in_path, Shell};

#[derive(Default)]
pub struct Nushell {}

impl Shell for Nushell {
    fn activate(&self, exe: &Path, status: bool) -> String {
        let dir = exe.parent().unwrap();
        let exe = exe.display();
        let status = if status { " --status" } else { "" };
        let mut out = String::new();

        if !is_dir_in_path(dir) {
            out.push_str(&format!(
                "let-env PATH = ($env.PATH | prepend '{}')\n", // TODO: set PATH as Path on windows
                dir.display()
            ));
        }

        out.push_str(&formatdoc! {r#"
          let-env RTX_SHELL = "nu"

          def rtx [command?: string, ...rest: string] {{
            let commands = ["shell", "deactivate"]
            if ($command == null) {{
              run-external {exe}
            }} else if ($command in $commands) {{
              run-external {exe} $command $rest --redirect-stdout | nu -e $in
            }} else {{
              run-external {exe} $command $rest
            }}
          }}
          
          def-env _rtx_hook [] {{
            let lines = (^"{exe}" hook-env{status} -s nu | lines | parse "{{name}} = {{value}}")
            
            if ($lines | is-empty) {{
              return
            }}
          
            let paths = ($lines | find PATH)
          
            let rejector = if ($paths | length | $in > 1) {{
              $paths.0.value
            }} else {{
              null
            }}
          
            if not ($rejector == null) {{
              $lines | where value != $rejector | transpose -i -r -d | load-env
            }} else {{
              $lines | transpose -i -r -d | load-env
            }}
          }}
          
          let-env config = ($env.config | upsert hooks {{
              pre_prompt: {{ _rtx_hook }}
              env_change: {{
                  PWD: [{{
                    |before, after| _rtx_hook
                  }}]
              }}
          }})
            "#});

        out
    }

    // TODO: properly handle deactivate
    fn deactivate(&self) -> String {
        formatdoc! {r#"
          print $"TODO: properly deactivate nushell"
        "#}
    }

    fn set_env(&self, k: &str, v: &str) -> String {
        let k = shell_escape::unix::escape(k.into());
        let v = shell_escape::unix::escape(v.into());
        let v = v.replace("\\n", "\n");
        let v = v.replace('\'', "");

        format!("{k} = {v}\n")
    }

    fn unset_env(&self, k: &str) -> String {
        format!("{k} = null \n", k = shell_escape::unix::escape(k.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::replace_path;
    use insta::assert_snapshot;

    #[test]
    fn test_hook_init() {
        let nushell = Nushell::default();
        let exe = Path::new("/some/dir/rtx");
        assert_snapshot!(nushell.activate(exe, true));
    }

    #[test]
    fn test_set_env() {
        assert_snapshot!(Nushell::default().set_env("FOO", "1"));
    }

    #[test]
    fn test_unset_env() {
        assert_snapshot!(Nushell::default().unset_env("FOO"));
    }

    #[test]
    fn test_deactivate() {
        let deactivate = Nushell::default().deactivate();
        assert_snapshot!(replace_path(&deactivate));
    }
}
