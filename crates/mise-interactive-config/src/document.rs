//! TomlDocument: In-memory TOML representation with sections and entries

use std::path::Path;
use toml_edit::{DocumentMut, Formatted, Item, Table, Value};

/// Represents a TOML document with navigable sections
#[derive(Debug)]
pub struct TomlDocument {
    pub sections: Vec<Section>,
    pub modified: bool,
}

/// A section in the TOML document (e.g., [tools], [env])
#[derive(Debug, Clone)]
pub struct Section {
    pub name: String,
    pub entries: Vec<Entry>,
    pub expanded: bool,
    /// Comments appearing before this section header
    pub comments: Vec<String>,
}

/// An entry within a section (key = value)
#[derive(Debug, Clone)]
pub struct Entry {
    pub key: String,
    pub value: EntryValue,
    pub expanded: bool,
    /// Comments appearing before this entry
    pub comments: Vec<String>,
}

/// The value of an entry
#[derive(Debug, Clone)]
pub enum EntryValue {
    /// Simple string, number, or boolean value
    Simple(String),
    /// Array of values
    Array(Vec<String>),
    /// Inline table of key-value pairs
    InlineTable(Vec<(String, String)>),
}

impl TomlDocument {
    /// Create a new document with default sections
    pub fn new() -> Self {
        Self::new_with_prepare(false)
    }

    /// Create a new document with default sections, optionally including prepare
    pub fn new_with_prepare(include_prepare: bool) -> Self {
        let mut sections = vec![
            Section {
                name: "tools".to_string(),
                entries: Vec::new(),
                expanded: true,
                comments: Vec::new(),
            },
            Section {
                name: "env".to_string(),
                entries: Vec::new(),
                expanded: false,
                comments: Vec::new(),
            },
            Section {
                name: "tasks".to_string(),
                entries: Vec::new(),
                expanded: false,
                comments: Vec::new(),
            },
        ];

        if include_prepare {
            sections.push(Section {
                name: "prepare".to_string(),
                entries: Vec::new(),
                expanded: false,
                comments: Vec::new(),
            });
        }

        sections.push(Section {
            name: "settings".to_string(),
            entries: Vec::new(),
            expanded: false,
            comments: Vec::new(),
        });

        Self {
            sections,
            modified: false,
        }
    }

    /// Parse a TOML document from a string
    pub fn parse(content: &str) -> Result<Self, toml_edit::TomlError> {
        let doc: DocumentMut = content.parse()?;
        let mut sections = Vec::new();

        // Known sections in preferred order
        let known_sections = ["tools", "env", "tasks", "prepare", "settings"];

        // Collect top-level entries (non-table items like min_version)
        let mut root_entries = Vec::new();
        for (key, item) in doc.iter() {
            if !item.is_table()
                && !item.is_array_of_tables()
                && let Some(entry) = Self::parse_entry(key, item)
            {
                root_entries.push(entry);
            }
        }

        // Add root section (empty name) if we have top-level entries
        if !root_entries.is_empty() {
            sections.push(Section {
                name: String::new(),
                entries: root_entries,
                expanded: true,
                comments: Vec::new(),
            });
        }

        // Add known sections first (in order)
        for name in &known_sections {
            if let Some(item) = doc.get(name)
                && let Some(table) = item.as_table()
            {
                sections.push(Self::parse_section(name, table));
            }
        }

        // Add any other sections
        for (key, item) in doc.iter() {
            if !known_sections.contains(&key)
                && let Some(table) = item.as_table()
            {
                sections.push(Self::parse_section(key, table));
            }
        }

        // Add missing default sections
        for name in &known_sections {
            if !sections.iter().any(|s| s.name == *name) {
                sections.push(Section {
                    name: name.to_string(),
                    entries: Vec::new(),
                    expanded: false,
                    comments: Vec::new(),
                });
            }
        }

        // Sort to maintain preferred order (empty name for root entries comes first)
        sections.sort_by(|a, b| {
            let order = |n: &str| {
                if n.is_empty() {
                    return 0; // Root entries come first
                }
                known_sections
                    .iter()
                    .position(|&s| s == n)
                    .map(|p| p + 1)
                    .unwrap_or(known_sections.len() + 1)
            };
            order(&a.name).cmp(&order(&b.name))
        });

        // Expand first non-empty section, or tools if all empty
        let first_non_empty = sections.iter_mut().find(|s| !s.entries.is_empty());
        if let Some(section) = first_non_empty {
            section.expanded = true;
        } else if let Some(tools) = sections.iter_mut().find(|s| s.name == "tools") {
            tools.expanded = true;
        }

        Ok(Self {
            sections,
            modified: false,
        })
    }

    /// Load a TOML document from a file
    pub fn load(path: &Path) -> std::io::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Self::parse(&content).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    fn parse_section(name: &str, table: &Table) -> Section {
        let mut entries = Vec::new();

        for (key, item) in table.iter() {
            if let Some(entry) = Self::parse_entry(key, item) {
                entries.push(entry);
            }
        }

        // Extract comments from the table's decor
        let comments = Self::extract_comments_from_decor(table.decor().prefix());

        Section {
            name: name.to_string(),
            entries,
            expanded: false,
            comments,
        }
    }

    fn parse_entry(key: &str, item: &Item) -> Option<Entry> {
        // Extract comments from the item's decor (handle both Values and Tables)
        let comments = match item {
            Item::Value(v) => Self::extract_comments_from_decor(v.decor().prefix()),
            Item::Table(t) => Self::extract_comments_from_decor(t.decor().prefix()),
            _ => Vec::new(),
        };

        let value = match item {
            Item::Value(v) => Self::parse_value(v),
            Item::Table(t) => {
                // Nested table - convert to inline table representation
                let pairs: Vec<(String, String)> = t
                    .iter()
                    .filter_map(|(k, v)| {
                        if let Item::Value(val) = v {
                            Some((k.to_string(), Self::value_to_string(val)))
                        } else {
                            None
                        }
                    })
                    .collect();
                EntryValue::InlineTable(pairs)
            }
            _ => return None,
        };

        Some(Entry {
            key: key.to_string(),
            value,
            expanded: false,
            comments,
        })
    }

    /// Extract comment lines from a decor prefix
    fn extract_comments_from_decor(prefix: Option<&toml_edit::RawString>) -> Vec<String> {
        let Some(prefix) = prefix else {
            return Vec::new();
        };
        let prefix_str = prefix.as_str().unwrap_or("");
        prefix_str
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if trimmed.starts_with('#') {
                    Some(trimmed.to_string())
                } else {
                    None
                }
            })
            .collect()
    }

    fn parse_value(value: &Value) -> EntryValue {
        match value {
            Value::Array(arr) => {
                let items: Vec<String> = arr.iter().map(Self::value_to_string).collect();
                EntryValue::Array(items)
            }
            Value::InlineTable(t) => {
                let pairs: Vec<(String, String)> = t
                    .iter()
                    .map(|(k, v)| (k.to_string(), Self::value_to_string(v)))
                    .collect();
                EntryValue::InlineTable(pairs)
            }
            _ => EntryValue::Simple(Self::value_to_string(value)),
        }
    }

    fn value_to_string(value: &Value) -> String {
        match value {
            Value::String(s) => s.value().to_string(),
            Value::Integer(i) => i.value().to_string(),
            Value::Float(f) => f.value().to_string(),
            Value::Boolean(b) => b.value().to_string(),
            Value::Array(arr) => {
                let items: Vec<String> = arr.iter().map(Self::value_to_string).collect();
                format!("[{}]", items.join(", "))
            }
            Value::InlineTable(t) => {
                let pairs: Vec<String> = t
                    .iter()
                    .map(|(k, v)| format!("{} = {}", k, Self::value_to_string(v)))
                    .collect();
                format!("{{ {} }}", pairs.join(", "))
            }
            Value::Datetime(dt) => dt.value().to_string(),
        }
    }

    /// Serialize the document to a TOML string
    pub fn to_toml(&self) -> String {
        let mut doc = DocumentMut::new();

        for section in &self.sections {
            if section.entries.is_empty() {
                continue;
            }

            // Handle root-level entries (section with empty name)
            if section.name.is_empty() {
                for entry in &section.entries {
                    let item = Self::entry_value_to_item(&entry.value);
                    doc.insert(&entry.key, item);
                }
                continue;
            }

            let mut table = Table::new();

            for entry in &section.entries {
                let item = Self::entry_value_to_item(&entry.value);

                // Handle dotted keys (like _.path in env section) by creating nested tables
                if entry.key.contains('.') && section.name == "env" {
                    Self::insert_dotted_key(&mut table, &entry.key, item);
                } else {
                    table.insert(&entry.key, item);
                }
            }

            doc.insert(&section.name, Item::Table(table));
        }

        doc.to_string()
    }

    /// Insert a dotted key into a table by creating nested structure
    /// e.g., "_.path" becomes _: { path: value }
    fn insert_dotted_key(table: &mut Table, key: &str, item: Item) {
        let parts: Vec<&str> = key.splitn(2, '.').collect();
        if parts.len() == 2 {
            let parent_key = parts[0];
            let child_key = parts[1];

            // Get or create the parent subtable
            if !table.contains_key(parent_key) {
                let mut subtable = Table::new();
                subtable.set_implicit(true);
                table.insert(parent_key, Item::Table(subtable));
            }

            if let Some(Item::Table(subtable)) = table.get_mut(parent_key) {
                // Recursively handle if child_key also contains a dot
                if child_key.contains('.') {
                    Self::insert_dotted_key(subtable, child_key, item);
                } else {
                    subtable.insert(child_key, item);
                }
            }
        } else {
            // No dot, insert directly
            table.insert(key, item);
        }
    }

    fn entry_value_to_item(value: &EntryValue) -> Item {
        match value {
            EntryValue::Simple(s) => {
                // Only special-case booleans, keep everything else as strings
                // This is appropriate for mise configs where versions like "22" should stay quoted
                if s == "true" {
                    Item::Value(Value::Boolean(Formatted::new(true)))
                } else if s == "false" {
                    Item::Value(Value::Boolean(Formatted::new(false)))
                } else {
                    Item::Value(Value::String(Formatted::new(s.clone())))
                }
            }
            EntryValue::Array(items) => {
                let mut arr = toml_edit::Array::new();
                for item in items {
                    // Keep array items as strings unless explicitly boolean
                    let val = if item == "true" {
                        Value::Boolean(Formatted::new(true))
                    } else if item == "false" {
                        Value::Boolean(Formatted::new(false))
                    } else {
                        Value::String(Formatted::new(item.clone()))
                    };
                    arr.push(val);
                }
                Item::Value(Value::Array(arr))
            }
            EntryValue::InlineTable(pairs) => {
                let mut table = toml_edit::InlineTable::new();
                for (k, v) in pairs {
                    let val = if v == "true" {
                        Value::Boolean(Formatted::new(true))
                    } else if v == "false" {
                        Value::Boolean(Formatted::new(false))
                    } else {
                        Value::String(Formatted::new(v.clone()))
                    };
                    table.insert(k, val);
                }
                Item::Value(Value::InlineTable(table))
            }
        }
    }

    /// Save the document to a file
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        std::fs::write(path, self.to_toml())
    }

    /// Add a new section
    pub fn add_section(&mut self, name: String) {
        if !self.sections.iter().any(|s| s.name == name) {
            self.sections.push(Section {
                name,
                entries: Vec::new(),
                expanded: true,
                comments: Vec::new(),
            });
            self.modified = true;
        }
    }

    /// Add an entry to a section with a simple string value
    pub fn add_entry(&mut self, section_idx: usize, key: String, value: String) {
        self.add_entry_with_value(section_idx, key, EntryValue::Simple(value));
    }

    /// Add an entry to a section with a specific value type
    pub fn add_entry_with_value(&mut self, section_idx: usize, key: String, value: EntryValue) {
        if let Some(section) = self.sections.get_mut(section_idx) {
            section.entries.push(Entry {
                key,
                value,
                expanded: false,
                comments: Vec::new(),
            });
            self.modified = true;
        }
    }

    /// Delete an entry from a section
    pub fn delete_entry(&mut self, section_idx: usize, entry_idx: usize) {
        if let Some(section) = self.sections.get_mut(section_idx)
            && entry_idx < section.entries.len()
        {
            section.entries.remove(entry_idx);
            self.modified = true;
        }
    }

    /// Update an entry's value
    pub fn update_entry(&mut self, section_idx: usize, entry_idx: usize, value: String) {
        if let Some(section) = self.sections.get_mut(section_idx)
            && let Some(entry) = section.entries.get_mut(entry_idx)
        {
            entry.value = EntryValue::Simple(value);
            self.modified = true;
        }
    }

    /// Add an item to an array entry
    pub fn add_array_item(&mut self, section_idx: usize, entry_idx: usize, value: String) {
        if let Some(section) = self.sections.get_mut(section_idx)
            && let Some(entry) = section.entries.get_mut(entry_idx)
            && let EntryValue::Array(ref mut items) = entry.value
        {
            items.push(value);
            self.modified = true;
        }
    }

    /// Update an array item
    pub fn update_array_item(
        &mut self,
        section_idx: usize,
        entry_idx: usize,
        array_idx: usize,
        value: String,
    ) {
        if let Some(section) = self.sections.get_mut(section_idx)
            && let Some(entry) = section.entries.get_mut(entry_idx)
            && let EntryValue::Array(ref mut items) = entry.value
            && let Some(item) = items.get_mut(array_idx)
        {
            *item = value;
            self.modified = true;
        }
    }

    /// Delete an array item
    pub fn delete_array_item(&mut self, section_idx: usize, entry_idx: usize, array_idx: usize) {
        if let Some(section) = self.sections.get_mut(section_idx)
            && let Some(entry) = section.entries.get_mut(entry_idx)
            && let EntryValue::Array(ref mut items) = entry.value
            && array_idx < items.len()
        {
            items.remove(array_idx);
            self.modified = true;
        }
    }

    /// Toggle section expanded state
    pub fn toggle_section(&mut self, section_idx: usize) {
        if let Some(section) = self.sections.get_mut(section_idx) {
            section.expanded = !section.expanded;
        }
    }

    /// Toggle entry expanded state (for arrays/inline tables)
    pub fn toggle_entry(&mut self, section_idx: usize, entry_idx: usize) {
        if let Some(section) = self.sections.get_mut(section_idx)
            && let Some(entry) = section.entries.get_mut(entry_idx)
        {
            entry.expanded = !entry.expanded;
        }
    }

    /// Delete a section
    pub fn delete_section(&mut self, section_idx: usize) {
        if section_idx < self.sections.len() {
            self.sections.remove(section_idx);
            self.modified = true;
        }
    }

    /// Convert a simple entry value to an inline table with version key
    /// Returns true if conversion was successful
    pub fn convert_to_inline_table(&mut self, section_idx: usize, entry_idx: usize) -> bool {
        if let Some(section) = self.sections.get_mut(section_idx)
            && let Some(entry) = section.entries.get_mut(entry_idx)
            && let EntryValue::Simple(value) = &entry.value
        {
            // Convert "value" to { version = "value" }
            entry.value = EntryValue::InlineTable(vec![("version".to_string(), value.clone())]);
            entry.expanded = true;
            self.modified = true;
            return true;
        }
        false
    }

    /// Add a field to an inline table entry
    #[allow(dead_code)]
    pub fn add_inline_table_field(
        &mut self,
        section_idx: usize,
        entry_idx: usize,
        key: String,
        value: String,
    ) {
        if let Some(section) = self.sections.get_mut(section_idx)
            && let Some(entry) = section.entries.get_mut(entry_idx)
            && let EntryValue::InlineTable(ref mut pairs) = entry.value
        {
            pairs.push((key, value));
            self.modified = true;
        }
    }
}

impl Default for TomlDocument {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)]
impl EntryValue {
    /// Check if this is a complex value (array or inline table)
    pub fn is_complex(&self) -> bool {
        !matches!(self, EntryValue::Simple(_))
    }

    /// Get the display string for this value
    pub fn display(&self) -> String {
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
    pub fn item_count(&self) -> Option<usize> {
        match self {
            EntryValue::Simple(_) => None,
            EntryValue::Array(items) => Some(items.len()),
            EntryValue::InlineTable(pairs) => Some(pairs.len()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_document() {
        let doc = TomlDocument::new();
        assert_eq!(doc.sections.len(), 4);
        assert_eq!(doc.sections[0].name, "tools");
        assert!(doc.sections[0].expanded);
    }

    #[test]
    fn test_parse_simple() {
        let content = r#"
[tools]
node = "22"
python = "3.12"

[env]
NODE_ENV = "development"
"#;
        let doc = TomlDocument::parse(content).unwrap();
        assert_eq!(doc.sections[0].name, "tools");
        assert_eq!(doc.sections[0].entries.len(), 2);
        assert_eq!(doc.sections[0].entries[0].key, "node");
    }

    #[test]
    fn test_parse_array() {
        let content = r#"
[env]
paths = ["./bin", "./node_modules/.bin"]
"#;
        let doc = TomlDocument::parse(content).unwrap();
        let env_section = doc.sections.iter().find(|s| s.name == "env").unwrap();
        let entry = &env_section.entries[0];
        assert_eq!(entry.key, "paths");
        assert!(matches!(entry.value, EntryValue::Array(_)));
        if let EntryValue::Array(items) = &entry.value {
            assert_eq!(items.len(), 2);
            assert_eq!(items[0], "./bin");
        }
    }

    #[test]
    fn test_to_toml() {
        let mut doc = TomlDocument::new();
        doc.add_entry(0, "node".to_string(), "22".to_string());
        let toml = doc.to_toml();
        assert!(toml.contains("[tools]"));
        assert!(toml.contains("node = \"22\""));
    }

    #[test]
    fn test_roundtrip() {
        let content = r#"[tools]
node = "22"
python = "3.12"

[env]
NODE_ENV = "development"
"#;
        let doc = TomlDocument::parse(content).unwrap();
        let output = doc.to_toml();
        assert!(output.contains("node = \"22\""));
        assert!(output.contains("python = \"3.12\""));
        assert!(output.contains("NODE_ENV = \"development\""));
    }

    #[test]
    fn test_parse_top_level_entries() {
        let content = r#"min_version = "2024.1.0"

[tools]
node = "22"
"#;
        let doc = TomlDocument::parse(content).unwrap();
        // Root section (empty name) should be first
        let root_section = doc.sections.iter().find(|s| s.name.is_empty()).unwrap();
        assert_eq!(root_section.entries.len(), 1);
        assert_eq!(root_section.entries[0].key, "min_version");
    }

    #[test]
    fn test_top_level_entries_roundtrip() {
        let content = r#"min_version = "2024.1.0"

[tools]
node = "22"
"#;
        let doc = TomlDocument::parse(content).unwrap();
        let output = doc.to_toml();
        assert!(output.contains("min_version = \"2024.1.0\""));
        assert!(output.contains("[tools]"));
        assert!(output.contains("node = \"22\""));
    }

    #[test]
    fn test_env_dotted_key_serialization() {
        // Create a document with _.path in the env section
        let mut doc = TomlDocument::new();
        let env_idx = doc.sections.iter().position(|s| s.name == "env").unwrap();

        // Add _.path as an array
        doc.sections[env_idx].entries.push(Entry {
            key: "_.path".to_string(),
            value: EntryValue::Array(vec!["./bin".to_string(), "./node_modules/.bin".to_string()]),
            expanded: false,
            comments: Vec::new(),
        });

        let output = doc.to_toml();
        // Should output as dotted key, not quoted key
        // _.path = [...] means _: { path: [...] }
        assert!(
            output.contains("_.path") || output.contains("[env._]"),
            "Output should contain dotted key notation: {}",
            output
        );
        // Should NOT contain quoted key
        assert!(
            !output.contains("\"_.path\""),
            "Output should not contain quoted key: {}",
            output
        );
    }
}
