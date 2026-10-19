//! Completion: the keys the schema names for the table the cursor is in, and the values a key
//! accepts as literals.
//!
//! The editor sends documents that are half-typed, so this reads the text around the cursor
//! instead of parsing the whole document: the nearest `[header]` above it says which table it
//! is in, and the line says whether it is typing a key or a value.

use std::ops::Range;

use serde_json::Value;

use crate::keypath;
use crate::schema::Schema;
use crate::text::LineIndex;

pub(crate) enum Kind {
    Key,
    Value,
}

pub(crate) struct Item {
    pub label: String,
    pub insert: String,
    pub kind: Kind,
    pub detail: Option<String>,
    pub documentation: Option<String>,
    /// The text the insertion replaces, as byte offsets.
    pub replace: Range<usize>,
}

pub(crate) fn complete(text: &str, offset: usize, schema: &Schema) -> Vec<Item> {
    let index = LineIndex::new(text);
    let (line, _) = index.position(offset);
    let line_start = index.offset(line, 0);
    let line_text = index.line(line);
    let column = offset.saturating_sub(line_start).min(line_text.len());
    let (before, after) = line_text.split_at(column);
    if in_comment(before) {
        return vec![];
    }
    let trimmed = before.trim_start();
    if let Some(header) = trimmed.strip_prefix('[') {
        let header = header.trim_start_matches('[');
        return key_items(schema, &[], header, offset);
    }
    let table = table_path(&index, line);
    match unquoted_equals(before) {
        Some(eq) => {
            let mut path = table;
            path.extend(keypath::split(&before[..eq]));
            value_items(schema, &path, &before[eq + 1..], after, offset)
        }
        None => key_items(schema, &table, trimmed, offset),
    }
}

/// Keys for the segment being typed at the end of `typed`, which is a dotted key or header.
fn key_items(schema: &Schema, table: &[String], typed: &str, offset: usize) -> Vec<Item> {
    let mut segments = keypath::split(typed);
    let partial = segments.pop().unwrap_or_default();
    let mut parent = table.to_vec();
    parent.extend(segments);
    let raw = typed.rsplit('.').next().unwrap_or(typed).trim_start();
    let replace = offset - raw.len()..offset;
    let needle = partial.to_lowercase();
    schema
        .keys(&parent)
        .into_iter()
        .filter(|(name, schema_)| {
            name.to_lowercase().starts_with(&needle) && !schema.schema_is_deprecated(schema_)
        })
        .map(|(name, key_schema)| {
            let description = schema.schema_description(key_schema);
            Item {
                label: name.to_string(),
                insert: keypath::quote(name),
                kind: Kind::Key,
                detail: description
                    .as_deref()
                    .and_then(|d| d.lines().next())
                    .map(str::to_string),
                documentation: description,
                replace: replace.clone(),
            }
        })
        .collect()
}

/// Literal values for the key at `path`, given what follows its `=`.
fn value_items(
    schema: &Schema,
    path: &[String],
    typed: &str,
    after: &str,
    offset: usize,
) -> Vec<Item> {
    let raw = typed.trim_start();
    let replace = offset - raw.len()..offset;
    let quoted = raw.strip_prefix('"');
    let needle = quoted.unwrap_or(raw);
    schema
        .values(&schema.at(path))
        .into_iter()
        .filter_map(|value| {
            let (label, insert) = match (&value, quoted.is_some()) {
                (Value::String(s), true) => {
                    let close = if after.starts_with('"') { "" } else { "\"" };
                    (s.clone(), format!("\"{s}{close}"))
                }
                (Value::String(s), false) => (s.clone(), format!("\"{s}\"")),
                (Value::Bool(_) | Value::Number(_), false) => {
                    (value.to_string(), value.to_string())
                }
                _ => return None,
            };
            label.starts_with(needle).then_some(Item {
                label,
                insert,
                kind: Kind::Value,
                detail: None,
                documentation: None,
                replace: replace.clone(),
            })
        })
        .collect()
}

/// The table a line belongs to: the nearest header above it.
fn table_path(index: &LineIndex, line: usize) -> Vec<String> {
    for l in (0..line).rev() {
        let text = index.line(l).trim();
        if let Some(rest) = text.strip_prefix('[') {
            let rest = rest.trim_start_matches('[');
            let end = unquoted(rest, ']').unwrap_or(rest.len());
            return keypath::split(&rest[..end]);
        }
    }
    vec![]
}

fn unquoted_equals(text: &str) -> Option<usize> {
    unquoted(text, '=')
}

fn in_comment(text: &str) -> bool {
    unquoted(text, '#').is_some()
}

/// The byte offset of the first `target` outside a quoted string.
fn unquoted(text: &str, target: char) -> Option<usize> {
    let mut quote = None;
    for (i, c) in text.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, c) if c == target => return Some(i),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::tests::sample;

    /// Completes at the `|` in `marked`.
    fn labels(marked: &str) -> Vec<String> {
        let offset = marked.find('|').unwrap();
        let text = marked.replacen('|', "", 1);
        complete(&text, offset, &sample())
            .into_iter()
            .map(|i| i.label)
            .collect()
    }

    #[test]
    fn top_level_keys_skip_deprecated_ones() {
        assert_eq!(labels("|"), ["min_version", "settings", "tools"]);
        assert_eq!(labels("se|"), ["settings"]);
    }

    #[test]
    fn keys_follow_the_table_header() {
        assert_eq!(labels("[settings]\n|"), ["color", "mode"]);
        assert_eq!(labels("[settings]\nco|"), ["color"]);
    }

    #[test]
    fn dotted_keys_complete_in_the_named_table() {
        assert_eq!(labels("settings.|"), ["color", "mode"]);
    }

    #[test]
    fn headers_complete_too() {
        assert_eq!(labels("[set|"), ["settings"]);
        assert_eq!(labels("[settings.m|"), ["mode"]);
    }

    #[test]
    fn values_come_from_the_schema() {
        assert_eq!(labels("[settings]\ncolor = |"), ["true", "false"]);
        assert_eq!(labels("[settings]\nmode = \"|"), ["a", "b"]);
        assert_eq!(labels("[settings]\nmode = \"b|"), ["b"]);
    }

    #[test]
    fn comments_complete_nothing() {
        assert!(labels("# se|").is_empty());
    }

    #[test]
    fn a_closing_quote_is_not_doubled() {
        let text = "[settings]\nmode = \"\"";
        let offset = text.len() - 1;
        let items = complete(text, offset, &sample());
        assert_eq!(items[0].insert, "\"a");
    }
}
