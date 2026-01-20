#![allow(unused_assignments)] // Fields are read by miette's derive macros at runtime

use crate::file::display_path;
use miette::{Diagnostic, NamedSource, SourceSpan};
use std::fmt;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// A TOML parsing error with source location information for rich display.
#[derive(Debug, Error, Diagnostic)]
#[error("Invalid TOML in config file: {}", path.display())]
#[diagnostic(code(mise::config::parse_error))]
pub struct TomlParseError {
    path: PathBuf,
    #[source_code]
    src: NamedSource<String>,
    #[label("{message}")]
    span: SourceSpan,
    message: String,
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
    pub fn new<D: Diagnostic + Send + Sync + 'static>(diagnostic: D) -> Self {
        let message = diagnostic.to_string();
        let rendered = format!("{:?}", miette::Report::new(diagnostic));
        MiseDiagnostic { message, rendered }
    }

    /// Get the pre-rendered miette output
    pub fn render(&self) -> &str {
        &self.rendered
    }
}

/// Create an eyre error from a toml::de::Error with rich source context.
pub fn toml_parse_error(err: &toml::de::Error, source: &str, path: &Path) -> eyre::Report {
    let message = err.message().to_string();

    // Get the byte span from toml error
    let span = err
        .span()
        .map(|r| SourceSpan::from((r.start, r.end.saturating_sub(r.start))))
        .unwrap_or_else(|| SourceSpan::from((0, 0)));

    let diagnostic = TomlParseError {
        path: path.to_path_buf(),
        src: NamedSource::new(display_path(path), source.to_string()),
        span,
        message,
    };

    eyre::Report::new(MiseDiagnostic::new(diagnostic))
}
