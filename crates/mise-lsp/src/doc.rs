//! A parsed `mise.toml`: the value as JSON for schema validation, and where each key and value
//! sits in the text.

use std::ops::Range;

use serde_json::{Map, Number, Value};
use toml_edit::{Document, Item, TableLike, Value as TomlValue};

/// One key or array element of the document. `path` names array elements by their index.
pub(crate) struct Entry {
    pub path: Vec<String>,
    pub key: Option<Range<usize>>,
    pub value: Option<Range<usize>>,
}

pub(crate) struct Parsed {
    pub entries: Vec<Entry>,
    pub json: Value,
}

#[derive(Debug)]
pub(crate) struct ParseError {
    pub message: String,
    pub span: Range<usize>,
}

pub(crate) fn parse(text: &str) -> Result<Parsed, ParseError> {
    let doc = Document::parse(text).map_err(|e| ParseError {
        message: e.message().to_string(),
        span: e.span().unwrap_or(0..0),
    })?;
    let mut entries = vec![];
    let json = table(doc.as_table(), &mut vec![], &mut entries);
    Ok(Parsed { entries, json })
}

fn table(t: &dyn TableLike, path: &mut Vec<String>, out: &mut Vec<Entry>) -> Value {
    let mut map = Map::new();
    for (name, _) in t.iter() {
        let Some((key, item)) = t.get_key_value(name) else {
            continue;
        };
        path.push(name.to_string());
        out.push(Entry {
            path: path.clone(),
            key: key.span(),
            value: item.span(),
        });
        map.insert(name.to_string(), item_json(item, path, out));
        path.pop();
    }
    Value::Object(map)
}

fn item_json(item: &Item, path: &mut Vec<String>, out: &mut Vec<Entry>) -> Value {
    match item {
        Item::None => Value::Null,
        Item::Value(v) => value_json(v, path, out),
        Item::Table(t) => table(t, path, out),
        Item::ArrayOfTables(tables) => Value::Array(
            tables
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    path.push(i.to_string());
                    out.push(Entry {
                        path: path.clone(),
                        key: None,
                        value: t.span(),
                    });
                    let value = table(t, path, out);
                    path.pop();
                    value
                })
                .collect(),
        ),
    }
}

fn value_json(value: &TomlValue, path: &mut Vec<String>, out: &mut Vec<Entry>) -> Value {
    match value {
        TomlValue::String(s) => Value::String(s.value().clone()),
        TomlValue::Integer(i) => Value::from(*i.value()),
        TomlValue::Float(f) => Number::from_f64(*f.value()).map_or(Value::Null, Value::Number),
        TomlValue::Boolean(b) => Value::Bool(*b.value()),
        TomlValue::Datetime(d) => Value::String(d.value().to_string()),
        TomlValue::Array(array) => Value::Array(
            array
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    path.push(i.to_string());
                    out.push(Entry {
                        path: path.clone(),
                        key: None,
                        value: v.span(),
                    });
                    let value = value_json(v, path, out);
                    path.pop();
                    value
                })
                .collect(),
        ),
        TomlValue::InlineTable(t) => table(t, path, out),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_have_spans_and_paths() {
        let text = "[tools]\nnode = \"24\"\n[settings]\ncolor.x = true\n";
        let parsed = parse(text).unwrap();
        let node = parsed
            .entries
            .iter()
            .find(|e| e.path == ["tools", "node"])
            .unwrap();
        assert_eq!(&text[node.key.clone().unwrap()], "node");
        assert_eq!(&text[node.value.clone().unwrap()], "\"24\"");
        assert!(
            parsed
                .entries
                .iter()
                .any(|e| e.path == ["settings", "color", "x"])
        );
        assert_eq!(parsed.json["tools"]["node"], "24");
    }

    #[test]
    fn syntax_errors_have_a_span() {
        let err = parse("a = \n").err().unwrap();
        assert!(!err.message.is_empty());
    }
}
