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
