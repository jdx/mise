//! Completion: the keys the schema names for the table the cursor is in, and the values a key
//! accepts as literals.
//!
//! The editor sends documents that are half-typed, so this reads the text around the cursor
//! instead of parsing the whole document. A small lexer walks the text above the cursor and
//! keeps what is open there: the nearest `[header]` says which table it is in, and the arrays
//! and inline tables still open say how deep, with the key each one belongs to. What is typed
//! since the last separator says whether the cursor is on a key or a value.

use std::ops::Range;

use serde_json::Value;

use crate::doc;
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
    if unquoted(before, '#').is_some() {
        return vec![];
    }
    let (table, state, line_starts_closed) = scan(&index, line, before);
    if state.open.is_some() {
        return vec![];
    }
    let frame = state.frame();
    // A header: a line that starts with `[` where nothing is open.
    if line_starts_closed && let Some(header) = before.trim_start().strip_prefix('[') {
        let header = header.trim_start_matches('[');
        return key_items(schema, &[], header, text, after, offset);
    }
    match &frame.pending {
        Some(path) => {
            let path = if frame.kind == '[' {
                [path.as_slice(), &["0".to_string()]].concat()
            } else {
                path.clone()
            };
            value_items(schema, &path, &frame.segment, after, offset)
        }
        None if frame.kind == '[' => {
            let path = [frame.path.as_slice(), &["0".to_string()]].concat();
            value_items(schema, &path, &frame.segment, after, offset)
        }
        None => {
            let parent = if state.stack.is_empty() {
                table
            } else {
                frame.path.clone()
            };
            key_items(schema, &parent, &frame.segment, text, after, offset)
        }
    }
}

/// Keys for the segment being typed at the end of `typed`, which is a dotted key or header. The
/// keys the table already has are not offered again.
fn key_items(
    schema: &Schema,
    table: &[String],
    typed: &str,
    text: &str,
    after: &str,
    offset: usize,
) -> Vec<Item> {
    let typed = typed.trim_start();
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
    let existing = existing_keys(text, &replace, &parent);
    schema
        .keys(&parent)
        .into_iter()
        .filter(|(name, key_schema)| {
            name.to_lowercase().starts_with(&needle)
                && !schema.schema_is_deprecated(key_schema)
                && !existing.iter().any(|e| e == name)
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

/// The keys the table at `parent` has, with the text being completed taken out so that a
/// half-typed key does not stop the document from parsing. Nothing is known while it still does
/// not parse.
fn existing_keys(text: &str, replace: &Range<usize>, parent: &[String]) -> Vec<String> {
    let mut without = String::with_capacity(text.len());
    without.push_str(&text[..replace.start]);
    without.push_str(&text[replace.end..]);
    let Ok(parsed) = doc::parse(&without) else {
        return vec![];
    };
    parsed
        .entries
        .iter()
        .filter(|e| e.path.len() == parent.len() + 1 && e.path.starts_with(parent))
        .map(|e| e.path[parent.len()].clone())
        .collect()
}

/// Literal values for the key at `path`, given what follows its `=` (or, in an array, the
/// element so far).
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

/// One level of what is open: the top of a table (`t`), an array (`[`) or an inline table (`{`).
#[derive(Clone)]
struct Frame {
    kind: char,
    /// The schema path of the table, or of the array or inline table, this level is.
    path: Vec<String>,
    /// What has been written since the last separator at this level.
    segment: String,
    /// The path of the key whose `=` has been seen at this level, while its value is written.
    pending: Option<Vec<String>>,
}

impl Frame {
    fn new(kind: char, path: Vec<String>) -> Self {
        Self {
            kind,
            path,
            segment: String::new(),
            pending: None,
        }
    }

    fn separate(&mut self) {
        self.segment.clear();
        self.pending = None;
    }
}

/// What is open at a position: a multiline string (by its quote), and the arrays and inline
/// tables that have not closed, outermost first. `base` is the table the line is in.
#[derive(Clone)]
struct State {
    open: Option<char>,
    base: Frame,
    stack: Vec<Frame>,
}

impl State {
    fn new(table: Vec<String>) -> Self {
        Self {
            open: None,
            base: Frame::new('t', table),
            stack: vec![],
        }
    }

    fn closed(&self) -> bool {
        self.open.is_none() && self.stack.is_empty()
    }

    fn frame(&self) -> &Frame {
        self.stack.last().unwrap_or(&self.base)
    }

    fn frame_mut(&mut self) -> &mut Frame {
        self.stack.last_mut().unwrap_or(&mut self.base)
    }
}

/// The table the cursor's line is in and what is open at the cursor. A header's brackets are not
/// values, and neither are those inside a string or comment.
fn scan(index: &LineIndex, line: usize, before: &str) -> (Vec<String>, State, bool) {
    let mut table = vec![];
    let mut state = State::new(vec![]);
    for l in 0..line {
        let text = index.line(l);
        // A header of an array of tables (`[[watch_files]]`) names its latest element, so its
        // path ends in an index, which is how the schema walk enters an array's items.
        if state.closed()
            && let Some(rest) = text.trim().strip_prefix('[')
        {
            let array = rest.starts_with('[');
            let rest = rest.trim_start_matches('[');
            let end = unquoted(rest, ']').unwrap_or(rest.len());
            table = keypath::split(&rest[..end]);
            if array {
                table.push("0".to_string());
            }
            state = State::new(table.clone());
            continue;
        }
        advance(text, &mut state, true);
    }
    let line_starts_closed = state.closed();
    advance(before, &mut state, false);
    (table, state, line_starts_closed)
}

/// Runs `text`, a line or the start of one, through `state`. Quotes and brackets only count
/// outside comments and single-line strings. At the end of a whole line (`eol`) the line break
/// separates entries in an inline table and at the top of a table.
fn advance(text: &str, state: &mut State, eol: bool) {
    let chars: Vec<char> = text.chars().collect();
    let triple = |i: usize, q: char| chars.get(i..i + 3) == Some(&[q, q, q][..]);
    let mut single = None;
    let mut closing = 0;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if let Some(q) = state.open {
            state.frame_mut().segment.push(c);
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
            state.frame_mut().segment.push(c);
            if q == '"' && c == '\\' {
                if let Some(next) = chars.get(i + 1) {
                    state.frame_mut().segment.push(*next);
                }
                i += 2;
                continue;
            }
            if c == q {
                single = None;
            }
            i += 1;
            continue;
        }
        match c {
            '#' => break,
            '"' | '\'' if triple(i, c) => {
                state.open = Some(c);
                state.frame_mut().segment.push_str(&format!("{c}{c}{c}"));
                i += 3;
                continue;
            }
            '"' | '\'' => {
                single = Some(c);
                state.frame_mut().segment.push(c);
            }
            '=' => {
                let frame = state.frame_mut();
                if frame.pending.is_none() {
                    let mut path = frame.path.clone();
                    if frame.kind == '[' {
                        path.push("0".to_string());
                    }
                    path.extend(keypath::split(&frame.segment));
                    frame.pending = Some(path);
                    frame.segment.clear();
                } else {
                    frame.segment.push(c);
                }
            }
            ',' => state.frame_mut().separate(),
            '[' | '{' => {
                let parent = state.frame();
                let path = match (&parent.pending, parent.kind) {
                    (Some(pending), _) => pending.clone(),
                    (None, '[') => [parent.path.as_slice(), &["0".to_string()]].concat(),
                    (None, _) => parent.path.clone(),
                };
                state.stack.push(Frame::new(c, path));
            }
            ']' | '}' => {
                if state.stack.pop().is_some() {
                    // The value that just closed ends the entry it belonged to.
                    let frame = state.frame_mut();
                    if frame.kind != '[' {
                        frame.segment.clear();
                    }
                }
            }
            _ => state.frame_mut().segment.push(c),
        }
        i += 1;
    }
    if eol && state.open.is_none() && state.frame().kind != '[' {
        state.frame_mut().separate();
    }
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
    fn inline_tables_complete_their_keys() {
        assert_eq!(labels("settings = { | }"), ["color", "mode"]);
        assert_eq!(labels("settings = { color = true, | }"), ["mode"]);
        assert_eq!(labels("settings = {\n  color = true,\n  mo|\n}"), ["mode"]);
        assert_eq!(labels("settings = {\n  color = true\n  |\n}"), ["mode"]);
        assert_eq!(labels("settings = { mode = \"|\" }"), ["a", "b"]);
        assert_eq!(labels("settings = { color = |"), ["true", "false"]);
    }

    #[test]
    fn keys_already_set_are_not_offered_again() {
        assert_eq!(labels("[settings]\ncolor = true\n|"), ["mode"]);
        assert!(labels("[settings]\ncolor = true\nco|").is_empty());
        // a half-written document has no known keys yet
        assert_eq!(labels("[settings]\ncolor = \n|"), ["color", "mode"]);
    }

    #[test]
    fn a_closing_quote_is_not_doubled() {
        let text = "[settings]\nmode = \"\"";
        let offset = text.len() - 1;
        let items = complete(text, offset, &sample());
        assert_eq!(items[0].insert, "\"a");
    }
}
