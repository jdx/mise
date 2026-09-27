use super::*;

impl TomlDocument {
    /// Create a new document with default sections
    pub(crate) fn new() -> Self {
        Self::new_with_deps(false)
    }

    /// Create a new document with default sections, optionally including deps
    pub(crate) fn new_with_deps(include_deps: bool) -> Self {
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

        if include_deps {
            sections.push(Section {
                name: "deps".to_string(),
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
    pub(crate) fn parse(content: &str) -> Result<Self, toml_edit::TomlError> {
        let doc: DocumentMut = content.parse()?;
        let mut sections = Vec::new();

        // Known sections in preferred order
        let known_sections = ["tools", "env", "tasks", "deps", "settings"];

        // Collect top-level entries (non-table items like min_version)
        let mut root_entries = Vec::new();
        for (key, item) in doc.iter() {
            let key_prefix = doc
                .as_table()
                .key(key)
                .and_then(|k| k.leaf_decor().prefix());
            if !item.is_table()
                && !item.is_array_of_tables()
                && let Some(entry) = Self::parse_entry(key, item, key_prefix)
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
    pub(crate) fn load(path: &Path) -> std::io::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Self::parse(&content).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    fn parse_section(name: &str, table: &Table) -> Section {
        let mut entries = Vec::new();

        for (key, item) in table.iter() {
            // The leading comment of a key/value pair belongs to the key, not the
            // value — the value's prefix is only the space after `=`. Reading the
            // value meant `comments` was always empty for entries, so nothing was
            // displayed and nothing could be written back (discussion #10650).
            let key_prefix = table.key(key).and_then(|k| k.leaf_decor().prefix());
            if let Some(entry) = Self::parse_entry(key, item, key_prefix) {
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

    fn parse_entry(
        key: &str,
        item: &Item,
        key_prefix: Option<&toml_edit::RawString>,
    ) -> Option<Entry> {
        // A nested table carries its own decor; a key/value pair carries it on
        // the key.
        let comments = match item {
            Item::Table(t) => Self::extract_comments_from_decor(t.decor().prefix()),
            _ => Self::extract_comments_from_decor(key_prefix),
        };
        let trailing_comment = match item {
            Item::Value(v) => Self::extract_trailing_comment(v.decor().suffix()),
            _ => None,
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
            trailing_comment,
        })
    }

    /// Keep a decor suffix that carries a same-line comment, spacing and all.
    fn extract_trailing_comment(suffix: Option<&toml_edit::RawString>) -> Option<String> {
        let raw = suffix?.as_str()?;
        raw.trim_start().starts_with('#').then(|| raw.to_string())
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
}
