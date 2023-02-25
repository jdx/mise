use std::path::Path;

use indoc::formatdoc;

use crate::shell::{is_dir_in_path, Shell};

#[derive(Default)]
pub struct Xonsh {}

use std::borrow::Cow;

fn xonsh_escape_sq(input: &str) -> Cow<str> {
    for (i, ch) in input.chars().enumerate() {
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
    fn activate(&self, exe: &Path, status: bool) -> String {
        let dir = exe.parent().unwrap();
        let exe = exe.display();
        let status = if status { " --status" } else { "" };
        let mut out = String::new();

        // todo: xonsh doesn't update the environment that rtx relies on with $PATH.add even with $UPDATE_OS_ENVIRON (github.com/xonsh/xonsh/issues/3207)
        // with envx.swap(UPDATE_OS_ENVIRON=True): # ← use when ↑ fixed before PATH.add; remove environ
        // meanwhile, save variables twice: in shell env + in os env
        // use xonsh API instead of $.xsh to allow use inside of .py configs, which start faster due to being compiled to .pyc
        out.push_str(&formatdoc! {r#"
            from os               import environ
            from xonsh.built_ins  import XSH

        "#});
        if !is_dir_in_path(dir) {
            let dir_str = dir.to_string_lossy();
            let dir_esc = xonsh_escape_sq(&dir_str);
            out.push_str(&formatdoc! {r#"
                envx = XSH.env
                envx['PATH'].add('{dir_esc}')
                environ['PATH'] = envx.get_detyped('PATH')

            "#});
        }
        // todo: subprocess instead of $() is a bit faster, but lose auto-color detection (use $FORCE_COLOR)
        out.push_str(&formatdoc! {r#"
            def listen_prompt(): # Hook Events
              execx($({exe} hook-env{status} -s xonsh))

            XSH.builtins.events.on_pre_prompt(listen_prompt) # Activate hook: before showing the prompt
            "#});

        out
    }

    fn deactivate(&self, path: String) -> String {
        formatdoc! {r#"
            from xonsh.built_ins  import XSH

            environ['PATH'] = '{path}'
            hooks = {{
              'on_pre_prompt' : ['listen_prompt'],
            }}
            for   hook_type in hooks:
              hook_fns = hooks[hook_type]
              for hook_fn   in hook_fns:
                hndl = getattr(XSH.builtins.events, hook_type)
                for fn in hndl:
                  if fn.__name__ == hook_fn:
                    hndl.remove(fn)
                    break
            "#}
    }

    fn set_env(&self, k: &str, v: &str) -> String {
        let k = shell_escape::unix::escape(k.into()); // todo: drop illegal chars, not escape?
        formatdoc!(
            r#"
            from os               import environ
            from xonsh.built_ins  import XSH

            envx = XSH.env
            envx[   '{k}'] = '{v}'
            environ['{k}'] = envx.get_detyped('{k}')
        "#,
            k = shell_escape::unix::escape(k), // todo: drop illegal chars, not escape?
            v = xonsh_escape_sq(v)
        )
    }

    fn unset_env(&self, k: &str) -> String {
        formatdoc!(
            r#"
            from os               import environ
            from xonsh.built_ins  import XSH

            envx = XSH.env
            envx.pop[   '{k}',None]
            environ.pop['{k}',None]
        "#,
            k = shell_escape::unix::escape(k.into()) // todo: drop illegal chars, not escape?
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::replace_path;
    use insta::assert_snapshot;

    #[test]
    fn test_hook_init() {
        let xonsh = Xonsh::default();
        let exe = Path::new("/some/dir/rtx");
        insta::assert_snapshot!(xonsh.activate(exe, true));
    }

    #[test]
    fn test_set_env() {
        insta::assert_snapshot!(Xonsh::default().set_env("FOO", "1"));
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
        let deactivate = Xonsh::default().deactivate("oldpath".into());
        assert_snapshot!(replace_path(&deactivate));
    }
}
