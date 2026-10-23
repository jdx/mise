//! Semantic tokens: what each key and template of a config is, for editors whose TOML grammar
//! sees only keys and strings.

use std::ops::Range;

use serde_json::Value;

use crate::doc::{self, Parsed};
use crate::schema::Schema;
use crate::tasks::{self, value_at};
use crate::text::LineIndex;

/// The token types the server sends, in the order of the legend.
pub(crate) const TYPES: [&str; 6] = [
    "function", "type", "variable", "keyword", "macro", "property",
];
/// The token modifiers the server sends, in the order of the legend.
pub(crate) const MODIFIERS: [&str; 3] = ["declaration", "readonly", "deprecated"];

const FUNCTION: u32 = 0;
const TYPE: u32 = 1;
const VARIABLE: u32 = 2;
const KEYWORD: u32 = 3;
const MACRO: u32 = 4;
const PROPERTY: u32 = 5;

const DECLARATION: u32 = 1;
const READONLY: u32 = 1 << 1;
const DEPRECATED: u32 = 1 << 2;

#[derive(Debug, PartialEq)]
struct Token {
    range: Range<usize>,
    kind: u32,
    modifiers: u32,
}

/// The `data` of a `textDocument/semanticTokens` reply for `text`, limited to the bytes in
/// `within` when given. `None` when the text does not parse.
pub(crate) fn encode(
    text: &str,
    schema: &Schema,
    within: Option<Range<usize>>,
) -> Option<Vec<u32>> {
    let parsed = doc::parse(text).ok()?;
    let index = LineIndex::new(text);
    let mut data = vec![];
    let (mut line, mut start) = (0, 0);
    for token in tokens(text, &parsed, schema) {
        if within
            .as_ref()
            .is_some_and(|w| token.range.end <= w.start || token.range.start >= w.end)
        {
            continue;
        }
        // A token cannot span lines, so a template in a multi-line string is sent per line.
        let mut from = token.range.start;
        while from < token.range.end {
            let (l, c) = index.position(from);
            let line_end = text[from..token.range.end]
                .find('\n')
                .map_or(token.range.end, |n| from + n);
            let to = text[from..line_end].trim_end_matches('\r').len() + from;
            let length = index.position(to).1.saturating_sub(c);
            if length > 0 {
                let delta_line = l - line;
                let delta_start = if delta_line == 0 { c - start } else { c };
                data.extend([
                    delta_line as u32,
                    delta_start as u32,
                    length as u32,
                    token.kind,
                    token.modifiers,
                ]);
                (line, start) = (l, c);
            }
            from = line_end + 1;
        }
    }
    Some(data)
}

/// The tokens of a document, in text order and without overlaps.
fn tokens(text: &str, parsed: &Parsed, schema: &Schema) -> Vec<Token> {
    let mut out = vec![];
    for entry in &parsed.entries {
        let mut path: Vec<&str> = entry.path.iter().map(String::as_str).collect();
        // `[[env]]` is a list of env tables: its keys read as the keys of `[env]`.
        if let ["env", index, ..] = path.as_slice()
            && index.parse::<usize>().is_ok()
        {
            path.remove(1);
        }
        if let Some(range) = entry.key.clone() {
            let nodes = schema.at(&entry.path);
            let deprecated = if schema.is_deprecated(&nodes) {
                DEPRECATED
            } else {
                0
            };
            let known = match path.as_slice() {
                ["tasks" | "task_templates", _] => Some((FUNCTION, DECLARATION)),
                ["tools", _] | ["tasks" | "task_templates", _, "tools", _] => Some((TYPE, 0)),
                ["env", "_"] | ["tasks" | "task_templates", _, "env", "_"] => Some((KEYWORD, 0)),
                ["env", _] | ["tasks" | "task_templates", _, "env", _] => Some((VARIABLE, 0)),
                ["vars", _] | ["tasks" | "task_templates", _, "vars", _] => {
                    Some((VARIABLE, READONLY))
                }
                _ => None,
            };
            match known {
                Some((kind, modifiers)) => out.push(Token {
                    range,
                    kind,
                    modifiers: modifiers | deprecated,
                }),
                None if deprecated != 0 => out.push(Token {
                    range,
                    kind: PROPERTY,
                    modifiers: deprecated,
                }),
                None => {}
            }
        }
        if let (Some(span), Some(Value::String(_))) =
            (entry.value.clone(), value_at(&parsed.json, &entry.path))
        {
            templates(text, span, &mut out);
        }
    }
    for reference in tasks::references(text, parsed) {
        out.push(Token {
            range: reference.range,
            kind: FUNCTION,
            modifiers: 0,
        });
    }
    out.sort_by_key(|t| (t.range.start, t.range.end));
    let mut end = 0;
    out.retain(|t| {
        let keep = t.range.start >= end;
        if keep {
            end = t.range.end;
        }
        keep
    });
    out
}

/// The `{{ }}`, `{% %}` and `{# #}` in the string at `span`.
fn templates(text: &str, span: Range<usize>, out: &mut Vec<Token>) {
    let Some(raw) = text.get(span.clone()) else {
        return;
    };
    let mut at = 0;
    while let Some(open) = raw[at..].find('{').map(|n| n + at) {
        let close = match raw.as_bytes().get(open + 1) {
            Some(b'{') => "}}",
            Some(b'%') => "%}",
            Some(b'#') => "#}",
            _ => {
                at = open + 1;
                continue;
            }
        };
        let Some(end) = raw[open + 2..].find(close) else {
            return;
        };
        let end = open + 2 + end + 2;
        out.push(Token {
            range: span.start + open..span.start + end,
            kind: MACRO,
            modifiers: 0,
        });
        at = end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `(text, type, modifiers)` of each token.
    fn spans(text: &str) -> Vec<(String, &'static str, Vec<&'static str>)> {
        let parsed = doc::parse(text).unwrap();
        tokens(text, &parsed, &crate::schema::tests::sample())
            .into_iter()
            .map(|t| {
                let modifiers = (0..MODIFIERS.len())
                    .filter(|i| t.modifiers & (1 << i) != 0)
                    .map(|i| MODIFIERS[i])
                    .collect();
                (text[t.range].to_string(), TYPES[t.kind as usize], modifiers)
            })
            .collect()
    }

    #[test]
    fn keys_are_colored_by_what_they_name() {
        let text = "[tools]\nnode = \"24\"\n[env]\nFOO = \"1\"\n_.path = [\"bin\"]\n\
                    [vars]\nv = \"x\"\n[tasks.build]\nrun = \"make\"\n\
                    [tasks.test]\ndepends = [\"build\"]\n[tasks.test.env]\nA = \"1\"\n";
        assert_eq!(
            spans(text),
            [
                ("node".into(), "type", vec![]),
                ("FOO".into(), "variable", vec![]),
                ("_".into(), "keyword", vec![]),
                ("v".into(), "variable", vec!["readonly"]),
                ("build".into(), "function", vec!["declaration"]),
                ("test".into(), "function", vec!["declaration"]),
                ("build".into(), "function", vec![]),
                ("A".into(), "variable", vec![]),
            ]
        );
    }

    #[test]
    fn env_lists_and_task_vars_are_colored_too() {
        let text = "[[env]]\nFOO = \"1\"\n_.path = [\"bin\"]\n[tasks.a.vars]\nv = \"x\"\n";
        assert_eq!(
            spans(text),
            [
                ("FOO".into(), "variable", vec![]),
                ("_".into(), "keyword", vec![]),
                ("a".into(), "function", vec!["declaration"]),
                ("v".into(), "variable", vec!["readonly"]),
            ]
        );
    }

    #[test]
    fn a_deprecated_key_is_marked() {
        assert_eq!(
            spans("old = \"x\"\n"),
            [("old".into(), "property", vec!["deprecated"])]
        );
    }

    #[test]
    fn templates_are_macros() {
        let text = "[env]\nA = \"{{ env.HOME }}/x{% if true %}y{% endif %}\"\n";
        let found = spans(text);
        assert_eq!(found[1], ("{{ env.HOME }}".into(), "macro", vec![]));
        assert_eq!(found.len(), 4);
    }

    #[test]
    fn a_multi_line_template_is_sent_per_line() {
        let text = "[tasks.a]\nrun = \"\"\"\n{{ a\n b }}\n\"\"\"\n";
        let data = encode(text, &crate::schema::tests::sample(), None).unwrap();
        // `a` (declaration), then `{{ a` and ` b }}` on their own lines
        assert_eq!(
            data,
            [
                0, 7, 1, 0, 1, // `a`
                2, 0, 4, 4, 0, // `{{ a`
                1, 0, 5, 4, 0, // ` b }}`
            ]
        );
    }

    #[test]
    fn unparseable_text_has_no_tokens() {
        assert_eq!(
            encode("a = \n", &crate::schema::tests::sample(), None),
            None
        );
    }
}
