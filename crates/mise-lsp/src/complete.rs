//! Completion: the keys the schema names for the table the cursor is in, and the values a key
//! accepts as literals.
//!
//! The editor sends documents that are half-typed, so this reads the text around the cursor
//! instead of parsing the whole document. A small lexer walks the text above the cursor and
//! keeps what is open there: the nearest `[header]` says which table it is in, and the arrays
//! and inline tables still open say how deep, with the key each one belongs to. What is typed
//! since the last separator says whether the cursor is on a key or a value.

use std::collections::HashMap;
use std::ops::Range;

use serde_json::Value;

use crate::doc;
use crate::keypath;
use crate::registry::{self, Registry};
use crate::schema::Schema;
use crate::tasks::{self, Candidates, Namespace};
use crate::text::LineIndex;

pub(crate) enum Kind {
    Key,
    Value,
    Task,
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

/// The items at `offset`. `tools` names the tools to offer under `[tools]`. `names` is asked for the tasks and templates the document can see
/// only when the cursor is where one is named.
pub(crate) fn complete(
    text: &str,
    offset: usize,
    schema: &Schema,
    tools: &Registry,
    names: &dyn Fn() -> Candidates,
) -> Vec<Item> {
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
        return key_items(schema, tools, &[], header, text, after, offset);
    }
    match &frame.pending {
        Some(path) => {
            let path = if frame.kind == '[' {
                [path.as_slice(), &[frame.index.to_string()]].concat()
            } else {
                path.clone()
            };
            value_items(schema, &path, &frame.segment, after, offset, names)
        }
        None if frame.kind == '[' => {
            let path = [frame.path.as_slice(), &[frame.index.to_string()]].concat();
            value_items(schema, &path, &frame.segment, after, offset, names)
        }
        None => {
            let parent = if state.stack.is_empty() {
                table
            } else {
                frame.path.clone()
            };
            key_items(schema, tools, &parent, &frame.segment, text, after, offset)
        }
    }
}

/// Keys for the segment being typed at the end of `typed`, which is a dotted key or header. The
/// keys the table already has are not offered again.
fn key_items(
    schema: &Schema,
    tools: &Registry,
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
    let raw = keypath::last_segment(typed);
    // A quote that is still open: `"llama.c|` and not `"llama.cpp"|`.
    let quote = raw
        .chars()
        .next()
        .filter(|c| matches!(c, '"' | '\'') && closing_quote(&raw[1..], *c).is_none());
    // A key typed in quotes goes through its closing quote, whatever is inside (`"llama.cpp"`).
    // Otherwise the rest of the word goes.
    let rest = match quote.and_then(|q| closing_quote(after, q).map(|i| i + 1)) {
        Some(through_quote) => through_quote,
        None => after
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
            .unwrap_or(after.len()),
    };
    let replace = offset - raw.len()..offset + rest;
    let needle = partial.to_lowercase();
    let existing = existing_keys(text, &replace, &parent, false);
    let mut items: Vec<Item> = schema
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
        .collect();
    if Registry::is_tools_table(&parent) {
        // a tool written as `[tools.node]` is listed too, unlike the tables of other keys
        let existing = existing_keys(text, &replace, &parent, true);
        items.extend(
            tools
                .offered()
                .filter(|tool| {
                    tool.name.to_lowercase().starts_with(&needle)
                        && !existing
                            .iter()
                            .any(|e| tools.get(e).is_some_and(|t| t.name == tool.name))
                })
                .map(|tool| Item {
                    label: tool.name.clone(),
                    insert: keypath::quote(&tool.name),
                    kind: Kind::Key,
                    detail: tool
                        .description
                        .as_deref()
                        .and_then(|d| d.lines().next())
                        .map(str::to_string),
                    documentation: Some(registry::describe(tool)),
                    replace: replace.clone(),
                }),
        );
    }
    items
}

/// The keys the table at `parent` has a value for, with the text being completed taken out so that a
/// half-typed key does not stop the document from parsing. Nothing is known while it still does
/// not parse.
fn existing_keys(
    text: &str,
    replace: &Range<usize>,
    parent: &[String],
    tables: bool,
) -> Vec<String> {
    // The text being completed goes, or turns into a placeholder key where its absence would not
    // parse, as in the header `[tools.no]` that would become `[tools.]`.
    let without =
        |filler: &str| format!("{}{filler}{}", &text[..replace.start], &text[replace.end..]);
    // A header still being typed may not be closed yet, so the placeholder closes it too.
    let Some(parsed) = [
        "",
        "_placeholder_",
        "_placeholder_]",
        "_placeholder_]]",
        // a dotted key on a line of its own needs a value
        "_placeholder_ = 0",
    ]
    .into_iter()
    .find_map(|filler| doc::parse(&without(filler)).ok()) else {
        return vec![];
    };
    parsed
        .entries
        .iter()
        .filter(|e| {
            (tables || !e.table) && e.path.len() == parent.len() + 1 && e.path.starts_with(parent)
        })
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
    names: &dyn Fn() -> Candidates,
) -> Vec<Item> {
    let raw = typed.trim_start();
    let quoted = raw.strip_prefix('"');
    let reference = tasks::reference_kind(path);
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
    if let Some(namespace) = reference {
        return reference_items(path, namespace, raw, after, offset, names);
    }
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

/// Where a string opened with `quote` ends in `text`, which starts just after the opening quote:
/// the first quote that a backslash does not escape (a literal `'` string has no escapes).
fn closing_quote(text: &str, quote: char) -> Option<usize> {
    let mut chars = text.char_indices();
    while let Some((i, c)) = chars.next() {
        if c == quote {
            return Some(i);
        }
        if c == '\\' && quote == '"' {
            chars.next();
        }
    }
    None
}

/// The tasks or templates to offer where one is named: in a string that may carry environment
/// assignments before the name and arguments after it (`"PROFILE=release bu|ild --fast"`).
/// Only the name is replaced; nothing is offered where an argument is being typed.
fn reference_items(
    path: &[String],
    namespace: Namespace,
    raw: &str,
    after: &str,
    offset: usize,
    names: &dyn Fn() -> Candidates,
) -> Vec<Item> {
    let quote = raw.chars().next().filter(|c| matches!(c, '"' | '\''));
    let (word, before) = match quote {
        Some(_) => {
            let inner = &raw[1..];
            let at = inner.rfind(char::is_whitespace).map_or(0, |i| i + 1);
            (&inner[at..], &inner[..at])
        }
        None if raw.is_empty() => ("", ""),
        None => return vec![],
    };
    if !before.split_whitespace().all(tasks::is_env_assignment) {
        return vec![];
    }
    let end = match quote {
        Some(q) => after
            .find(|c: char| c == q || c.is_whitespace())
            .unwrap_or(after.len()),
        None => 0,
    };
    // An environment assignment being edited is not a name, whether the cursor is before or
    // after its `=`.
    if tasks::is_env_assignment(&format!("{word}{}", &after[..end])) {
        return vec![];
    }
    let replace = offset - word.len()..offset + end;
    let candidates = names();
    let (list, noun) = match namespace {
        Namespace::Task => (candidates.tasks, "task"),
        Namespace::Template => (candidates.templates, "task template"),
    };
    // A task does not wait for itself.
    let own = (path[0] == "tasks" && namespace == Namespace::Task).then(|| path[1].as_str());
    list.into_iter()
        .filter(|c| c.name.starts_with(word) && Some(c.name.as_str()) != own)
        .map(|c| {
            let insert = match quote {
                // inside a string, the name goes between what is already there
                Some(q) if after.contains(q) => c.name.clone(),
                Some(q) => format!("{}{q}", c.name),
                None => format!("\"{}\"", c.name),
            };
            Item {
                insert,
                label: c.name,
                kind: Kind::Task,
                detail: c.detail.or_else(|| Some(noun.to_string())),
                documentation: c.documentation.filter(|d| !d.is_empty()),
                replace: replace.clone(),
            }
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
    /// In an array, the index of the element being written.
    index: usize,
}

impl Frame {
    fn new(kind: char, path: Vec<String>) -> Self {
        Self {
            kind,
            path,
            segment: String::new(),
            pending: None,
            index: 0,
        }
    }

    fn separate(&mut self) {
        self.segment.clear();
        self.pending = None;
        // the next element of an array
        if self.kind == '[' {
            self.index += 1;
        }
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
    // The latest element of each array of tables seen so far, by the key path of the array.
    let mut arrays: HashMap<Vec<String>, usize> = HashMap::new();
    for l in 0..line {
        let text = index.line(l);
        if state.closed()
            && let Some(rest) = text.trim().strip_prefix('[')
        {
            let array = rest.starts_with('[');
            let rest = rest.trim_start_matches('[');
            let end = unquoted(rest, ']').unwrap_or(rest.len());
            table = header_path(keypath::split(&rest[..end]), array, &mut arrays);
            state = State::new(table.clone());
            continue;
        }
        advance(text, &mut state, true);
    }
    let line_starts_closed = state.closed();
    advance(before, &mut state, false);
    (table, state, line_starts_closed)
}

/// The schema path of a header's table. Each array of tables on the way is entered at its
/// latest element, which is how the schema walk and the parsed document name an array's items:
/// `[[a]]`, `[[a]]`, `[a.b]` is `a.1.b`.
fn header_path(
    keys: Vec<String>,
    array: bool,
    arrays: &mut HashMap<Vec<String>, usize>,
) -> Vec<String> {
    let mut path = vec![];
    for (i, key) in keys.iter().enumerate() {
        path.push(key.clone());
        let prefix = &keys[..=i];
        if i + 1 == keys.len() && array {
            // a new element ends the nested arrays of the one before it
            arrays.retain(|k, _| !k.starts_with(prefix) || k.len() == prefix.len());
            let n = arrays
                .entry(prefix.to_vec())
                .and_modify(|n| *n += 1)
                .or_insert(0);
            path.push(n.to_string());
        } else if let Some(n) = arrays.get(prefix) {
            path.push(n.to_string());
        }
    }
    path
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
                        path.push(frame.index.to_string());
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
                    (None, '[') => [parent.path.as_slice(), &[parent.index.to_string()]].concat(),
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
pub(crate) fn unquoted(text: &str, target: char) -> Option<usize> {
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
        complete(
            &text,
            offset,
            &sample(),
            &Registry::default(),
            &Candidates::default,
        )
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
        let item = complete(
            &text,
            offset,
            &sample(),
            &Registry::default(),
            &Candidates::default,
        )
        .remove(0);
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
    fn later_array_elements_have_their_own_keys() {
        assert_eq!(labels("[[list]]\nname = \"a\"\n[[list]]\n|"), ["name"]);
        assert!(labels("[[list]]\nname = \"a\"\n|").is_empty());
    }

    #[test]
    fn headers_below_an_array_table_enter_its_latest_element() {
        let mut arrays = HashMap::new();
        let keys = |s: &[&str]| s.iter().map(|k| k.to_string()).collect::<Vec<_>>();
        assert_eq!(header_path(keys(&["a"]), true, &mut arrays), ["a", "0"]);
        assert_eq!(header_path(keys(&["a"]), true, &mut arrays), ["a", "1"]);
        assert_eq!(
            header_path(keys(&["a", "b"]), false, &mut arrays),
            ["a", "1", "b"]
        );
        assert_eq!(header_path(keys(&["c"]), false, &mut arrays), ["c"]);
    }

    #[test]
    fn tables_do_not_occupy_their_key() {
        assert_eq!(
            labels("settings.color = true\n|"),
            ["list", "min_version", "settings", "tools"]
        );
        assert_eq!(labels("[settings]\ncolor = true\n[list]\n|").len(), 0);
        assert_eq!(
            labels("[[list]]\nname = \"a\"\n|\n[tools]\n"),
            Vec::<String>::new()
        );
    }

    fn tool_labels(marked: &str) -> Vec<String> {
        let offset = marked.find('|').unwrap();
        let text = marked.replacen('|', "", 1);
        complete(
            &text,
            offset,
            &sample(),
            &registry::tests::sample(),
            &Candidates::default,
        )
        .into_iter()
        .map(|i| i.label)
        .collect()
    }

    #[test]
    fn a_tool_header_being_typed_does_not_offer_the_tools_already_there() {
        assert!(tool_labels("[tools.node]\nversion = \"24\"\n[tools.no|]\n").is_empty());
    }

    #[test]
    fn an_unclosed_tool_header_does_not_offer_the_tools_already_there() {
        assert!(tool_labels("[tools.node]\nversion = \"24\"\n[tools.no|").is_empty());
    }

    #[test]
    fn a_quoted_key_is_replaced_through_its_closing_quote_from_inside() {
        let registry = Registry::new(vec![crate::RegistryEntry {
            name: "llama.cpp".to_string(),
            description: None,
            aliases: vec![],
            backends: vec![],
            deprecated: None,
        }]);
        let text = "[tools]\n\"llam.a\" = \"latest\"\n";
        let offset = text.find("llam").unwrap() + 4;
        let item = complete(text, offset, &sample(), &registry, &Candidates::default).remove(0);
        let applied = format!(
            "{}{}{}",
            &text[..item.replace.start],
            item.insert,
            &text[item.replace.end..]
        );
        assert_eq!(applied, "[tools]\n\"llama.cpp\" = \"latest\"\n");
    }

    #[test]
    fn a_quoted_key_with_a_dot_is_replaced_whole() {
        let registry = Registry::new(vec![crate::RegistryEntry {
            name: "llama.cpp".to_string(),
            description: None,
            aliases: vec![],
            backends: vec![],
            deprecated: None,
        }]);
        let text = "[tools]\n\"llama.c\" = \"latest\"\n";
        let offset = text.find("llama.c").unwrap() + "llama.c".len();
        let item = complete(text, offset, &sample(), &registry, &Candidates::default).remove(0);
        let applied = format!(
            "{}{}{}",
            &text[..item.replace.start],
            item.insert,
            &text[item.replace.end..]
        );
        assert_eq!(applied, "[tools]\n\"llama.cpp\" = \"latest\"\n");
    }

    #[test]
    fn an_escaped_quote_does_not_end_a_quoted_key() {
        let registry = Registry::new(vec![crate::RegistryEntry {
            name: "node".to_string(),
            description: None,
            aliases: vec![],
            backends: vec![],
            deprecated: None,
        }]);
        let text = "[tools]\n\"node\\\"x\" = \"24\"\n";
        let offset = text.find("node").unwrap() + 2;
        let item = complete(text, offset, &sample(), &registry, &Candidates::default).remove(0);
        let applied = format!(
            "{}{}{}",
            &text[..item.replace.start],
            item.insert,
            &text[item.replace.end..]
        );
        assert_eq!(applied, "[tools]\nnode = \"24\"\n");
    }

    #[test]
    fn a_key_whose_quote_is_closed_leaves_the_value_alone() {
        let text = "[tools]\n\"no\" = \"24\"\n";
        let offset = text.find("\"no\"").unwrap() + 4;
        let item = complete(
            text,
            offset,
            &sample(),
            &registry::tests::sample(),
            &Candidates::default,
        )
        .remove(0);
        let applied = format!(
            "{}{}{}",
            &text[..item.replace.start],
            item.insert,
            &text[item.replace.end..]
        );
        assert_eq!(applied, "[tools]\nnode = \"24\"\n");
    }

    #[test]
    fn dotted_tools_already_set_are_not_offered_again() {
        assert!(tool_labels("tools.node = \"24\"\ntools.no|").is_empty());
        assert!(tool_labels("tools.node = \"24\"\ntools.no| = \"x\"").is_empty());
    }

    #[test]
    fn a_quoted_key_is_replaced_whole() {
        let text = "[tools]\n\"no\" = \"24\"\n";
        let offset = text.find("no").unwrap() + 2;
        let item = complete(
            text,
            offset,
            &sample(),
            &registry::tests::sample(),
            &Candidates::default,
        )
        .remove(0);
        let applied = format!(
            "{}{}{}",
            &text[..item.replace.start],
            item.insert,
            &text[item.replace.end..]
        );
        assert_eq!(applied, "[tools]\nnode = \"24\"\n");
    }

    #[test]
    fn a_tool_written_as_a_table_is_not_offered_again() {
        assert!(tool_labels("[tools.node]\nversion = \"24\"\n[tools]\n|").is_empty());
    }

    #[test]
    fn a_tool_listed_by_an_alias_is_not_offered_again() {
        assert!(tool_labels("[tools]\nnodejs = \"24\"\n|").is_empty());
    }

    #[test]
    fn tools_complete_from_the_registry() {
        assert_eq!(tool_labels("[tools]\n|"), ["node"]);
        assert_eq!(tool_labels("[tools]\nno|"), ["node"]);
        assert!(tool_labels("[tools]\nzz|").is_empty());
        assert_eq!(tool_labels("tools.|"), ["node"]);
        assert_eq!(tool_labels("[tools.no|"), ["node"]);
        assert_eq!(tool_labels("[tasks.build.tools]\n|"), ["node"]);
        assert_eq!(tool_labels("tools = { | }"), ["node"]);
        // deprecated tools and tools already listed (under any of their names) are not offered
        assert!(tool_labels("[tools]\nnode = \"24\"\n|").is_empty());
        // only tool tables take tool names
        assert_eq!(tool_labels("[settings]\n|"), ["color", "mode"]);
    }

    fn candidate(name: &str, detail: &str) -> tasks::Candidate {
        tasks::Candidate {
            name: name.to_string(),
            detail: Some(detail.to_string()),
            documentation: None,
        }
    }

    fn task_labels(marked: &str) -> Vec<String> {
        let offset = marked.find('|').unwrap();
        let text = marked.replacen('|', "", 1);
        let names = || Candidates {
            tasks: vec![
                candidate("build", "compile"),
                candidate("test", "run tests"),
            ],
            templates: vec![candidate("rust", "rust task")],
        };
        complete(&text, offset, &sample(), &Registry::default(), &names)
            .into_iter()
            .map(|i| i.label)
            .collect()
    }

    #[test]
    fn dependencies_complete_task_names() {
        let tasks = |s: &str| task_labels(&format!("[tasks.lint]\n{s}"));
        assert_eq!(tasks("depends = \"|"), ["build", "test"]);
        assert_eq!(tasks("depends = [\"build\", \"t|"), ["test"]);
        assert_eq!(tasks("wait_for = [|"), ["build", "test"]);
        assert_eq!(tasks("depends_post = { task = \"|\" }"), ["build", "test"]);
        assert_eq!(tasks("run = [{ task = \"b|\" }]"), ["build"]);
        assert_eq!(
            tasks("run = [\"echo\", { tasks = [\"|\"] }]"),
            ["build", "test"]
        );
        assert_eq!(tasks("extends = \"|"), ["rust"]);
        // not a place that names a task
        assert!(tasks("run = \"|").is_empty());
        assert!(tasks("description = \"|").is_empty());
    }

    #[test]
    fn only_the_first_element_of_a_dependency_array_is_a_name() {
        let tasks = |s: &str| task_labels(&format!("[tasks.lint]\n{s}"));
        assert!(tasks("depends = [[\"build\", \"te|\"]]").is_empty());
        assert_eq!(tasks("depends = [[\"te|\", \"x\"]]"), ["test"]);
        assert_eq!(tasks("depends = [[\"build\"], [\"te|\"]]"), ["test"]);
        assert_eq!(tasks("run = [\"echo\", { task = \"te|\" }]"), ["test"]);
    }

    #[test]
    fn a_task_is_not_offered_to_itself() {
        let labels = task_labels("[tasks.build]\ndepends = [\"|");
        assert_eq!(labels, ["test"]);
    }

    #[test]
    fn a_task_name_replaces_only_the_name() {
        let apply = |marked: &str| {
            let offset = marked.find('|').unwrap();
            let text = marked.replacen('|', "", 1);
            let names = || Candidates {
                tasks: vec![candidate("build", ""), candidate("burn", "")],
                templates: vec![],
            };
            let item = complete(&text, offset, &sample(), &Registry::default(), &names).remove(0);
            format!(
                "{}{}{}",
                &text[..item.replace.start],
                item.insert,
                &text[item.replace.end..]
            )
        };
        let head = "[tasks.lint]\ndepends = [";
        assert_eq!(
            apply(&format!("{head}\"bu|ild --release\"]")),
            format!("{head}\"build --release\"]")
        );
        assert_eq!(apply(&format!("{head}'b|u' ]")), format!("{head}'build' ]"));
        assert_eq!(
            apply(&format!("{head}\"PROFILE=release b|\"]")),
            format!("{head}\"PROFILE=release build\"]")
        );
        assert_eq!(apply(&format!("{head}\"b|")), format!("{head}\"build\""));
        assert_eq!(apply(&format!("{head}|")), format!("{head}\"build\""));
    }

    #[test]
    fn arguments_are_not_completed_as_task_names() {
        let tasks = |s: &str| task_labels(&format!("[tasks.lint]\n{s}"));
        assert!(tasks("depends = [\"build --b|\"]").is_empty());
        assert!(tasks("depends = [\"X=1 build b|\"]").is_empty());
        // an assignment is not replaced by a name, with the cursor on either side of its `=`
        assert!(tasks("depends = [\"X|=1 build\"]").is_empty());
        assert!(tasks("depends = [\"X=1|\"]").is_empty());
        assert!(tasks("depends = [\"A=b C|=d build\"]").is_empty());
    }

    #[test]
    fn a_closing_quote_is_not_doubled() {
        let text = "[settings]\nmode = \"\"";
        let offset = text.len() - 1;
        let items = complete(
            text,
            offset,
            &sample(),
            &Registry::default(),
            &Candidates::default,
        );
        assert_eq!(items[0].insert, "\"a");
    }
}
