use super::*;

impl TomlDocument {
    /// Serialize the document to a TOML string
    pub(crate) fn to_toml(&self) -> String {
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
                    Self::apply_entry_decor(doc.as_table_mut(), entry);
                }
                continue;
            }

            let mut table = Table::new();

            for entry in &section.entries {
                let item = Self::entry_value_to_item(&entry.value);

                // Handle dotted keys (like _.path in env section) by creating nested tables
                if entry.key.contains('.') && section.name == "env" {
                    // The leaf sits in a subtable, so the decor helper below cannot
                    // reach it. Comments on a dotted key stay lost for now.
                    Self::insert_dotted_key(&mut table, &entry.key, item);
                } else {
                    table.insert(&entry.key, item);
                    Self::apply_entry_decor(&mut table, entry);
                }
            }

            let prefix = Self::comment_prefix(&section.comments);
            if !prefix.is_empty() {
                table.decor_mut().set_prefix(prefix);
            }
            doc.insert(&section.name, Item::Table(table));
        }

        doc.to_string()
    }

    /// Render comment lines as a decor prefix.
    fn comment_prefix(comments: &[String]) -> String {
        comments
            .iter()
            .map(|c| format!("{c}\n"))
            .collect::<Vec<_>>()
            .concat()
    }

    /// Put an entry's comments back on the item it was written as.
    ///
    /// The leading comment goes on the key and a same-line comment on the value:
    /// `to_toml` builds a fresh document, so nothing carries over unless it is
    /// written here (discussion #10650).
    fn apply_entry_decor(table: &mut Table, entry: &Entry) {
        let prefix = Self::comment_prefix(&entry.comments);
        if !prefix.is_empty()
            && let Some(mut key) = table.key_mut(&entry.key)
        {
            key.leaf_decor_mut().set_prefix(prefix);
        }
        if let Some(trailing) = &entry.trailing_comment
            && let Some(Item::Value(value)) = table.get_mut(&entry.key)
        {
            value.decor_mut().set_suffix(trailing.clone());
        }
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

    /// Save the document to a file, creating its directory if it is not there yet.
    ///
    /// The editor is routinely pointed at a config that does not exist yet, and its directory
    /// may not either — `mise generate config --global` on a fresh install is exactly that
    /// case. Without this the save fails after the whole session's worth of edits, which is
    /// the worst possible moment to find out.
    ///
    /// A bare relative name gives an empty parent, which `create_dir_all` treats as a no-op.
    pub(crate) fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, self.to_toml())
    }
}
