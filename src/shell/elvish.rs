#![allow(unknown_lints)]
use std::borrow::Cow;
use std::fmt::Display;

use crate::shell::{self, ActivateOptions, Shell};
use indoc::formatdoc;

#[derive(Default)]
pub(super) struct Elvish {}

impl Shell for Elvish {
    fn activate(&self, opts: ActivateOptions) -> String {
        let exe = opts.exe;
        let flags = opts.flags;
        let exe = escape(exe.to_string_lossy());

        let mut out = String::new();
        out.push_str(&shell::build_deactivation_script(self));
        out.push_str(&self.format_activate_prelude(&opts.prelude));
        out.push_str(&formatdoc! {r#"
            var hook-enabled = $false

            fn hook-env {{
              if $hook-enabled {{
                eval ((external {exe}) hook-env{flags} -s elvish | slurp)
              }}
            }}

            set after-chdir = (conj $after-chdir {{|_| hook-env }})
            set edit:before-readline = (conj $edit:before-readline $hook-env~)

            fn activate {{
              set-env MISE_SHELL elvish
              set hook-enabled = ${hook_enabled}
              hook-env
            }}

            fn deactivate {{
              set hook-enabled = $false
              eval ((external {exe}) deactivate | slurp)
            }}

            fn mise {{|@a|
              if (== (count $a) 0) {{
                (external {exe})
                return
              }}

              if (not (or (has-value $a -h) (has-value $a --help))) {{
                var command = $a[0]
                if (==s $command shell) {{
                  try {{ eval ((external {exe}) $@a) }} catch {{ }}
                  return
                }} elif (==s $command deactivate) {{
                  deactivate
                  return
                }} elif (==s $command activate) {{
                  activate
                  return
                }}
              }}
              (external {exe}) $@a
            }}
            "#, hook_enabled = !opts.no_hook_env});
        out
    }

    fn deactivate(&self) -> String {
        formatdoc! {r#"
            unset-env MISE_SHELL
            unset-env __MISE_DIFF
            unset-env __MISE_SESSION
        "#}
    }

    fn set_env(&self, k: &str, v: &str) -> String {
        let k = escape(k.into());
        // No `\n` rewriting. This used to replace the two characters `\` and `n` with a newline,
        // which no other shell here does, and which cost `C:\nodejs` its `n`: the value reached
        // elvish as `C:` + newline + `odejs`, since a newline inside `'...'` is just a newline.
        // A value that holds a real newline still carries one through, unchanged -- so the only
        // thing this drops is the rewriting of values that never had one.
        let v = escape(v.into());
        format!("set-env {k} {v}\n")
    }

    fn prepend_env(&self, k: &str, v: &str) -> String {
        let k = escape(k.into());
        // The list separator is required: without it the new entry is glued onto the first
        // existing one, so prepending /new/dir to /usr/bin:/bin produced /new/dir/usr/bin:/bin.
        //
        // It goes inside the quoted value rather than beside it, because two quoted words may
        // not be written adjacent: elvish reads `''` as one literal quote, so `{v}':'` was a
        // single string ending in `':` whenever `v` itself came out quoted. On Windows that is
        // every time -- a drive letter and backslashes both trip `needs_quoting`.
        //
        // The separator is the host's, since the shell reading this script runs on the platform
        // mise was built for. bash and zsh hardcode ":" here too, but under Git Bash their own
        // $PATH is unix-form and colon-separated, so that is a different question.
        let sep = if cfg!(windows) { ';' } else { ':' };
        let v = escape(format!("{v}{sep}").into());
        format!("set-env {k} {v}(get-env {k})\n")
    }

    fn unset_env(&self, k: &str) -> String {
        format!("unset-env {k}\n", k = escape(k.into()))
    }
}

impl Display for Elvish {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "elvish")
    }
}

/// Quote `input` for elvish, leaving it bare when it needs no quoting.
///
/// Inside `'...'` elvish escapes nothing: "All enclosed characters represent themselves, except
/// the single quote. Two consecutive single quotes are handled as a special case: they represent
/// one single quote." `shell_escape::unix::escape` was writing bash's rules instead — `'` as
/// `'\''` and `!` as `'\!'` — and a backslash is an ordinary bareword character in elvish, not an
/// escape. So neither produced a parse error; both simply arrived with a backslash stuck to them,
/// and `a'b` reached the environment as `a\'b`. Same reasoning as `escape_sq` in the pwsh shell
/// and `xonsh_escape_sq` in the xonsh one, both of which already spell their own shell's rules.
///
/// The signature is `shell_escape`'s so the call sites are unchanged.
fn escape(input: Cow<'_, str>) -> Cow<'_, str> {
    if !input.is_empty() && !input.contains(needs_quoting) {
        return input;
    }
    Cow::Owned(format!("'{}'", input.replace('\'', "''")))
}

/// Whether `ch` forces its string to be quoted.
///
/// Deliberately the set `shell_escape` used, not elvish's own bareword set, which is wider
/// (`!%+,-./:@\_` and more). Quoting something elvish would have accepted bare is harmless, and
/// keeping the decision fixed means this change is only about how a quoted value is written.
fn needs_quoting(ch: char) -> bool {
    !matches!(ch, 'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '=' | '/' | ',' | '.' | '+')
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
        let elvish = Elvish::default();
        let exe = Path::new("/some/dir/mise");
        let opts = ActivateOptions {
            exe: exe.to_path_buf(),
            flags: " --status".into(),
            no_hook_env: false,
            prelude: vec![],
        };
        assert_snapshot!(elvish.activate(opts));
    }

    #[test]
    fn test_set_env() {
        assert_snapshot!(Elvish::default().set_env("FOO", "1"));
    }

    /// The separator differs by host, so this one is unix-only; the Windows form is asserted
    /// below. The snapshot moved because the value here needs quoting: it used to end
    /// `'/some/dir:/2/dir'':'`, which elvish reads as the single string `/some/dir:/2/dir':`.
    #[cfg(not(windows))]
    #[test]
    fn test_prepend_env() {
        let sh = Elvish::default();
        assert_snapshot!(replace_path(&sh.prepend_env("PATH", "/some/dir:/2/dir")));
    }

    /// The control for the change on unix: a path the escaper leaves bare is what production
    /// actually passes, and there the emitted text moved from `/some/dir':'(get-env PATH)` to
    /// this while the value elvish builds -- `/some/dir:` then PATH -- did not.
    #[cfg(not(windows))]
    #[test]
    fn a_bare_path_still_prepends_the_same_value() {
        assert_eq!(
            Elvish::default().prepend_env("PATH", "/some/dir"),
            "set-env PATH '/some/dir:'(get-env PATH)\n"
        );
    }

    /// The rule this is all about, stated directly. Elvish reads `''` as one literal quote
    /// rather than as the end of one string and the start of the next, so two quoted words may
    /// never be written adjacent. A value with a space is quoted on either host, so this test
    /// means the same thing on both.
    #[test]
    fn two_quotes_are_never_written_together() {
        // No apostrophe in the value on purpose: `''` is elvish's way of writing one, so a value
        // that holds an apostrophe is allowed -- and required -- to contain it.
        let out = Elvish::default().prepend_env("PATH", "/some dir");
        assert!(!out.contains("''"), "{out:?}");
        assert!(out.contains("/some dir"), "{out:?}");
        assert!(out.ends_with("(get-env PATH)\n"), "{out:?}");
    }

    /// Elvish writes an apostrophe by doubling it. `shell_escape::unix::escape` wrote bash's
    /// `'\''`, and since a backslash is an ordinary bareword character in elvish rather than an
    /// escape, that was not a parse error -- it reached the environment as `a\'b`.
    #[test]
    fn an_apostrophe_is_doubled_rather_than_backslashed() {
        assert_eq!(
            Elvish::default().set_env("FOO", "a'b"),
            "set-env FOO 'a''b'\n"
        );
    }

    /// The other character bash escaping treats specially. Elvish has no history expansion, so
    /// `!` inside quotes is already literal; it was arriving as `a\!b`.
    #[test]
    fn a_bang_carries_no_backslash() {
        assert_eq!(
            Elvish::default().set_env("FOO", "a!b"),
            "set-env FOO 'a!b'\n"
        );
    }

    /// The empty value takes the quoting branch on its own, and `''` is an empty string in
    /// elvish rather than the start of an escaped quote -- the lookahead only pairs them when a
    /// third character follows.
    #[test]
    fn an_empty_value_is_an_empty_quoted_string() {
        assert_eq!(Elvish::default().set_env("FOO", ""), "set-env FOO ''\n");
    }

    /// `set_env` used to replace the two characters `\` and `n` with a newline, which no other
    /// shell in this directory does. Measured on 2026.8.14: `mise env -s elvish` printed
    /// `set-env NODEDIR 'C:` / `odejs'` across two lines for a value of `C:\nodejs`, and a newline
    /// inside `'...'` is just a newline, so that is what the variable ended up holding.
    #[test]
    fn a_literal_backslash_n_stays_two_characters() {
        assert_eq!(
            Elvish::default().set_env("NODEDIR", r"C:\nodejs"),
            concat!(r"set-env NODEDIR 'C:\nodejs'", "\n")
        );
    }

    /// The control, and the reason removing that replacement loses nothing: a value that really
    /// does hold a newline still carries one through. Before this, the two were indistinguishable
    /// -- `C:\nodejs` and a genuine newline produced the same text -- which is the defect.
    #[test]
    fn a_real_newline_is_still_carried_through() {
        assert_eq!(
            Elvish::default().set_env("MULTI", "a\nb"),
            "set-env MULTI 'a\nb'\n"
        );
    }

    /// The control for the quoting decision, which this change deliberately leaves alone: a value
    /// that needed no quotes still gets none.
    #[test]
    fn a_plain_value_is_still_left_bare() {
        assert_eq!(
            Elvish::default().set_env("FOO", "a=b/c.d"),
            "set-env FOO a=b/c.d\n"
        );
    }

    /// The reproduction. A Windows path is always quoted -- the drive letter's `:` and the
    /// backslashes are both outside the escaper's whitelist -- so it always hit the doubled
    /// quote, and it was joined to the rest of PATH by `:` rather than `;`.
    #[cfg(windows)]
    #[test]
    fn a_windows_path_is_quoted_once_and_joined_with_a_semicolon() {
        assert_eq!(
            Elvish::default().prepend_env("PATH", r"C:\a\shims"),
            concat!(r"set-env PATH 'C:\a\shims;'(get-env PATH)", "\n")
        );
    }

    #[test]
    fn test_unset_env() {
        assert_snapshot!(Elvish::default().unset_env("FOO"));
    }

    #[test]
    fn test_deactivate() {
        let deactivate = Elvish::default().deactivate();
        assert_snapshot!(replace_path(&deactivate));
    }
}
