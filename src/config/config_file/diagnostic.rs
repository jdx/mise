#![allow(unused_assignments)] // Fields are read by miette's derive macros at runtime

use crate::dirs;
use crate::file::display_path;
use miette::{Diagnostic, NamedSource, SourceSpan};
use std::fmt;
use std::path::Path;
use thiserror::Error;

/// A TOML parsing error with source location information for rich display.
///
/// The message deliberately does not name the file. `src` already does, and miette prints it above
/// the snippet, so repeating it here rendered the same path twice in two different forms -- raw
/// from `Path::display` in the message, shortened by [`display_path`] in the snippet header. On
/// Windows the duplicate was also the one that wrapped, mid drive letter. Every caller that shows
/// this error without the snippet names the file itself.
#[derive(Debug, Error, Diagnostic)]
#[error("Invalid TOML in config file")]
#[diagnostic(code(mise::config::parse_error))]
pub(crate) struct TomlParseError {
    #[source_code]
    src: NamedSource<String>,
    #[label("{message}")]
    span: SourceSpan,
    message: String,
    /// Rendered as a `help:` line, and only when set. See [`backslash_help`].
    #[help]
    help: Option<String>,
}

/// Advice for the way a Windows path most often breaks a config file.
///
/// A backslash starts an escape inside a TOML basic string, so `"C:\Users\you"` fails with a
/// complaint about unicode digits or an expected escape character -- neither of which mentions the
/// backslash that caused it. `docs/configuration.md` already explains this, but only under the
/// `path:` tool scope, and nothing connects the error to it.
///
/// Two conditions, deliberately: the wording comes from the `toml` crate and could be reworded,
/// while a backslash on the failing line is a fact about the user's file. Requiring both means a
/// reworded message costs the advice rather than misplacing it.
fn backslash_help(source: &str, span: &SourceSpan, message: &str) -> Option<String> {
    if !(message.contains("escape") || message.contains("unicode")) {
        return None;
    }
    if !failing_line(source, span.offset())?.contains('\\') {
        return None;
    }
    Some(
        "a backslash starts an escape inside a double-quoted TOML string. \
         Write a Windows path as a literal string -- 'C:\\Users\\you' -- \
         or double the backslashes: \"C:\\\\Users\\\\you\"."
            .to_string(),
    )
}

/// The line `offset` falls on, or `None` if it is not a character boundary.
fn failing_line(source: &str, offset: usize) -> Option<&str> {
    let offset = offset.min(source.len());
    let before = source.get(..offset)?;
    let start = before.rfind('\n').map_or(0, |i| i + 1);
    let end = source
        .get(offset..)?
        .find('\n')
        .map_or(source.len(), |i| offset + i);
    source.get(start..end)
}

/// A diagnostic error that stores pre-rendered miette output.
/// This allows miette's fancy formatting to be preserved when wrapped in eyre.
#[derive(Debug)]
pub struct MiseDiagnostic {
    /// Short description for Display
    message: String,
    /// Pre-rendered miette output for rich display
    rendered: String,
}

impl fmt::Display for MiseDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for MiseDiagnostic {}

impl MiseDiagnostic {
    /// Create a new diagnostic from any miette Diagnostic
    pub(crate) fn new<D: Diagnostic + Send + Sync + 'static>(diagnostic: D) -> Self {
        let message = diagnostic.to_string();
        let rendered = format!("{:?}", miette::Report::new(diagnostic));
        MiseDiagnostic { message, rendered }
    }

    /// Get the pre-rendered miette output
    pub fn render(&self) -> &str {
        &self.rendered
    }
}

/// Whether a diagnostic about `path` may quote the text that was read from it.
///
/// A project can ship any file mise parses as a symlink, and some are parsed
/// with no trust decision behind them: `.miserc.toml`, `mise.lock` and
/// idiomatic version files always, `mise.toml` in safe mode. The text behind
/// such a link is the user's, not the project's (`/proc/self/environ` has no
/// newline, so its failing "line" is the whole environment), while stderr is
/// often a job log the project's author can read. Links inside the global and
/// system config directories are the user's own; those are told apart by
/// directory because the trust lookups need settings, which are still loading
/// while `.miserc.toml` is parsed.
fn may_quote(path: &Path) -> bool {
    path.starts_with(*dirs::CONFIG)
        || path.starts_with(*dirs::SYSTEM_CONFIG)
        || !reached_through_link(path)
}

/// Whether `path`, or a config directory mise descended into to find it
/// (`.config`, `.mise`, `mise`, `conf.d` and its folders, task directories), is
/// a symlink. The directory the file was found in and those above it are not
/// checked: the user chose to be there, and macOS keeps `/tmp` and `/var`
/// behind links.
fn reached_through_link(path: &Path) -> bool {
    let is_layout_dir = |dir: &Path| {
        let name = dir.file_name().and_then(|n| n.to_str());
        matches!(
            name,
            Some(".config" | ".mise" | "mise" | "conf.d" | "tasks" | "mise-tasks" | ".mise-tasks")
        ) || dir
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|n| n == "conf.d")
    };
    let mut current = path;
    loop {
        if current.is_symlink() {
            return true;
        }
        match current.parent() {
            Some(parent) if is_layout_dir(parent) => current = parent,
            _ => return false,
        }
    }
}

/// The 1-based line and column of `offset`, for a diagnostic that cannot show
/// the line itself.
fn position(source: &str, offset: usize) -> (usize, usize) {
    let before = source.get(..offset).unwrap_or("");
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().unwrap_or("").chars().count() + 1;
    (line, column)
}

/// For callers that report a `toml::de::Error` as is. Its `Display` quotes the
/// failing line, so only the message and its position are kept unless
/// [`may_quote`] allows `path` to be quoted.
pub(crate) fn hide_linked_source(err: toml::de::Error, source: &str, path: &Path) -> eyre::Report {
    if may_quote(path) {
        return err.into();
    }
    let (line, column) = position(source, err.span().map_or(0, |r| r.start));
    eyre::eyre!(
        "TOML parse error at line {line}, column {column}: {}",
        err.message()
    )
}

/// Create an eyre error from a toml::de::Error with rich source context.
pub(crate) fn toml_parse_error(err: &toml::de::Error, source: &str, path: &Path) -> eyre::Report {
    let message = err.message().to_string();

    // Get the byte span from toml error
    let span = err
        .span()
        .map(|r| SourceSpan::from((r.start, r.end.saturating_sub(r.start))))
        .unwrap_or_else(|| SourceSpan::from((0, 0)));

    if !may_quote(path) {
        let (line, column) = position(source, span.offset());
        return eyre::eyre!(
            "Invalid TOML in config file {}:{line}:{column}: {message}",
            display_path(path)
        );
    }

    let help = backslash_help(source, &span, &message);
    let diagnostic = TomlParseError {
        src: NamedSource::new(display_path(path), source.to_string()),
        span,
        message,
        help,
    };

    eyre::Report::new(MiseDiagnostic::new(diagnostic))
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    const BODY: &str = "[env]\ntoken s3cr3t\n";

    /// Everything the two reports would print about `BODY` read through `path`.
    fn reports(path: &Path) -> String {
        let err = toml::from_str::<toml::Value>(BODY).unwrap_err();
        let rich = toml_parse_error(&err, BODY, path);
        let rich = match rich.downcast_ref::<MiseDiagnostic>() {
            Some(diagnostic) => diagnostic.render().to_string(),
            None => format!("{rich:?}"),
        };
        format!("{rich}\n{:?}", hide_linked_source(err, BODY, path))
    }

    #[test]
    fn quotes_a_file_but_not_what_a_link_leads_to() {
        let tmp = tempfile::tempdir().unwrap();
        // the project itself may sit behind a link, as macOS's temp dirs do
        let tmp = tmp.path();
        let (home, project) = (tmp.join("home"), tmp.join("project"));
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(home.join("mise.toml"), BODY).unwrap();

        assert_eq!(
            reports(&home.join("mise.toml")).matches("s3cr3t").count(),
            2
        );

        symlink(home.join("mise.toml"), project.join("mise.toml")).unwrap();
        let linked_file = reports(&project.join("mise.toml"));
        assert!(!linked_file.contains("s3cr3t"), "{linked_file}");
        assert!(linked_file.contains("mise.toml:2:7: "), "{linked_file}");
        assert!(linked_file.contains("line 2, column 7: "), "{linked_file}");

        symlink(&home, project.join(".config")).unwrap();
        let linked_dir = reports(&project.join(".config/mise.toml"));
        assert!(!linked_dir.contains("s3cr3t"), "{linked_dir}");

        // A project reached through a link of the user's own is still quoted.
        let via = tmp.join("via");
        symlink(&home, &via).unwrap();
        assert_eq!(reports(&via.join("mise.toml")).matches("s3cr3t").count(), 2);
    }
}
