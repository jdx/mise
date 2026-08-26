#![allow(unused_assignments)] // Fields are read by miette's derive macros at runtime

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
pub(crate) struct MiseDiagnostic {
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
    pub(crate) fn render(&self) -> &str {
        &self.rendered
    }
}

/// Create an eyre error from a toml::de::Error with rich source context.
pub(crate) fn toml_parse_error(err: &toml::de::Error, source: &str, path: &Path) -> eyre::Report {
    let message = err.message().to_string();

    // Get the byte span from toml error
    let span = err
        .span()
        .map(|r| SourceSpan::from((r.start, r.end.saturating_sub(r.start))))
        .unwrap_or_else(|| SourceSpan::from((0, 0)));

    let help = backslash_help(source, &span, &message);
    let diagnostic = TomlParseError {
        src: NamedSource::new(display_path(path), source.to_string()),
        span,
        message,
        help,
    };

    eyre::Report::new(MiseDiagnostic::new(diagnostic))
}
