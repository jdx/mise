#![allow(unknown_lints)]
use std::fmt::Display;

use indoc::formatdoc;

use crate::{
    env,
    shell::{ActivateOptions, ActivatePrelude, Shell},
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
    /// `s` as a Nushell raw string. A raw string has no escapes and ends at the first `'`
    /// followed by as many `#` as opened it, so it opens with one `#` more than the longest run
    /// of `#` in `s`: nothing in `s` can end it early.
    fn raw_string(s: &str) -> String {
        let longest_run = s.split(|c| c != '#').map(str::len).max().unwrap_or(0);
        let hashes = "#".repeat(longest_run + 1);
        format!("r{hashes}'{s}'{hashes}")
    }

    /// `s` as a Nushell double-quoted string, where `\` and `"` are the characters to escape.
    fn double_quoted(s: &str) -> String {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    }

    fn escape_csv_value(s: &str) -> String {
        if s.contains(['\r', '\n', '"', ',']) {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_owned()
        }
    }

    fn format_activate_prelude_inline(&self, prelude: &[ActivatePrelude]) -> String {
        prelude
            .iter()
            .map(|p| match p {
                ActivatePrelude::Set(k, v) if env::is_path_key(k) => {
                    let v = Self::raw_string(v);
                    format!("$env.{k} = ({v} | split row (char esep))\n")
                }
                // `mise activate nu` is saved to a file that outlives the session, and this key
                // decrypts the env cache. The module makes one when it is loaded instead, and
                // keeps one it inherits, as `CachedEnv::ensure_encryption_key` does.
                ActivatePrelude::Set(k, _) if k == "__MISE_ENV_CACHE_KEY" => formatdoc! {r#"
                    if "{k}" not-in $env {{ $env.{k} = (random binary 32 | encode base64) }}
                "#},
                ActivatePrelude::Set(k, v) => format!("$env.{k} = {}\n", Self::raw_string(v)),
                ActivatePrelude::Prepend(k, v) | ActivatePrelude::MovePrepend(k, v) => {
                    self.prepend_env(k, v)
                }
            })
            .join("")
    }
}

impl Shell for Nushell {
    fn activate(&self, opts: ActivateOptions) -> String {
        let exe = opts.exe;
        let flags = opts.flags;
        let exe = Self::double_quoted(&exe.to_string_lossy());

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

        // Undoing an inherited activation restores the value every variable had before mise
        // changed it, secrets included. The module is saved to a file, so it asks mise for those
        // operations when it is loaded rather than carrying them. `mise deactivate` also hides
        // `__MISE_ORIG_PATH`, which activation keeps. When mise fails or is gone, the module still
        // loads, with nothing undone, so the rest of the config that uses it still runs.
        let inline_prelude = self.format_activate_prelude_inline(&opts.prelude);
        out.push_str(&formatdoc! {r#"
          export-env {{
            let deactivation_ops = if "__MISE_DIFF" in $env {{
              try {{
                with-env {{ MISE_SHELL: "nu" }} {{ ^{exe} deactivate }}
                | parse vars
                | where name != "__MISE_ORIG_PATH"
              }} catch {{
                []
              }}
            }} else {{
              []
            }}
            {inline_prelude}
            $deactivation_ops | update-env
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

          # `main` is a Nushell command, so the external completer that `mise completion nu`
          # installs never sees `mise <Tab>`. Ask mise for the candidates instead.
          def "nu-complete mise" [context: string] {{
            let out = (try {{ ^{exe} __complete_word__ --shell nu --line $context | complete }} catch {{ null }})
            if $out == null or $out.exit_code != 0 {{ return null }}
            let marker = "\u{{1}}"
            let lines = ($out.stdout | lines | where {{|l| $l != "" }})
            let wants_paths = ($lines | any {{|l| $l | str starts-with $marker }})
            let candidates = (
              $lines
              | where {{|l| not ($l | str starts-with $marker) }}
              | each {{|l|
                let parts = ($l | split row (char tab))
                {{
                  value: ($parts | get 0)
                  description: (if ($parts | length) > 1 {{ $parts | get 1 }} else {{ "" }})
                }}
              }}
            )
            if ($candidates | is-empty) and $wants_paths {{ null }} else {{ $candidates }}
          }}

          export def --env --wrapped main [command?: string@"nu-complete mise", --help, ...rest: string@"nu-complete mise"] {{
            let commands = ["deactivate", "shell", "sh"]

            if ($command == null) {{
              ^{exe}
            }} else if ($command == "activate") {{
              $env.MISE_SHELL = "nu"
            }} else if ($command in $commands) {{
              ^{exe} $command ...$rest
              | parse vars
              | update-env
            }} else {{
              ^{exe} $command ...$rest
            }}
          }}

          def --env mise_hook [] {{
            ^{exe} hook-env{flags} -s nu
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
        ]
        .join("")
    }

    fn set_env(&self, k: &str, v: &str) -> String {
        let k = Nushell::escape_csv_value(k);
        let v = Nushell::escape_csv_value(v);

        EnvOp::Set { key: &k, val: &v }.to_string()
    }

    fn prepend_env(&self, k: &str, v: &str) -> String {
        let v = Self::raw_string(v);
        format!("$env.{k} = ($env.{k} | prepend {v})\n")
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
    fn raw_string_cannot_be_ended_by_its_contents() {
        assert_eq!(Nushell::raw_string("a'b"), "r#'a'b'#");
        assert_eq!(Nushell::raw_string("x'#; y"), "r##'x'#; y'##");
        assert_eq!(Nushell::raw_string("#a##'###"), "r####'#a##'###'####");
        assert_eq!(Nushell::raw_string(""), "r#''#");
    }

    #[test]
    fn activation_does_not_carry_the_env_cache_key() {
        let nushell = Nushell::default();
        let opts = ActivateOptions {
            exe: Path::new("/some/dir/mise").to_path_buf(),
            flags: "".into(),
            no_hook_env: false,
            prelude: vec![ActivatePrelude::Set(
                "__MISE_ENV_CACHE_KEY".into(),
                "c2VjcmV0LWtleQ==".into(),
            )],
        };
        let out = nushell.activate(opts);
        assert!(!out.contains("c2VjcmV0LWtleQ=="), "{out}");
        assert!(out.contains("random binary 32 | encode base64"), "{out}");
    }

    #[test]
    fn exe_path_is_quoted_for_nushell() {
        let nushell = Nushell::default();
        let opts = ActivateOptions {
            exe: Path::new(r#"/a "b"\c/mise"#).to_path_buf(),
            flags: "".into(),
            no_hook_env: false,
            prelude: vec![],
        };
        let out = nushell.activate(opts);
        assert!(out.contains(r#"^"/a \"b\"\\c/mise" hook-env"#), "{out}");
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
