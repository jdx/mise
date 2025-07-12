#![allow(unknown_lints)]
#![allow(clippy::literal_string_with_formatting_args)]
use std::borrow::Cow;
use std::fmt::Display;

use indoc::formatdoc;

use crate::shell::{ActivateOptions, Shell};

#[derive(Default)]
pub struct Xonsh {}

fn xonsh_escape_sq(input: &str) -> Cow<'_, str> {
    for (i, ch) in input.char_indices() {
        if xonsh_escape_char(ch).is_some() {
            let mut escaped_string = String::with_capacity(input.len());

            escaped_string.push_str(&input[..i]);
            for ch in input[i..].chars() {
                match xonsh_escape_char(ch) {
                    Some(escaped_char) => escaped_string.push_str(escaped_char),
                    None => escaped_string.push(ch),
                };
            }
            return Cow::Owned(escaped_string);
        }
    }
    Cow::Borrowed(input)
}

fn xonsh_escape_char(ch: char) -> Option<&'static str> {
    match ch {
        // escape ' \ ␤ (docs.python.org/3/reference/lexical_analysis.html#strings)
        '\'' => Some("\\'"),
        '\\' => Some("\\\\"),
        '\n' => Some("\\n"),
        _ => None,
    }
}

impl Shell for Xonsh {
    fn activate(&self, opts: ActivateOptions) -> String {
        let exe = opts.exe;
        let flags = opts.flags;
        let exe = exe.display();

        let mut out = String::new();
        out.push_str(&self.format_activate_prelude(&opts.prelude));

        // use xonsh API instead of $.xsh to allow use inside of .py configs, which start faster due to being compiled to .pyc
        out.push_str(&formatdoc! {r#"
            from xonsh.built_ins import XSH

            def _mise(args):
              if args and args[0] in ('deactivate', 'shell', 'sh'):
                execx($(mise @(args)))
              else:
                mise @(args)

            XSH.env['MISE_SHELL'] = 'xonsh'
            XSH.aliases['mise'] = _mise
        "#});

        if !opts.no_hook_env {
            out.push_str(&formatdoc! {r#"
                import shlex
                import subprocess

                extra_args = shlex.split('{flags}')
                def mise_hook(**kwargs): # Hook Events
                    script = subprocess.run(
                        ['{exe}', 'hook-env', *extra_args, '-s', 'xonsh'],
                        env=XSH.env.detype(),
                        stdout=subprocess.PIPE,
                    ).stdout.decode()
                    execx(script)

                XSH.builtins.events.on_pre_prompt(mise_hook) # Activate hook: before showing the prompt
                XSH.builtins.events.on_chdir(mise_hook) # Activate hook: when the working directory changes
            "#});
        }
        out
    }

    fn deactivate(&self) -> String {
        formatdoc! {r#"
            import os
            from xonsh.built_ins import XSH

            hooks = {{
              'on_pre_prompt' : ['mise_hook'],
              'on_chdir': ['mise_hook'],
            }}
            for hook_type, hook_fns in hooks.items():
              for hook_fn in hook_fns:
                hndl = getattr(XSH.builtins.events, hook_type)
                for fn in hndl:
                  if fn.__name__ == hook_fn:
                    hndl.remove(fn)
                    break

            del XSH.aliases['mise']
            del XSH.env['MISE_SHELL']
            del XSH.env['__MISE_DIFF']
            del XSH.env['__MISE_SESSION']
            "#}
    }

    fn set_env(&self, k: &str, v: &str) -> String {
        formatdoc!(
            r#"
            from xonsh.built_ins import XSH
            XSH.env['{k}'] = '{v}'
        "#,
            k = shell_escape::unix::escape(k.into()), // todo: drop illegal chars, not escape?
            v = xonsh_escape_sq(v)
        )
    }

    fn prepend_env(&self, k: &str, v: &str) -> String {
        formatdoc!(
            r#"
            from xonsh.built_ins import XSH
            XSH.env['{k}'].add('{v}', front=True)
        "#,
            k = shell_escape::unix::escape(k.into()),
            v = xonsh_escape_sq(v)
        )
    }

    fn unset_env(&self, k: &str) -> String {
        formatdoc!(
            r#"
            from xonsh.built_ins import XSH
            XSH.env.pop('{k}',None)
        "#,
            k = shell_escape::unix::escape(k.into()) // todo: drop illegal chars, not escape?
        )
    }
}

impl Display for Xonsh {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "xonsh")
    }
}

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;
    use pretty_assertions::assert_eq;
    use std::path::Path;

    use crate::test::replace_path;

    use super::*;

    #[test]
    fn test_hook_init() {
        let xonsh = Xonsh::default();
        let exe = Path::new("/some/dir/mise");
        let opts = ActivateOptions {
            exe: exe.to_path_buf(),
            flags: " --status".into(),
            no_hook_env: false,
            prelude: vec![],
        };
        insta::assert_snapshot!(xonsh.activate(opts));
    }

    #[test]
    fn test_set_env() {
        insta::assert_snapshot!(Xonsh::default().set_env("FOO", "1"));
    }

    #[test]
    fn test_prepend_env() {
        let sh = Xonsh::default();
        assert_snapshot!(replace_path(&sh.prepend_env("PATH", "/some/dir:/2/dir")));
    }

    #[test]
    fn test_unset_env() {
        insta::assert_snapshot!(Xonsh::default().unset_env("FOO"));
    }

    #[test]
    fn test_xonsh_escape_sq() {
        assert_eq!(xonsh_escape_sq("foo"), "foo");
        assert_eq!(xonsh_escape_sq("foo'bar"), "foo\\'bar");
        assert_eq!(xonsh_escape_sq("foo\\bar"), "foo\\\\bar");
        assert_eq!(xonsh_escape_sq("foo\nbar"), "foo\\nbar");
    }

    #[test]
    fn test_xonsh_deactivate() {
        let deactivate = Xonsh::default().deactivate();
        assert_snapshot!(replace_path(&deactivate));
    }
}
