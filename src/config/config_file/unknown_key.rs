//! The warning for a `mise.toml` key mise does not read: where it is, and the key it most likely
//! meant.
//!
//! ```text
//! unknown field in ~/src/app/mise.toml:4:1: tasks.build.depend (did you mean depends?)
//! ```

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

/// The warning for the unknown key at `segments` in the `mise.toml` at `file`, whose text is
/// `body`.
pub(crate) fn message(file: &Path, body: &str, segments: &[String]) -> String {
    let location = key_span(body, segments)
        .map(|span| {
            let (line, column) = line_column(body, span.start);
            format!(":{line}:{column}")
        })
        .unwrap_or_default();
    let suggestion = suggestion(segments)
        .map(|key| format!(" (did you mean {key}?)"))
        .unwrap_or_default();
    format!(
        "unknown field in {}{location}: {}{suggestion}",
        display_path(file),
        segments.join(".")
    )
}

/// The known key closest to the unknown one, among those its table may hold.
fn suggestion(segments: &[String]) -> Option<&'static str> {
    let (unknown, parent) = segments.split_last()?;
    let parent: Vec<&str> = parent.iter().map(String::as_str).collect();
    crate::fuzzy::nearest(unknown, crate::config::schema::known_keys(&parent))
}

/// Where the key at `segments` is written in `body`. Only tables are followed: an array's
/// elements are structs that reject an unknown key outright, so serde never skips one there.
fn key_span(body: &str, segments: &[String]) -> Option<Range<usize>> {
    let doc = toml_edit::Document::parse(body).ok()?;
    let mut table: &dyn TableLike = doc.as_table();
    let mut span = None;
    for segment in segments {
        let (key, item) = table.get_key_value(segment)?;
        span = key.span();
        table = match item {
            Item::Table(t) => t,
            Item::Value(Value::InlineTable(t)) => t,
            _ => break,
        };
    }
    span
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
        unknown
            .iter()
            .map(|segments| message(Path::new("mise.toml"), body, segments))
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
    fn omits_a_suggestion_nothing_is_close_to() {
        assert_eq!(
            warn("zzzzzz = 1\n"),
            ["unknown field in mise.toml:1:1: zzzzzz"]
        );
    }
}
