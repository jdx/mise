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
          
          def "parse vars" [] {{
            $in | lines | parse "{{name}} = {{value}}"
          }}
            
          def "format vars" [] {{
            $in | reverse | uniq-by name | transpose -i -r -d
          }}

          def-env rtx [command?: string, ...rest: string] {{
            let commands = ["shell", "deactivate"]
            
            if ($command == null) {{
              run-external {exe}
            }} else if ($command == "activate") {{
              let-env RTX_SHELL = "nu"
            }} else if ($command in $commands) {{
              let vars = (^"{exe}" $command $rest
                | parse vars )
              
              if not ($vars | is-empty) {{
                $vars | format vars | load-env
              }}
            }} else {{
              run-external {exe} $command $rest
            }}
          }}
          
          def-env rtx_hook [] {{
            let vars = (^"{exe}" hook-env{status} $command $rest
              | parse vars )
            
            if not ($vars | is-empty) {{
              $vars | format vars | load-env| load-env
            }}
          }}
          
          let-env config = ($env.config | upsert hooks {{
            pre_prompt: [{{
              condition: {{ $env.RTX_SHELL != "null" }}
              code: {{ rtx_hook }}
            }}]
            env_change: {{
                PWD: [{{
                  condition: {{ $env.RTX_SHELL != "null" }}
                  code: {{ rtx_hook }}
                }}]
            }}
          }})
            "#});

        out
    }

    // TODO: properly handle deactivate
    fn deactivate(&self) -> String {
        formatdoc! {r#"
          RTX_SHELL = null
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
