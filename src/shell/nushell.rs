#![allow(unknown_lints)]
use std::fmt::Display;

use indoc::formatdoc;

use crate::{
    env,
    shell::{self, ActivateOptions, ActivatePrelude, PORTABLE_HOME_VAR, PortablePath, Shell},
};
use itertools::Itertools;

#[derive(Default)]
pub(super) struct Nushell {}

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

    /// Quote `s` for Nushell source. Raw strings cannot hold `#` at all —
    /// any `#` inside breaks out of the literal — so values with `#` use a
    /// double-quoted string with `\`, `"`, and `$` escaped instead.
    fn nu_string(s: &str) -> String {
        if s.contains('#') {
            format!(
                "\"{}\"",
                s.replace('\\', "\\\\")
                    .replace('"', "\\\"")
                    .replace('$', "\\$")
            )
        } else {
            format!("r#'{s}'#")
        }
    }

    fn format_activate_prelude_inline(&self, prelude: &[ActivatePrelude]) -> String {
        prelude
            .iter()
            .map(|p| match p {
                ActivatePrelude::Set(k, v) if env::is_path_key(k) => {
                    format!(
                        "$env.{k} = ({} | split row (char esep))\n",
                        Self::nu_string(v)
                    )
                }
                ActivatePrelude::Set(k, v) => format!("$env.{k} = {}\n", Self::nu_string(v)),
                ActivatePrelude::Prepend(k, v) | ActivatePrelude::MovePrepend(k, v) => {
                    self.prepend_env(k, v)
                }
                ActivatePrelude::Raw(s) => s.clone(),
            })
            .join("")
    }

    fn build_deactivation_script(&self) -> String {
        let deactivation_ops = shell::build_deactivation_script(self);
        deactivation_ops.trim_end_matches('\n').to_owned()
    }
}

impl Shell for Nushell {
    fn activate(&self, opts: ActivateOptions) -> String {
        let exe = opts.exe;
        let flags = opts.flags;
        let exe = exe.to_string_lossy().replace('\\', r#"\\"#);

        let mut out = String::new();

        out.push_str(&formatdoc! {r#"
          def "parse vars" [] {{
            $in | from csv --noheaders --no-infer | rename 'op' 'name' 'value'
          }}

          def --env "update-env" [] {{
            for $var in $in {{
              if $var.op == "set" {{
                if ($var.name =~ '(?i)^path$') {{
                  $env.PATH = ($var.value | split row (char esep))
                }} else {{
                  load-env {{($var.name): $var.value}}
                }}
              }} else if $var.op == "hide" {{
                try {{ hide-env $var.name }}
              }}
            }}
          }}
        "#});

        let deactivation_ops_csv = self.build_deactivation_script();
        let inline_prelude = self.format_activate_prelude_inline(&opts.prelude);
        out.push_str(&formatdoc! {r#"
          export-env {{
            {inline_prelude}
            '{deactivation_ops_csv}' | parse vars | update-env
            $env.MISE_SHELL = "nu"
            let mise_hook = {{
              condition: {{ "MISE_SHELL" in $env }}
              code: {{ mise_hook }}
            }}
            add-hook hooks.pre_prompt $mise_hook
            add-hook hooks.env_change.PWD $mise_hook
          }}

          def --env add-hook [field: cell-path new_hook: any] {{
            let field = $field | split cell-path | update optional true | into cell-path
            let old_config = $env.config? | default {{}}
            let old_hooks = $old_config | get $field | default []
            $env.config = ($old_config | upsert $field ($old_hooks ++ [$new_hook]))
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

          def --env mise_hook [] {{
            ^"{exe}" hook-env{flags} -s nu
              | parse vars
              | update-env
          }}

        "#});
        out
    }

    fn deactivate(&self) -> String {
        [
            self.unset_env("MISE_SHELL"),
            self.unset_env("__MISE_DIFF"),
            self.unset_env("__MISE_SESSION"),
            self.unset_env("__MISE_HOME"),
        ]
        .join("")
    }

    fn set_env(&self, k: &str, v: &str) -> String {
        let k = Nushell::escape_csv_value(k);
        let v = Nushell::escape_csv_value(v);

        EnvOp::Set { key: &k, val: &v }.to_string()
    }

    fn render_portable_home_init(&self) -> String {
        // An env var rather than `mut`: `use`/`source` re-runs this file and
        // redeclaring a `mut` would fail, while assignment is always safe.
        // `'~' | path expand` resolves even with neither HOME nor USERPROFILE
        // set (a bare `~` would parse as an external command).
        format!(
            "$env.{var} = if (\"HOME\" in $env) and ($env.HOME != \"\" and $env.HOME != \"~\") {{ $env.HOME }} else if (\"USERPROFILE\" in $env) and ($env.USERPROFILE != \"\" and $env.USERPROFILE != \"~\") {{ $env.USERPROFILE }} else {{ '~' | path expand }}\n",
            var = PORTABLE_HOME_VAR,
        )
    }

    fn render_portable_path_block(&self, key: &str, front: &[PortablePath]) -> String {
        debug_assert!(!front.is_empty());
        let var = PORTABLE_HOME_VAR;
        let render = |e: &PortablePath| match &e.home_suffix {
            Some(suffix) => {
                let sep = if cfg!(windows) { '\\' } else { '/' };
                format!(
                    "($env.{var} + {})",
                    Self::nu_string(&format!("{sep}{suffix}"))
                )
            }
            None => Self::nu_string(&e.absolute),
        };
        // Rebuild rather than prepend: always idempotent and order-stable,
        // however often a saved snapshot is sourced.
        let list = front.iter().map(render).collect::<Vec<_>>().join(", ");
        let keep = format!("{{|p| $p not-in [{list}]}}");
        format!("$env.{key} = ([{list}] | append ($env.{key} | where {keep}))\n")
    }

    fn render_orig_path_init(&self) -> String {
        "if (\"__MISE_ORIG_PATH\" not-in $env) { $env.__MISE_ORIG_PATH = $env.PATH }\n".to_string()
    }

    fn prepend_env(&self, k: &str, v: &str) -> String {
        format!("$env.{k} = ($env.{k} | prepend {})\n", Self::nu_string(v))
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
            prelude: vec![],
        };
        assert_snapshot!(nushell.activate(opts));
    }

    #[test]
    fn test_set_env() {
        assert_snapshot!(Nushell::default().set_env("FOO", "1"));
    }

    #[test]
    fn test_format_activate_prelude_path() {
        let nushell = Nushell::default();
        let path_key = if cfg!(windows) { "Path" } else { "PATH" };
        let prelude = vec![ActivatePrelude::Set(
            path_key.to_string(),
            "/one:/two".to_string(),
        )];

        assert_eq!(
            nushell.format_activate_prelude_inline(&prelude),
            format!("$env.{path_key} = (r#'/one:/two'# | split row (char esep))\n")
        );
    }

    #[test]
    fn test_prepend_env() {
        let sh = Nushell::default();
        assert_snapshot!(replace_path(&sh.prepend_env("PATH", "/some/dir:/2/dir")));
    }

    /// Values holding `#` cannot use raw strings at all, so they render
    /// double-quoted with `\`, `"`, and `$` escaped instead.
    #[test]
    fn strings_with_hashes_use_escaped_quotes() {
        assert_eq!(Nushell::nu_string("/plain/path"), "r#'/plain/path'#");
        assert_eq!(Nushell::nu_string("/a'#b"), "\"/a'#b\"");
        assert_eq!(Nushell::nu_string("/a#$b"), "\"/a#\\$b\"");
        // A lone `'` is fine inside a raw string; only `#` forces quotes.
        assert_eq!(Nushell::nu_string("C:\\a'b"), "r#'C:\\a'b'#");
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
