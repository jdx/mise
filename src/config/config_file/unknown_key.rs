//! The warning for a `mise.toml` key mise does not read: where it is, and the key it most likely
//! meant.
//!
//! ```text
//! unknown field in ~/src/app/mise.toml:4:1: tasks.build.depend (did you mean depends?)
//! ```

use std::cell::OnceCell;
use std::ops::Range;
use std::path::Path;

use toml_edit::{Item, TableLike, Value};

use crate::file::display_path;

/// The key path serde skipped, as the table keys and array indices that lead to it.
pub(crate) fn segments(path: &serde_ignored::Path) -> Vec<String> {
    use serde_ignored::Path;
    let mut out = match path {
        Path::Root => return vec![],
        Path::Seq { parent, .. }
        | Path::Map { parent, .. }
        | Path::Some { parent }
        | Path::NewtypeStruct { parent }
        | Path::NewtypeVariant { parent } => segments(parent),
    };
    match path {
        Path::Seq { index, .. } => out.push(index.to_string()),
        Path::Map { key, .. } => out.push(key.clone()),
        _ => {}
    }
    out
}

/// Words the warnings for one config body, parsing it for key positions at most once, and
/// only when there is something to warn about.
pub(crate) struct UnknownKeys<'a> {
    file: &'a Path,
    /// The text the keys are in, when `file` holds it; without it, a warning has no line.
    body: Option<&'a str>,
    doc: OnceCell<Option<toml_edit::Document<&'a str>>>,
}

impl<'a> UnknownKeys<'a> {
    pub(crate) fn new(file: &'a Path, body: Option<&'a str>) -> Self {
        Self {
            file,
            body,
            doc: OnceCell::new(),
        }
    }

    /// The warning for the unknown key at `segments`.
    pub(crate) fn message(&self, segments: &[String]) -> String {
        let location = self
            .body
            .zip(self.key_span(segments))
            .map(|(body, span)| {
                let (line, column) = line_column(body, span.start);
                format!(":{line}:{column}")
            })
            .unwrap_or_default();
        let suggestion = suggestion(segments)
            .map(|key| format!(" (did you mean {key}?)"))
            .unwrap_or_default();
        format!(
            "unknown field in {}{location}: {}{suggestion}",
            display_path(self.file),
            segments.join(".")
        )
    }

    /// Where the key at `segments` is written, following tables and the array indices serde
    /// names among the segments.
    fn key_span(&self, segments: &[String]) -> Option<Range<usize>> {
        let doc = self
            .doc
            .get_or_init(|| {
                self.body
                    .and_then(|body| toml_edit::Document::parse(body).ok())
            })
            .as_ref()?;
        let mut table: &dyn TableLike = doc.as_table();
        let mut segments = segments.iter();
        let mut span = None;
        while let Some(segment) = segments.next() {
            let (key, item) = table.get_key_value(segment)?;
            span = key.span();
            table = match item {
                Item::Table(t) => t,
                Item::Value(Value::InlineTable(t)) => t,
                Item::ArrayOfTables(tables) => match segments.next() {
                    Some(index) => tables.get(index.parse().ok()?)?,
                    None => break,
                },
                Item::Value(Value::Array(values)) => match segments.next() {
                    Some(index) => match values.get(index.parse().ok()?)? {
                        Value::InlineTable(t) => t,
                        _ => break,
                    },
                    None => break,
                },
                _ => break,
            };
        }
        span
    }
}

/// The known key closest to the unknown one, among those its table may hold.
fn suggestion(segments: &[String]) -> Option<&'static str> {
    let (unknown, parent) = segments.split_last()?;
    let parent: Vec<&str> = parent.iter().map(String::as_str).collect();
    crate::fuzzy::nearest(unknown, crate::config::schema::known_keys(&parent))
}

/// The 1-based line and column of byte `offset` in `body`.
fn line_column(body: &str, offset: usize) -> (usize, usize) {
    let before = &body[..offset.min(body.len())];
    let line = before.matches('\n').count() + 1;
    let column = before
        .rsplit('\n')
        .next()
        .map_or(0, |line| line.chars().count())
        + 1;
    (line, column)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn warn(body: &str) -> Vec<String> {
        let mut unknown = vec![];
        let des = toml::Deserializer::parse(body).unwrap();
        let _: crate::config::config_file::mise_toml::MiseToml =
            serde_ignored::deserialize(des, |path| unknown.push(segments(&path))).unwrap();
        let keys = UnknownKeys::new(Path::new("mise.toml"), Some(body));
        unknown
            .iter()
            .map(|segments| keys.message(segments))
            .collect()
    }

    #[test]
    fn points_at_the_key_and_suggests_a_near_one() {
        let body = "[tools]\nnode = \"22\"\n\n[settings]\nexperimantal = true\n";
        assert_eq!(
            warn(body),
            ["unknown field in mise.toml:5:1: settings.experimantal (did you mean experimental?)"]
        );
        assert_eq!(
            warn("[tool]\nnode = \"22\"\n"),
            ["unknown field in mise.toml:1:2: tool (did you mean tools?)"]
        );
    }

    #[test]
    fn reaches_inline_and_dotted_tables() {
        assert_eq!(
            warn("task_config = { dir = \"x\", includs = [] }\n"),
            ["unknown field in mise.toml:1:28: task_config.includs (did you mean includes?)"]
        );
        assert_eq!(
            warn("monorepo.config_root = [\"a\"]\n"),
            ["unknown field in mise.toml:1:10: monorepo.config_root (did you mean config_roots?)"]
        );
    }

    #[test]
    fn follows_arrays_of_tables() {
        let body = "[[watch_files]]\npatterns = [\"*.rs\"]\nrun = \"x\"\nshel = \"bash\"\n";
        assert_eq!(
            warn(body),
            ["unknown field in mise.toml:4:1: watch_files.0.shel (did you mean shell?)"]
        );
        let body = "watch_files = [{ patterns = [\"*.rs\"], run = \"x\", shel = \"bash\" }]\n";
        assert_eq!(
            warn(body),
            ["unknown field in mise.toml:1:50: watch_files.0.shel (did you mean shell?)"]
        );
    }

    #[test]
    fn has_no_line_without_the_body() {
        let keys = UnknownKeys::new(Path::new("mise.toml"), None);
        assert_eq!(
            keys.message(&["tool".to_string()]),
            "unknown field in mise.toml: tool (did you mean tools?)"
        );
    }

    #[test]
    fn omits_a_suggestion_nothing_is_close_to() {
        assert_eq!(
            warn("zzzzzz = 1\n"),
            ["unknown field in mise.toml:1:1: zzzzzz"]
        );
    }
}
