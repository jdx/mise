use super::*;

impl TomlDocument {
    /// Add a new section
    pub(crate) fn add_section(&mut self, name: String) {
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
    pub(crate) fn add_entry(&mut self, section_idx: usize, key: String, value: String) {
        self.add_entry_with_value(section_idx, key, EntryValue::Simple(value));
    }

    /// Add an entry to a section with a specific value type
    pub(crate) fn add_entry_with_value(
        &mut self,
        section_idx: usize,
        key: String,
        value: EntryValue,
    ) {
        if let Some(section) = self.sections.get_mut(section_idx) {
            section.entries.push(Entry {
                key,
                value,
                expanded: false,
                comments: Vec::new(),
                trailing_comment: None,
            });
            self.modified = true;
        }
    }

    /// Set `field` on the `_` entry of an env section, as in `_.file = ".env"`.
    ///
    /// An `_` entry already in the section (loaded from `_.path` or
    /// `_.source`) is an inline table and `to_toml` cannot nest a dotted key
    /// under it, so the field goes into that table. Without one, a dotted
    /// `_.<field>` entry is added. The result says what changed, for undo.
    pub(crate) fn add_env_underscore_field(
        &mut self,
        section_idx: usize,
        field: &str,
        value: String,
    ) -> EnvFieldChange {
        let existing = self.sections.get_mut(section_idx).and_then(|section| {
            section
                .entries
                .iter_mut()
                .enumerate()
                .find_map(|(idx, entry)| match &mut entry.value {
                    EntryValue::InlineTable(pairs) if entry.key == "_" => Some((idx, pairs)),
                    _ => None,
                })
        });
        let Some((entry_idx, pairs)) = existing else {
            self.add_entry(section_idx, format!("_.{field}"), value);
            return EnvFieldChange::NewEntry(self.sections[section_idx].entries.len() - 1);
        };
        let change = match pairs.iter().position(|(k, _)| k == field) {
            Some(field_idx) => {
                let old = std::mem::replace(&mut pairs[field_idx].1, value);
                EnvFieldChange::ReplacedField(entry_idx, field_idx, old)
            }
            None => {
                pairs.push((field.to_string(), value));
                EnvFieldChange::NewField(entry_idx, pairs.len() - 1)
            }
        };
        self.modified = true;
        change
    }

    /// Delete an entry from a section
    pub(crate) fn delete_entry(&mut self, section_idx: usize, entry_idx: usize) {
        if let Some(section) = self.sections.get_mut(section_idx)
            && entry_idx < section.entries.len()
        {
            section.entries.remove(entry_idx);
            self.modified = true;
        }
    }

    /// Update an entry's value
    pub(crate) fn update_entry(&mut self, section_idx: usize, entry_idx: usize, value: String) {
        if let Some(section) = self.sections.get_mut(section_idx)
            && let Some(entry) = section.entries.get_mut(entry_idx)
        {
            entry.value = EntryValue::Simple(value);
            self.modified = true;
        }
    }

    /// Add an item to an array entry
    pub(crate) fn add_array_item(&mut self, section_idx: usize, entry_idx: usize, value: String) {
        if let Some(section) = self.sections.get_mut(section_idx)
            && let Some(entry) = section.entries.get_mut(entry_idx)
            && let EntryValue::Array(ref mut items) = entry.value
        {
            items.push(value);
            self.modified = true;
        }
    }

    /// Update an array item
    pub(crate) fn update_array_item(
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
    pub(crate) fn delete_array_item(
        &mut self,
        section_idx: usize,
        entry_idx: usize,
        array_idx: usize,
    ) {
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
    pub(crate) fn toggle_section(&mut self, section_idx: usize) {
        if let Some(section) = self.sections.get_mut(section_idx) {
            section.expanded = !section.expanded;
        }
    }

    /// Toggle entry expanded state (for arrays/inline tables)
    pub(crate) fn toggle_entry(&mut self, section_idx: usize, entry_idx: usize) {
        if let Some(section) = self.sections.get_mut(section_idx)
            && let Some(entry) = section.entries.get_mut(entry_idx)
        {
            entry.expanded = !entry.expanded;
        }
    }

    /// Delete a section
    pub(crate) fn delete_section(&mut self, section_idx: usize) {
        if section_idx < self.sections.len() {
            self.sections.remove(section_idx);
            self.modified = true;
        }
    }

    /// Convert a simple entry value to an inline table with version key
    /// Returns true if conversion was successful
    pub(crate) fn convert_to_inline_table(&mut self, section_idx: usize, entry_idx: usize) -> bool {
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
    pub(crate) fn add_inline_table_field(
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

/// What [`TomlDocument::add_env_underscore_field`] changed.
pub(crate) enum EnvFieldChange {
    /// Added a new `_.<field>` entry (entry_idx)
    NewEntry(usize),
    /// Added a field to the existing `_` inline table (entry_idx, field_idx)
    NewField(usize, usize),
    /// Replaced a field's value in the existing `_` table (entry_idx, field_idx, old_value)
    ReplacedField(usize, usize, String),
}
