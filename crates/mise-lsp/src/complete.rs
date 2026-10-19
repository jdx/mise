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
    let (table, in_string) = scan(&index, line, before);
    if in_string {
        return vec![];
    }
    if let Some(header) = trimmed.strip_prefix('[') {
        let header = header.trim_start_matches('[');
        return key_items(schema, &[], header, after, offset);
    }
    match unquoted_equals(before) {
        Some(eq) => {
            let mut path = table;
            path.extend(keypath::split(&before[..eq]));
            value_items(schema, &path, &before[eq + 1..], after, offset)
        }
        None => key_items(schema, &table, trimmed, after, offset),
    }
}

/// Keys for the segment being typed at the end of `typed`, which is a dotted key or header.
fn key_items(
    schema: &Schema,
    table: &[String],
    typed: &str,
    after: &str,
    offset: usize,
) -> Vec<Item> {
    let mut segments = keypath::split(typed);
    let partial = segments.pop().unwrap_or_default();
    let mut parent = table.to_vec();
    parent.extend(segments);
    let raw = typed.rsplit('.').next().unwrap_or(typed).trim_start();
    let rest = after
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .unwrap_or(after.len());
    let replace = offset - raw.len()..offset + rest;
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
    let quoted = raw.strip_prefix('"');
    // The rest of the value under the cursor goes too: up to the closing quote in a string,
    // else the rest of the word.
    let rest = match quoted {
        Some(_) => after.find('"').unwrap_or(after.len()),
        None => after
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.'))
            .unwrap_or(after.len()),
    };
    let replace = offset - raw.len()..offset + rest;
    let needle = quoted.unwrap_or(raw);
    schema
        .values(&schema.at(path))
        .into_iter()
        .filter_map(|value| {
            let (label, insert) = match (&value, quoted.is_some()) {
                (Value::String(s), true) => {
                    let close = if after.contains('"') { "" } else { "\"" };
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

/// The table a line belongs to, and whether the cursor is inside a value that spans lines: a
/// multiline string, or an array or inline table still open.
///
/// The table is the nearest header above the line that is not itself string content. A header
/// of an array of tables (`[[watch_files]]`) names its latest element, so the path ends in an
/// index, which is how the schema walk enters an array's items. `before` is the text of the
/// cursor's line up to the cursor.
fn scan(index: &LineIndex, line: usize, before: &str) -> (Vec<String>, bool) {
    let mut table = vec![];
    let mut state = State::default();
    for l in 0..line {
        let text = index.line(l);
        if state.is_closed()
            && let Some(rest) = text.trim().strip_prefix('[')
        {
            let array = rest.starts_with('[');
            let rest = rest.trim_start_matches('[');
            let end = unquoted(rest, ']').unwrap_or(rest.len());
            table = keypath::split(&rest[..end]);
            if array {
                table.push("0".to_string());
            }
            // A header's brackets are not values, even when it is unfinished.
            continue;
        }
        state = advance(text, state);
    }
    // Brackets on the cursor's own line are the header or value being typed.
    let inside = state.depth > 0 || advance(before, state).open.is_some();
    (table, inside)
}

/// What is still open at a line break: a multiline string (by its quote) and the arrays and
/// inline tables that have not closed.
#[derive(Clone, Copy, Default)]
struct State {
    open: Option<char>,
    depth: usize,
}

impl State {
    fn is_closed(self) -> bool {
        self.open.is_none() && self.depth == 0
    }
}

/// The state at the end of `text`, given the state at its start. Quotes and brackets only count
/// outside comments and single-line strings.
fn advance(text: &str, mut state: State) -> State {
    let chars: Vec<char> = text.chars().collect();
    let triple = |i: usize, q: char| chars.get(i..i + 3) == Some(&[q, q, q][..]);
    let mut single = None;
    let mut closing = 0;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if let Some(q) = state.open {
            if q == '"' && c == '\\' {
                i += 2;
            } else if triple(i, q) {
                state.open = None;
                // Up to two more quotes belong to the string's end: `'''it's''''`.
                i += 3;
                while i < chars.len() && chars[i] == q && closing < 2 {
                    i += 1;
                    closing += 1;
                }
                closing = 0;
            } else {
                i += 1;
            }
            continue;
        }
        if let Some(q) = single {
            if q == '"' && c == '\\' {
                i += 2;
                continue;
            }
            if c == q {
                single = None;
            }
        } else {
            match c {
                '#' => break,
                '"' | '\'' if triple(i, c) => {
                    state.open = Some(c);
                    i += 3;
                    continue;
                }
                '"' | '\'' => single = Some(c),
                '[' | '{' => state.depth += 1,
                ']' | '}' => state.depth = state.depth.saturating_sub(1),
                _ => {}
            }
        }
        i += 1;
    }
    state
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
        assert_eq!(labels("|"), ["list", "min_version", "settings", "tools"]);
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

    /// Completes at the `|` and applies the first item to the text.
    fn applied(marked: &str) -> String {
        let offset = marked.find('|').unwrap();
        let text = marked.replacen('|', "", 1);
        let item = complete(&text, offset, &sample()).remove(0);
        format!(
            "{}{}{}",
            &text[..item.replace.start],
            item.insert,
            &text[item.replace.end..]
        )
    }

    #[test]
    fn completing_inside_a_word_replaces_all_of_it() {
        assert_eq!(
            applied("[settings]\nco|lor = true"),
            "[settings]\ncolor = true"
        );
        assert_eq!(
            applied("[settings]\ncolor = tr|ue"),
            "[settings]\ncolor = true"
        );
        assert_eq!(
            applied("[settings]\nmode = \"|b\""),
            "[settings]\nmode = \"a\""
        );
    }

    #[test]
    fn array_tables_enter_their_items() {
        assert_eq!(labels("[[list]]\n|"), ["name"]);
    }

    #[test]
    fn multiline_strings_do_not_open_tables() {
        let text = "[settings]\nnote = \"\"\"\n[[ -f x ]] && y\n\"\"\"\n|";
        assert_eq!(labels(text), ["color", "mode"]);
        assert!(labels("[settings]\nnote = \"\"\"\nco|").is_empty());
    }

    #[test]
    fn triple_quotes_in_comments_and_literals_open_nothing() {
        assert_eq!(labels("[settings]\n# \"\"\"\nco|"), ["color"]);
        assert_eq!(labels("[settings]\nnote = '\"\"\"'\nco|"), ["color"]);
        assert_eq!(labels("[settings]\nnote = \"a\\\"\"\"\"\nco|"), ["color"]);
    }

    #[test]
    fn a_multiline_string_opened_on_the_cursor_line_completes_nothing() {
        assert!(labels("[settings]\nnote = \"\"\"co|").is_empty());
    }

    #[test]
    fn multiline_arrays_are_not_tables() {
        let text = "[settings]\nlist = [\n  [\"a\"],\n  [[1]],\n]\nco|";
        assert_eq!(labels(text), ["color"]);
        assert!(labels("[settings]\nlist = [\n  [\"a\"],\n  |").is_empty());
        assert!(labels("[settings]\nt = {\n|").is_empty());
    }

    #[test]
    fn quotes_before_a_closing_delimiter_belong_to_the_string() {
        let text = "[settings]\nrun = [\'\'\'echo \'one\'\'\'\', \'\'\'echo two\'\'\']\nco|";
        assert_eq!(labels(text), ["color"]);
    }

    #[test]
    fn an_unfinished_header_does_not_hide_later_ones() {
        assert_eq!(labels("[settings\n[settings]\nco|"), ["color"]);
        assert_eq!(labels("[settings\nco|"), ["color"]);
    }

    #[test]
    fn a_closing_quote_is_not_doubled() {
        let text = "[settings]\nmode = \"\"";
        let offset = text.len() - 1;
        let items = complete(text, offset, &sample());
        assert_eq!(items[0].insert, "\"a");
    }
}
