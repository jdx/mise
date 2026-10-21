//! Hover: what the schema says about the key under the cursor.

use std::ops::Range;

use crate::doc;
use crate::registry::{self, Registry};
use crate::schema::Schema;

pub(crate) struct Hover {
    pub markdown: String,
    pub range: Range<usize>,
}

pub(crate) fn hover(text: &str, offset: usize, schema: &Schema, tools: &Registry) -> Option<Hover> {
    let parsed = doc::parse(text).ok()?;
    let (entry, range) = parsed
        .entries
        .iter()
        .filter_map(|e| e.key.clone().map(|range| (e, range)))
        .filter(|(_, range)| range.contains(&offset) || range.end == offset)
        .max_by_key(|(e, _)| e.path.len())?;
    if Registry::is_tool_key(&entry.path)
        && let Some(tool) = entry.path.last().and_then(|name| tools.get(name))
    {
        return Some(Hover {
            markdown: registry::describe(tool),
            range,
        });
    }
    let nodes = schema.at(&entry.path);
    let description = schema.description(&nodes);
    let types = schema.types(&nodes);
    if description.is_none() && types.is_empty() {
        return None;
    }
    let mut markdown = format!("**{}**", entry.path.join("."));
    if !types.is_empty() {
        markdown.push_str(&format!(" _{}_", types.join(" | ")));
    }
    if schema.is_deprecated(&nodes) {
        markdown.push_str("\n\n**Deprecated.**");
    }
    if let Some(description) = description {
        markdown.push_str("\n\n");
        markdown.push_str(&description);
    }
    Some(Hover { markdown, range })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::tests::sample;

    #[test]
    fn describes_the_key_under_the_cursor() {
        let text = "[settings]\ncolor = true\n";
        let offset = text.find("color").unwrap() + 2;
        let hover = hover(text, offset, &sample(), &Registry::default()).unwrap();
        assert!(hover.markdown.starts_with("**settings.color** _boolean_"));
        assert!(hover.markdown.contains("use color"));
        assert_eq!(&text[hover.range], "color");
    }

    #[test]
    fn a_tool_key_shows_the_registry_entry() {
        let tools = registry::tests::sample();
        let text = "[tools]\nnodejs = \"24\"\nother = \"1\"\n";
        let offset = text.find("nodejs").unwrap() + 2;
        let hover = hover(text, offset, &sample(), &tools).unwrap();
        assert!(hover.markdown.starts_with("**node** _tool_"));
        assert_eq!(&text[hover.range], "nodejs");
        // a tool the registry does not know falls back to the schema, which has nothing here
        let offset = text.find("other").unwrap() + 2;
        let other = super::hover(text, offset, &sample(), &tools);
        assert!(other.is_none_or(|h| !h.markdown.contains("_tool_")));
    }

    #[test]
    fn unknown_keys_have_no_hover() {
        let text = "nope = 1\n";
        assert!(hover(text, 1, &sample(), &Registry::default()).is_none());
    }
}
