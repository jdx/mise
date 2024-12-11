use std::fmt::Display;

use indoc::formatdoc;

use crate::shell::{ActivateOptions, Shell};

#[derive(Default)]
pub struct Nushell {}

enum EnvOp<'a> {
    Set { key: &'a str, val: &'a str },
    Hide { key: &'a str },
}

impl Display for EnvOp<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EnvOp::Set { key, val } => writeln!(f, "set,{key},{val}"),
            EnvOp::Hide { key } => writeln!(f, "hide,{key},"),
        }
    }
}

impl Nushell {
    fn escape_csv_value(s: &str) -> String {
        if s.contains(['\r', '\n', '"', ',']) {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_owned()
        }
    }
}

impl Shell for Nushell {
    fn activate(&self, opts: ActivateOptions) -> String {
        let exe = opts.exe;
        let flags = opts.flags;
        let exe = exe.to_string_lossy().replace('\\', r#"\\"#);

        formatdoc! {r#"
          export-env {{
            $env.MISE_SHELL = "nu"
            let mise_hook = {{
              condition: {{ "MISE_SHELL" in $env }}
              code: {{ mise_hook }}
            }}
            add-hook hooks.pre_prompt $mise_hook
            add-hook hooks.env_change.PWD $mise_hook
          }}

          def --env add-hook [field: cell-path new_hook: any] {{
            let old_config = $env.config? | default {{}}
            let old_hooks = $old_config | get $field --ignore-errors | default []
            $env.config = ($old_config | upsert $field ($old_hooks ++ $new_hook))
          }}

          def "parse vars" [] {{
            $in | from csv --noheaders --no-infer | rename 'op' 'name' 'value'
          }}

          export def --env --wrapped main [command?: string, --help, ...rest: string] {{
            let commands = ["deactivate", "shell", "sh"]

            if ($command == null) {{
              ^"{exe}"
            }} else if ($command == "activate") {{
              $env.MISE_SHELL = "nu"
            }} else if ($command in $commands) {{
              ^"{exe}" $command ...$rest
              | parse vars
              | update-env
            }} else {{
              ^"{exe}" $command ...$rest
            }}
          }}

          def --env "update-env" [] {{
            for $var in $in {{
              if $var.op == "set" {{
                load-env {{($var.name): $var.value}}
              }} else if $var.op == "hide" {{
                hide-env $var.name
              }}
            }}
          }}

          def --env mise_hook [] {{
            ^"{exe}" hook-env{flags} -s nu
              | parse vars
              | update-env
          }}

        "#}
    }

    fn deactivate(&self) -> String {
        [
            self.unset_env("MISE_SHELL"),
            self.unset_env("__MISE_DIFF"),
            self.unset_env("__MISE_DIFF"),
        ]
        .join("")
    }

    fn set_env(&self, k: &str, v: &str) -> String {
        let k = Nushell::escape_csv_value(k);
        let v = Nushell::escape_csv_value(v);

        EnvOp::Set { key: &k, val: &v }.to_string()
    }

    fn prepend_env(&self, k: &str, v: &str) -> String {
        format!("$env.{k} = ($env.{k} | prepend '{v}')\n")
    }

    fn unset_env(&self, k: &str) -> String {
        let k = Nushell::escape_csv_value(k);
        EnvOp::Hide { key: k.as_ref() }.to_string()
    }
}

impl Display for Nushell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "nu")
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
    fn test_hook_init() {
        let nushell = Nushell::default();
        let exe = Path::new("/some/dir/mise");
        let opts = ActivateOptions {
            exe: exe.to_path_buf(),
            flags: " --status".into(),
            no_hook_env: false,
        };
        assert_snapshot!(nushell.activate(opts));
    }

    #[test]
    fn test_set_env() {
        assert_snapshot!(Nushell::default().set_env("FOO", "1"));
    }

    #[test]
    fn test_prepend_env() {
        let sh = Nushell::default();
        assert_snapshot!(replace_path(&sh.prepend_env("PATH", "/some/dir:/2/dir")));
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
