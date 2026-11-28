#![allow(unknown_lints)]
use std::borrow::Cow;
use std::fmt::Display;

use shell_escape::unix::escape;

use crate::config::Settings;
use crate::env;
use crate::shell::{self, ActivateOptions, Shell};

#[derive(Default)]
pub(super) struct Bash {
    /// Accept letters outside ASCII in variable names, as zsh does in a UTF-8 locale. bash
    /// takes ASCII letters only and errors on the others at every prompt.
    unicode_names: bool,
}

impl Bash {
    /// The statements zsh writes through this file, which take zsh's variable names.
    pub(super) fn for_zsh() -> Self {
        Self {
            unicode_names: true,
        }
    }

    fn is_shell_name(&self, name: &str) -> bool {
        is_shell_name(name, self.unicode_names)
    }
}

fn render_template(template: &str, replacements: &[(&str, &str)]) -> String {
    let mut out = template.to_owned();
    for (needle, value) in replacements {
        out = out.replace(needle, value);
    }
    out
}

fn render_flags_array(value: &str) -> String {
    shell_words::split(value)
        .expect("failed to split activation flags")
        .into_iter()
        .map(|word| escape(word.into()).to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether `name` can be a variable in bash or zsh: letters, digits and `_`, not starting with
/// a digit.
///
/// Quoting cannot stand in for this check. `export` and `unset` parse the word again once the
/// quotes are off and evaluate the subscript of a `name[subscript]`, command substitutions
/// included: `export 'x[$(cmd)]'=y` runs `cmd` in zsh, and so does `unset 'DIRSTACK[$(cmd)]'` in
/// bash. Names come from env files and `[env]` tables, which take any string, so one the shell
/// could not hold as a variable is left out of the script.
///
/// With `unicode`, letters are not limited to ASCII: zsh accepts the others in a UTF-8 locale,
/// and a zsh that does not refuses them without evaluating anything. bash takes ASCII only.
fn is_shell_name(name: &str, unicode: bool) -> bool {
    let letter = |c: char| {
        if unicode {
            c.is_alphanumeric()
        } else {
            c.is_ascii_alphanumeric()
        }
    };
    !name.is_empty()
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name.chars().all(|c| c == '_' || letter(c))
}

/// The value of an `export k=v` or `alias k=v` statement, quoted for bash and zsh.
///
/// zsh writes these statements through this file too, and `shell_escape` leaves `=` bare. zsh
/// (`EQUALS`, on by default) replaces an unquoted `=cmd` at the start of an assigned value with
/// the path of `cmd`, and aborts the enclosing `eval` when there is no such command, so the rest
/// of the `hook-env` output is never applied. A value that starts with `=` is quoted as well; it
/// holds nothing else that needs quoting, or `shell_escape` would have quoted it.
fn escape_value(v: Cow<'_, str>) -> Cow<'_, str> {
    let v = escape(v);
    if v.starts_with('=') {
        format!("'{v}'").into()
    } else {
        v
    }
}

impl Shell for Bash {
    fn activate(&self, opts: ActivateOptions) -> String {
        let exe = opts.exe;
        let settings = Settings::get();

        let exe = exe.to_string_lossy();
        let exe = escape(crate::windows_posix::executable_for_shell(&exe));
        let flags = render_flags_array(&opts.flags);

        let mut out = String::new();

        out.push_str(&shell::build_deactivation_script(self));

        out.push_str(&self.format_activate_prelude(&opts.prelude));
        let activate = render_template(
            include_str!("../assets/bash/activate.sh"),
            &[
                ("__MISE_EXE_VALUE__", &exe),
                ("__MISE_FLAGS_VALUE__", &flags),
                (
                    "__MISE_HOOK_ENABLED_VALUE__",
                    if opts.no_hook_env { "0" } else { "1" },
                ),
                (
                    "__MISE_CHPWD_FUNCTIONS__",
                    include_str!("../assets/bash_zsh_support/chpwd/function.sh"),
                ),
                (
                    "__MISE_CHPWD_LOAD__",
                    include_str!("../assets/bash_zsh_support/chpwd/load.sh"),
                ),
            ],
        );
        out.push_str(&activate);
        out.push('\n');
        if settings.not_found_auto_install {
            let not_found = render_template(
                include_str!("../assets/bash/command_not_found.sh"),
                &[("__MISE_EXE__", &exe)],
            );
            out.push_str(&not_found);
            out.push('\n');
        }

        out
    }

    fn deactivate(&self) -> String {
        include_str!("../assets/bash/deactivate.sh").to_string()
    }

    fn set_env(&self, k: &str, v: &str) -> String {
        let is_path = env::is_path_key(k);
        let v = if is_path {
            crate::windows_posix::windows_path_list_for_shell(v)
        } else if k == "MANPATH" {
            crate::windows_posix::windows_manpath_for_shell(v)
        } else {
            v.into()
        };
        let k = if is_path { "PATH" } else { k };
        if !self.is_shell_name(k) {
            debug!("not exporting {k:?}: it is not a shell variable name");
            return String::new();
        }
        let k = shell_escape::unix::escape(k.into());
        let v = escape_value(v);
        format!("export {k}={v}\n")
    }

    fn prepend_env(&self, k: &str, v: &str) -> String {
        let is_path = env::is_path_key(k);
        let v = if is_path {
            crate::windows_posix::windows_path_list_for_shell(v)
        } else {
            v.into()
        };
        let k = if is_path { "PATH" } else { k };
        if !self.is_shell_name(k) {
            debug!("not exporting {k:?}: it is not a shell variable name");
            return String::new();
        }
        let v = v
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('`', "\\`")
            .replace('$', "\\$");
        format!("export {k}=\"{v}:${k}\"\n")
    }

    fn unset_env(&self, k: &str) -> String {
        if !self.is_shell_name(k) {
            return String::new();
        }
        format!("unset {k}\n", k = shell_escape::unix::escape(k.into()))
    }

    fn set_alias(&self, name: &str, cmd: &str) -> String {
        let name = shell_escape::unix::escape(name.into());
        let cmd = escape_value(cmd.into());
        format!("alias {name}={cmd}\n")
    }

    fn unset_alias(&self, name: &str) -> String {
        let name = shell_escape::unix::escape(name.into());
        format!("unalias {name} 2>/dev/null || true\n")
    }
}

impl Display for Bash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "bash")
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
    fn test_activate() {
        unsafe {
            std::env::remove_var("__MISE_ORIG_PATH");
            std::env::remove_var("__MISE_DIFF");
        }

        let bash = Bash::default();
        let exe = Path::new("/some/dir/mise");
        let opts = ActivateOptions {
            exe: exe.to_path_buf(),
            flags: " --status".into(),
            no_hook_env: false,
            prelude: vec![],
        };
        assert_snapshot!(bash.activate(opts));
    }

    #[test]
    fn test_set_env() {
        assert_snapshot!(Bash::default().set_env("FOO", "1"));
    }

    #[test]
    fn a_value_starting_with_equals_is_quoted() {
        let bash = Bash::default();
        assert_eq!(bash.set_env("FOO", "=bar"), "export FOO='=bar'\n");
        assert_eq!(bash.set_env("FOO", "="), "export FOO='='\n");
        assert_eq!(bash.set_env("FOO", "=a b"), "export FOO='=a b'\n");
        assert_eq!(bash.set_env("FOO", "a:=b"), "export FOO='a:=b'\n");
        assert_eq!(bash.set_env("FOO", "bar=baz="), "export FOO=bar=baz=\n");
        assert_eq!(bash.set_env("FOO", ""), "export FOO=''\n");
        assert_eq!(bash.set_alias("ll", "=ls"), "alias ll='=ls'\n");
        assert_eq!(bash.set_alias("ll", "ls"), "alias ll=ls\n");
    }

    #[test]
    fn a_name_that_is_not_a_shell_variable_is_left_out() {
        let bash = Bash::default();
        for name in [
            "x[$(touch marker)]",
            "DIRSTACK[`touch marker`]",
            "a[1]",
            "a-b",
            "a.b",
            "a b",
            "a=b",
            "1a",
            "",
        ] {
            assert_eq!(bash.set_env(name, "x"), "", "{name}");
            assert_eq!(bash.prepend_env(name, "x"), "", "{name}");
            assert_eq!(bash.unset_env(name), "", "{name}");
        }
        assert_eq!(bash.set_env("_a1", "x"), "export _a1=x\n");
        assert_eq!(bash.unset_env("_a1"), "unset _a1\n");
        assert_eq!(bash.set_env("ÉTÉ", "x"), "");
        assert_eq!(bash.unset_env("ÉTÉ"), "");
        let zsh = Bash::for_zsh();
        assert_eq!(zsh.set_env("ÉTÉ", "x"), "export 'ÉTÉ'=x\n");
        assert_eq!(zsh.set_env("x[1]", "x"), "");
    }

    #[test]
    fn test_prepend_env() {
        let bash = Bash::default();
        assert_snapshot!(replace_path(&bash.prepend_env("PATH", "/some/dir:/2/dir")));
    }

    #[test]
    fn path_preludes_quote_spaces_and_single_quotes() {
        assert_eq!(
            Bash::default().prepend_env("PATH", "/a b/say'hi"),
            "export PATH=\"/a b/say'hi:$PATH\"\n"
        );
    }

    #[cfg(windows)]
    #[test]
    fn an_unrecognized_windows_context_keeps_native_path_entries() {
        let output = Bash::default().set_env("Path", r"C:\a bin;D:\b");
        assert!(output.starts_with("export PATH="));
        assert!(output.contains(r"C:\a bin;D:\b"));
        assert!(!output.contains("/c/a bin"));
    }

    #[test]
    fn test_unset_env() {
        assert_snapshot!(Bash::default().unset_env("FOO"));
    }

    #[test]
    fn test_deactivate() {
        let deactivate = Bash::default().deactivate();
        assert_snapshot!(replace_path(&deactivate));
    }
}
