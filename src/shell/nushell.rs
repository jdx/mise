use std::{fmt::Display, path::Path};

use indoc::formatdoc;

use crate::shell::{is_dir_in_path, Shell};

#[derive(Default)]
pub struct Nushell {}

enum EnvOp<'a> {
    Set { key: &'a str, val: &'a str },
    Hide { key: &'a str },
}

impl<'a> Display for EnvOp<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        #[allow(clippy::write_with_newline)]
        match self {
            EnvOp::Set { key, val } => write!(f, "set,{key},{val}\n"),
            EnvOp::Hide { key } => write!(f, "hide,{key},\n"),
        }
    }
}

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
          export-env {{
            let-env RTX_SHELL = "nu"
            
            let-env config = ($env.config | upsert hooks {{
                pre_prompt: [{{
                condition: {{ "RTX_SHELL" in $env }}
                code: {{ rtx_hook }}
                }}]
                env_change: {{
                    PWD: [{{
                    condition: {{ "RTX_SHELL" in $env }}
                    code: {{ rtx_hook }}
                    }}]
                }}
            }})
          }}
            
          def "parse vars" [] {{
            $in | lines | parse "{{op}},{{name}},{{value}}"
          }}
            
          def "format vars" [] {{
            $in | reverse | uniq-by name | transpose -i -r -d
          }}
            
          def-env rtx [command?: string, --help, ...rest: string] {{
            let commands = ["shell", "deactivate"]
            
            if ($command == null) {{
                run-external {exe}
            }} else if ($command == "activate") {{
                let-env RTX_SHELL = "nu"
            }} else if ($command in $commands) {{
                let vars = (^"{exe}" $command $rest
                | parse vars )
                
                $vars | process load | handle load
                $vars | process hide | handle hide
            }} else {{
                run-external {exe} $command $rest
            }}
          }}
            
          def "process load" [] {{
            $in | filter {{ |var| $var.op == "set" }} | reject op
          }}
            
          def-env "handle load" [] {{
            if not ($in | is-empty) {{
                $in | format vars | load-env
            }}
          }}
            
          def-env "handle hide" [] {{
            if not ($in | is-empty) {{
              for $var in $in {{
                hide-env $var.name
              }}
            }}
          }}
            
          def "process hide" [] {{
            $in | filter {{ |var| $var.op == "hide" }} | reject op | reject value
          }}
            
          def-env rtx_hook [] {{
            let vars = (^"{exe}" hook-env{status} -s nu
                | parse vars )
            $vars | process load | handle load
            $vars | process hide | handle hide
          }}

        "#});

        out
    }

    fn deactivate(&self) -> String {
        self.unset_env("RTX_SHELL")
    }

    fn set_env(&self, k: &str, v: &str) -> String {
        let k = shell_escape::unix::escape(k.into());
        let v = shell_escape::unix::escape(v.into());
        let v = v.replace("\\n", "\n");
        let v = v.replace('\'', "");

        EnvOp::Set { key: &k, val: &v }.to_string()
    }

    fn unset_env(&self, k: &str) -> String {
        let k = shell_escape::unix::escape(k.into());
        EnvOp::Hide { key: k.as_ref() }.to_string()
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
