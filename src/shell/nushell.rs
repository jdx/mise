use std::fmt::Display;
use std::path::Path;

use indoc::formatdoc;

use crate::shell::Shell;

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
    fn activate(&self, exe: &Path, flags: String) -> String {
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
            let commands = ["shell", "deactivate"]

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
        self.unset_env("MISE_SHELL")
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

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;
    use test_log::test;

    use crate::test::{replace_path, reset};

    use super::*;

    #[test]
    fn test_hook_init() {
        reset();
        let nushell = Nushell::default();
        let exe = Path::new("/some/dir/mise");
        assert_snapshot!(nushell.activate(exe, " --status".into()));
    }

    #[test]
    fn test_set_env() {
        reset();
        assert_snapshot!(Nushell::default().set_env("FOO", "1"));
    }

    #[test]
    fn test_prepend_env() {
        reset();
        let sh = Nushell::default();
        assert_snapshot!(replace_path(&sh.prepend_env("PATH", "/some/dir:/2/dir")));
    }

    #[test]
    fn test_unset_env() {
        reset();
        assert_snapshot!(Nushell::default().unset_env("FOO"));
    }

    #[test]
    fn test_deactivate() {
        reset();
        let deactivate = Nushell::default().deactivate();
        assert_snapshot!(replace_path(&deactivate));
    }
}
