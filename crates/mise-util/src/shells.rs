//! The shells mise runs inline commands and task files with, from settings.

use eyre::Result;
use mise_settings::Settings;
use std::sync::OnceLock;

const UNIX_DEFAULT_FILE_SHELL_ARGS: &str = "sh";
const UNIX_DEFAULT_INLINE_SHELL_ARGS: &str = "sh -o errexit -c";
const WINDOWS_DEFAULT_FILE_SHELL_ARGS: &str = "cmd /c";
const WINDOWS_DEFAULT_INLINE_SHELL_ARGS: &str = "cmd /c";

/// The shell inline commands run under: `unix_default_inline_shell_args` or its Windows
/// counterpart, falling back to the built-in default when the setting is empty.
pub fn default_inline_shell(settings: &Settings) -> Result<Vec<String>> {
    let (sa, fallback) = if cfg!(windows) {
        (
            &settings.windows_default_inline_shell_args,
            WINDOWS_DEFAULT_INLINE_SHELL_ARGS,
        )
    } else {
        (
            &settings.unix_default_inline_shell_args,
            UNIX_DEFAULT_INLINE_SHELL_ARGS,
        )
    };
    let mut shell = split_default_shell_or_fallback(sa, fallback)?;
    maybe_no_profile(settings, &mut shell);
    Ok(shell)
}

/// The shell task files run under, chosen the same way as [`default_inline_shell`].
pub fn default_file_shell(settings: &Settings) -> Result<Vec<String>> {
    let (sa, fallback) = if cfg!(windows) {
        (
            &settings.windows_default_file_shell_args,
            WINDOWS_DEFAULT_FILE_SHELL_ARGS,
        )
    } else {
        (
            &settings.unix_default_file_shell_args,
            UNIX_DEFAULT_FILE_SHELL_ARGS,
        )
    };
    let mut shell = split_default_shell_or_fallback(sa, fallback)?;
    maybe_no_profile(settings, &mut shell);
    Ok(shell)
}

/// Inject `-NoProfile` into a PowerShell shell command when
/// `windows_powershell_no_profile` is enabled. No-op for other shells.
pub fn maybe_no_profile(settings: &Settings, shell: &mut Vec<String>) {
    if settings.windows_powershell_no_profile {
        crate::path::inject_powershell_no_profile(shell);
    }
}

fn split_default_shell_or_fallback(sa: &str, fallback: &str) -> Result<Vec<String>> {
    let shell = crate::path::split_shell_command(sa)?;
    if shell.is_empty() {
        crate::path::split_shell_command(fallback)
    } else {
        Ok(shell)
    }
}

static IMPLICIT_INLINE_SHELL: OnceLock<fn() -> bool> = OnceLock::new();

/// Register how to tell whether the inline shell is the implicit default rather
/// than one the user chose. Answering needs every config layer, so mise supplies it.
pub fn set_implicit_inline_shell(f: fn() -> bool) {
    let _ = IMPLICIT_INLINE_SHELL.set(f);
}

/// Whether the inline shell is the implicit default, so a command can be run directly
/// instead of through it. See [`set_implicit_inline_shell`].
pub fn implicit_inline_shell() -> bool {
    let f = IMPLICIT_INLINE_SHELL
        .get()
        .expect("mise_util::shells::set_implicit_inline_shell must be called first");
    f()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sv(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_split_default_shell_or_fallback_uses_fallback_for_empty_shell() {
        assert_eq!(
            split_default_shell_or_fallback("   ", "cmd /c").unwrap(),
            sv(&["cmd", "/c"])
        );
    }
    #[test]
    fn test_split_default_shell_or_fallback_preserves_custom_shell() {
        assert_eq!(
            split_default_shell_or_fallback("pwsh -Command", "cmd /c").unwrap(),
            sv(&["pwsh", "-Command"])
        );
    }
    #[test]
    fn test_split_default_shell_or_fallback_reports_parse_errors() {
        assert!(split_default_shell_or_fallback("\"unterminated", "cmd /c").is_err());
    }
}
