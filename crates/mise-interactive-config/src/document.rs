//! TomlDocument: In-memory TOML representation with sections and entries

use std::path::Path;
use toml_edit::{DocumentMut, Formatted, Item, Table, Value};

/// Represents a TOML document with navigable sections
#[derive(Debug)]
pub(crate) struct TomlDocument {
    pub sections: Vec<Section>,
    pub modified: bool,
}

/// A section in the TOML document (e.g., [tools], [env])
#[derive(Debug, Clone)]
pub(crate) struct Section {
    pub name: String,
    pub entries: Vec<Entry>,
    pub expanded: bool,
    /// Comments appearing before this section header
    pub comments: Vec<String>,
}

/// An entry within a section (key = value)
#[derive(Debug, Clone)]
pub(crate) struct Entry {
    pub key: String,
    pub value: EntryValue,
    pub expanded: bool,
    /// Comments appearing before this entry
    pub comments: Vec<String>,
    /// The value's decor suffix when it carries a same-line comment, kept
    /// verbatim so spacing survives. It lives in the *suffix*, which is why
    /// reading only the prefix never picked it up.
    pub trailing_comment: Option<String>,
}

/// The value of an entry
#[derive(Debug, Clone)]
pub(crate) enum EntryValue {
    /// Simple string, number, or boolean value
    Simple(String),
    /// Array of values
    Array(Vec<String>),
    /// Inline table of key-value pairs
    InlineTable(Vec<(String, String)>),
}

mod mutate;
mod parse;
mod serialize;

impl Default for TomlDocument {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)]
impl EntryValue {
    /// Check if this is a complex value (array or inline table)
    pub(crate) fn is_complex(&self) -> bool {
        !matches!(self, EntryValue::Simple(_))
    }

    /// Get the display string for this value
    pub(crate) fn display(&self) -> String {
        match self {
            EntryValue::Simple(s) => s.clone(),
            EntryValue::Array(items) => format!("[{}]", items.join(", ")),
            EntryValue::InlineTable(pairs) => {
                let parts: Vec<String> = pairs
                    .iter()
                    .map(|(k, v)| format!("{} = {}", k, v))
                    .collect();
                format!("{{ {} }}", parts.join(", "))
            }
        }
    }

    /// Get item count for complex values
    pub(crate) fn item_count(&self) -> Option<usize> {
        match self {
            EntryValue::Simple(_) => None,
            EntryValue::Array(items) => Some(items.len()),
            EntryValue::InlineTable(pairs) => Some(pairs.len()),
        }
    }
}

#[cfg(test)]
mod tests;
