#![allow(unused_assignments)] // Fields are read by miette's derive macros at runtime

use crate::dirs;
use crate::file::display_path;
use miette::{Diagnostic, NamedSource, SourceSpan};
use std::fmt;
use std::path::{Path, PathBuf};
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
pub(crate) fn may_quote(path: &Path) -> bool {
    may_quote_below(path, path.parent().unwrap_or(path))
}

/// [`may_quote`] for a file found under `root`, such as a task file found in a
/// task directory of the project at `root`: every directory between `root` and
/// the file is checked too, whatever its name.
///
/// Config discovery starts in the working directory and its ancestors, and a
/// configured filename may name any path below them
/// (`override_config_filenames = ["custom/private.toml"]`), so the check also
/// reaches back to the deepest directory the file shares with the working
/// directory. The working directory comes from the OS already resolved, so the
/// directories it shares hold no link the project put there.
pub(crate) fn may_quote_below(path: &Path, root: &Path) -> bool {
    path.starts_with(*dirs::CONFIG)
        || path.starts_with(*dirs::SYSTEM_CONFIG)
        || !reached_through_link(path, &discovery_root(path, root))
}

/// The shallower of `root` and the deepest ancestor `path` shares with the
/// working directory.
fn discovery_root(path: &Path, root: &Path) -> PathBuf {
    let Some(cwd) = dirs::CWD.as_deref() else {
        return root.to_path_buf();
    };
    match path.ancestors().find(|dir| cwd.starts_with(dir)) {
        Some(shared) if root.starts_with(shared) => shared.to_path_buf(),
        _ => root.to_path_buf(),
    }
}

/// Whether `path`, a directory between `root` and it, or a config directory
/// mise descended into to find it (`.config`, `.mise`, `mise`, `conf.d` and its
/// folders, task directories), is a symlink. `root` and the directories above
/// it are not checked: the user chose to be there, and macOS keeps `/tmp` and
/// `/var` behind links.
fn reached_through_link(path: &Path, root: &Path) -> bool {
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
    let below_root = |dir: &Path| dir != root && dir.starts_with(root);
    let mut current = path;
    loop {
        if current.is_symlink() {
            return true;
        }
        match current.parent() {
            Some(parent) if below_root(parent) || is_layout_dir(parent) => current = parent,
            _ => return false,
        }
    }
}

/// Whether a project version file (`.tool-versions`, `.node-version`, ...)
/// is reached through a symlink, as [`may_quote`] decides, to a file of
/// another name. Such a file is not read: a version file links to another
/// version file, while a link to `~/.netrc` or a token file would put that
/// file's text into tool names and versions, which mise prints in warnings,
/// `mise ls` and install errors, and puts into download URLs.
///
/// The user's own files are exempt: global config such as `~/.tool-versions`,
/// and any version file directly in the home directory, which a dotfiles
/// manager may well link to a file of another name. Unlike [`may_quote`], this
/// runs after settings load, so it can ask which files are global.
pub(crate) fn links_to_another_name(path: &Path) -> bool {
    if may_quote(path)
        || path.parent() == Some(&**dirs::HOME)
        || crate::config::is_global_config(path)
    {
        return false;
    }
    let target = path.canonicalize().ok();
    target.as_deref().and_then(Path::file_name) != path.file_name()
}

/// The warning for a version file [`links_to_another_name`] skips.
pub(crate) fn warn_linked_version_file(path: &Path) {
    warn_once!(
        "not reading {}: it is a symlink to a file of another name, and mise reads a linked version file only when the target has the same name",
        display_path(path)
    );
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
/// failing line and its message can quote a value, so only the position is kept
/// unless [`may_quote`] allows `path` to be quoted.
pub(crate) fn hide_linked_source(err: toml::de::Error, source: &str, path: &Path) -> eyre::Report {
    hide_linked_source_below(err, source, path, path.parent().unwrap_or(path))
}

/// [`hide_linked_source`] for a file found under `root`, see [`may_quote_below`].
pub(crate) fn hide_linked_source_below(
    err: toml::de::Error,
    source: &str,
    path: &Path,
    root: &Path,
) -> eyre::Report {
    if may_quote_below(path, root) {
        return err.into();
    }
    let (line, column) = position(source, err.span().map_or(0, |r| r.start));
    // Not the parser's message either: a value error quotes the value
    // (`invalid type: string "…"`).
    eyre::eyre!("TOML parse error at line {line}, column {column}: {LINKED_DETAILS_HIDDEN}")
}

/// What a parse error about a linked file says in place of the parser's
/// message, which can quote the file's text.
const LINKED_DETAILS_HIDDEN: &str =
    "details hidden because the file is reached through a link out of the project";

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
            "Invalid TOML in config file {}:{line}:{column}: {LINKED_DETAILS_HIDDEN}",
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

    /// A value error quotes the value, so a linked file's parser message is
    /// withheld too, not only its source excerpt.
    #[test]
    fn hides_the_value_a_linked_file_s_error_would_quote() {
        const VALUE: &str = "silent = \"s3cr3t\"\n";
        let tmp = tempfile::tempdir().unwrap();
        let tmp = tmp.path().canonicalize().unwrap();
        let (home, project) = (tmp.join("home"), tmp.join("project"));
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(home.join("private.toml"), VALUE).unwrap();
        symlink(home.join("private.toml"), project.join("mise.toml")).unwrap();
        let path = project.join("mise.toml");

        let err = toml::from_str::<std::collections::HashMap<String, bool>>(VALUE).unwrap_err();
        assert!(err.message().contains("s3cr3t"), "{}", err.message());
        let rich = format!("{:?}", toml_parse_error(&err, VALUE, &path));
        let plain = format!("{:?}", hide_linked_source(err, VALUE, &path));
        for report in [rich, plain] {
            assert!(!report.contains("s3cr3t"), "{report}");
            assert!(report.contains(LINKED_DETAILS_HIDDEN), "{report}");
        }
    }

    #[test]
    fn quotes_a_file_but_not_what_a_link_leads_to() {
        let tmp = tempfile::tempdir().unwrap();
        // resolved, as the working directory mise starts from is: macOS keeps
        // its temp dirs behind a link
        let tmp = tmp.path().canonicalize().unwrap();
        let tmp = tmp.as_path();
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

        // The directory discovery starts from, and those above it, are not
        // checked: a project reached through a link of the user's own is
        // still quoted.
        let via = tmp.join("via");
        symlink(&home, &via).unwrap();
        assert!(!reached_through_link(&via.join("mise.toml"), &via));

        // Below a known root every directory counts, whatever its name, but the
        // root itself may sit behind a link.
        let tasks = project.join("mise-tasks");
        std::fs::create_dir_all(&tasks).unwrap();
        symlink(&home, tasks.join("group")).unwrap();
        assert!(!may_quote_below(&tasks.join("group/mise.toml"), &project));

        // A configured filename can name a path below the discovery directory
        // (`custom/private.toml`), whose directories count too.
        symlink(&home, project.join("custom")).unwrap();
        let custom = project.join("custom/mise.toml");
        assert!(reached_through_link(&custom, &project));
        // Even when the caller only knows the file's own directory, the check
        // reaches back to the directory the file shares with the working one.
        assert!(!may_quote_below(&custom, &project.join("custom")));
        assert!(!may_quote(&custom));
    }

    #[test]
    fn a_version_file_linked_to_another_name_is_not_read() {
        let tmp = tempfile::tempdir().unwrap();
        let tmp = tmp.path();
        std::fs::write(tmp.join("netrc"), "password s3cr3t\n").unwrap();
        std::fs::create_dir(tmp.join("shared")).unwrap();
        std::fs::write(tmp.join("shared/.tool-versions"), "node 22\n").unwrap();
        let project = tmp.join("project");
        std::fs::create_dir(&project).unwrap();
        std::fs::write(project.join(".node-version"), "22\n").unwrap();
        symlink(tmp.join("netrc"), project.join(".tool-versions")).unwrap();
        symlink(tmp.join("shared/.tool-versions"), project.join("ok")).unwrap();
        std::fs::create_dir(project.join("sub")).unwrap();
        symlink(
            tmp.join("shared/.tool-versions"),
            project.join("sub/.tool-versions"),
        )
        .unwrap();

        assert!(links_to_another_name(&project.join(".tool-versions")));
        assert!(links_to_another_name(&project.join("ok")));
        assert!(!links_to_another_name(&project.join("sub/.tool-versions")));
        assert!(!links_to_another_name(&project.join(".node-version")));
    }
}
