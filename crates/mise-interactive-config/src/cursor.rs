//! Cursor: Navigation state and position tracking

use crate::document::{EntryValue, TomlDocument};

/// Cursor position in the document
#[derive(Debug, Clone)]
pub struct Cursor {
    /// Flat index into visible items
    index: usize,
}

/// What the cursor is currently pointing at
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CursorTarget {
    /// Section header (section index)
    SectionHeader(usize),
    /// Entry within a section (section index, entry index)
    Entry(usize, usize),
    /// Array item within an entry (section index, entry index, array index)
    ArrayItem(usize, usize, usize),
    /// Inline table field (section index, entry index, field index)
    InlineTableField(usize, usize, usize),
    /// Add button
    AddButton(AddButtonKind),
    /// Comment line (not navigable, just for rendering)
    Comment(String),
}

/// Types of add buttons
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddButtonKind {
    /// Add a new section
    Section,
    /// Add an entry to a section (section index) - generic
    Entry(usize),
    /// Add a tool from registry via picker (section index) - for [tools] section
    ToolRegistry(usize),
    /// Add a tool from a backend (section index) - for [tools] section
    ToolBackend(usize),
    /// Add PATH entry (section index) - for [env] section
    EnvPath(usize),
    /// Load .env file (section index) - for [env] section
    EnvDotenv(usize),
    /// Source a shell script (section index) - for [env] section
    EnvSource(usize),
    /// Add environment variable (section index) - for [env] section
    EnvVariable(usize),
    /// Add a task (section index) - for [tasks] section
    Task(usize),
    /// Add a prepare provider (section index) - for [prepare] section
    Prepare(usize),
    /// Add a setting via picker (section index) - for [settings] section
    Setting(usize),
    /// Add a hook via picker (section index) - for [hooks] section
    Hook(usize),
    /// Add a task_config key via picker (section index) - for [task_config] section
    TaskConfig(usize),
    /// Add a monorepo key via picker (section index) - for [monorepo] section
    Monorepo(usize),
    /// Add an item to an array (section index, entry index)
    ArrayItem(usize, usize),
    /// Add a field to an inline table (section index, entry index)
    InlineTableField(usize, usize),
}

impl Cursor {
    /// Create a new cursor at the beginning
    pub fn new() -> Self {
        Self { index: 0 }
    }

    /// Get the current index
    pub fn index(&self) -> usize {
        self.index
    }

    /// Set the index directly
    #[allow(dead_code)]
    pub fn set_index(&mut self, index: usize) {
        self.index = index;
    }

    /// Move cursor up, skipping comments
    pub fn up(&mut self, doc: &TomlDocument) {
        let items = Self::build_visible_items(doc);
        let max = items.len();
        if max == 0 {
            return;
        }

        // Move up, skipping comments
        let mut new_idx = if self.index > 0 {
            self.index - 1
        } else {
            max - 1 // Wrap to end
        };

        // Skip any comments
        let mut attempts = 0;
        while matches!(items.get(new_idx), Some(CursorTarget::Comment(_))) && attempts < max {
            if new_idx > 0 {
                new_idx -= 1;
            } else {
                new_idx = max - 1;
            }
            attempts += 1;
        }
        self.index = new_idx;
    }

    /// Move cursor down, skipping comments
    pub fn down(&mut self, doc: &TomlDocument) {
        let items = Self::build_visible_items(doc);
        let max = items.len();
        if max == 0 {
            return;
        }

        // Move down, skipping comments
        let mut new_idx = if self.index < max - 1 {
            self.index + 1
        } else {
            0 // Wrap to beginning
        };

        // Skip any comments
        let mut attempts = 0;
        while matches!(items.get(new_idx), Some(CursorTarget::Comment(_))) && attempts < max {
            if new_idx < max - 1 {
                new_idx += 1;
            } else {
                new_idx = 0;
            }
            attempts += 1;
        }
        self.index = new_idx;
    }

    /// Jump to next section header
    pub fn next_section(&mut self, doc: &TomlDocument) {
        let items = Self::build_visible_items(doc);
        let current = self.index;

        // Find next section header after current position
        for (i, item) in items.iter().enumerate().skip(current + 1) {
            if matches!(item, CursorTarget::SectionHeader(_)) {
                self.index = i;
                return;
            }
        }

        // Wrap to first section header
        for (i, item) in items.iter().enumerate() {
            if matches!(item, CursorTarget::SectionHeader(_)) {
                self.index = i;
                return;
            }
        }
    }

    /// Jump to previous section header
    pub fn prev_section(&mut self, doc: &TomlDocument) {
        let items = Self::build_visible_items(doc);
        let current = self.index;

        // Find previous section header before current position
        for i in (0..current).rev() {
            if matches!(items[i], CursorTarget::SectionHeader(_)) {
                self.index = i;
                return;
            }
        }

        // Wrap to last section header
        for i in (0..items.len()).rev() {
            if matches!(items[i], CursorTarget::SectionHeader(_)) {
                self.index = i;
                return;
            }
        }
    }

    /// Get what the cursor is currently pointing at
    pub fn target(&self, doc: &TomlDocument) -> Option<CursorTarget> {
        let items = Self::build_visible_items(doc);
        items.get(self.index).cloned()
    }

    /// Ensure cursor is within valid bounds and not on a comment
    pub fn clamp(&mut self, doc: &TomlDocument) {
        let items = Self::build_visible_items(doc);
        let max = items.len();
        if max == 0 {
            self.index = 0;
            return;
        }
        if self.index >= max {
            self.index = max - 1;
        }

        // If on a comment, move to the next non-comment item
        let mut attempts = 0;
        while matches!(items.get(self.index), Some(CursorTarget::Comment(_))) && attempts < max {
            if self.index < max - 1 {
                self.index += 1;
            } else {
                self.index = 0;
            }
            attempts += 1;
        }
    }

    /// Build list of all visible items (for navigation)
    pub fn build_visible_items(doc: &TomlDocument) -> Vec<CursorTarget> {
        let mut items = Vec::new();

        for (section_idx, section) in doc.sections.iter().enumerate() {
            // Add section comments (non-navigable)
            for comment in &section.comments {
                items.push(CursorTarget::Comment(comment.clone()));
            }

            // Section header is always visible
            items.push(CursorTarget::SectionHeader(section_idx));

            // If expanded, show entries and add button
            if section.expanded {
                for (entry_idx, entry) in section.entries.iter().enumerate() {
                    // Add entry comments (non-navigable)
                    for comment in &entry.comments {
                        items.push(CursorTarget::Comment(format!("    {}", comment)));
                    }
                    items.push(CursorTarget::Entry(section_idx, entry_idx));

                    // If entry is expanded and complex, show sub-items
                    if entry.expanded {
                        match &entry.value {
                            EntryValue::Array(arr) => {
                                for array_idx in 0..arr.len() {
                                    items.push(CursorTarget::ArrayItem(
                                        section_idx,
                                        entry_idx,
                                        array_idx,
                                    ));
                                }
                                items.push(CursorTarget::AddButton(AddButtonKind::ArrayItem(
                                    section_idx,
                                    entry_idx,
                                )));
                            }
                            EntryValue::InlineTable(pairs) => {
                                for field_idx in 0..pairs.len() {
                                    items.push(CursorTarget::InlineTableField(
                                        section_idx,
                                        entry_idx,
                                        field_idx,
                                    ));
                                }
                                items.push(CursorTarget::AddButton(
                                    AddButtonKind::InlineTableField(section_idx, entry_idx),
                                ));
                            }
                            EntryValue::Simple(_) => {}
                        }
                    }
                }

                // Add section-specific buttons
                Self::add_section_buttons(&mut items, section_idx, &section.name);
            }
        }

        // Add section button
        items.push(CursorTarget::AddButton(AddButtonKind::Section));

        items
    }

    /// Add the appropriate add button(s) for a section based on its name
    fn add_section_buttons(items: &mut Vec<CursorTarget>, section_idx: usize, section_name: &str) {
        match section_name {
            // Root section (empty name) doesn't have add buttons
            // Users add top-level entries via the section picker
            "" => {}
            "tools" => {
                items.push(CursorTarget::AddButton(AddButtonKind::ToolRegistry(
                    section_idx,
                )));
                items.push(CursorTarget::AddButton(AddButtonKind::ToolBackend(
                    section_idx,
                )));
            }
            "env" => {
                items.push(CursorTarget::AddButton(AddButtonKind::EnvPath(section_idx)));
                items.push(CursorTarget::AddButton(AddButtonKind::EnvDotenv(
                    section_idx,
                )));
                items.push(CursorTarget::AddButton(AddButtonKind::EnvSource(
                    section_idx,
                )));
                items.push(CursorTarget::AddButton(AddButtonKind::EnvVariable(
                    section_idx,
                )));
            }
            "tasks" => {
                items.push(CursorTarget::AddButton(AddButtonKind::Task(section_idx)));
            }
            "prepare" => {
                items.push(CursorTarget::AddButton(AddButtonKind::Prepare(section_idx)));
            }
            "settings" => {
                items.push(CursorTarget::AddButton(AddButtonKind::Setting(section_idx)));
            }
            "hooks" => {
                items.push(CursorTarget::AddButton(AddButtonKind::Hook(section_idx)));
            }
            "task_config" => {
                items.push(CursorTarget::AddButton(AddButtonKind::TaskConfig(
                    section_idx,
                )));
            }
            "monorepo" => {
                items.push(CursorTarget::AddButton(AddButtonKind::Monorepo(
                    section_idx,
                )));
            }
            _ => {
                // Generic entry button for unknown sections
                items.push(CursorTarget::AddButton(AddButtonKind::Entry(section_idx)));
            }
        }
    }

    /// Find index for a specific target
    pub fn find_index(doc: &TomlDocument, target: &CursorTarget) -> Option<usize> {
        let items = Self::build_visible_items(doc);
        items.iter().position(|t| t == target)
    }

    /// Move cursor to a specific target if it exists
    pub fn goto(&mut self, doc: &TomlDocument, target: &CursorTarget) {
        if let Some(idx) = Self::find_index(doc, target) {
            self.index = idx;
        }
    }
}

impl Default for Cursor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::TomlDocument;

    #[test]
    fn test_cursor_navigation() {
        let doc = TomlDocument::new();
        let mut cursor = Cursor::new();

        // First item should be section header
        assert!(matches!(
            cursor.target(&doc),
            Some(CursorTarget::SectionHeader(0))
        ));

        // Move down
        cursor.down(&doc);
        // Should be add tool from registry button for tools section (expanded)
        assert!(matches!(
            cursor.target(&doc),
            Some(CursorTarget::AddButton(AddButtonKind::ToolRegistry(0)))
        ));
    }

    #[test]
    fn test_cursor_wrap() {
        let doc = TomlDocument::new();
        let mut cursor = Cursor::new();

        // Move up from start should wrap to end
        cursor.up(&doc);
        let max = Cursor::build_visible_items(&doc).len();
        assert_eq!(cursor.index(), max - 1);
    }

    #[test]
    fn test_section_navigation() {
        let doc = TomlDocument::new();
        let mut cursor = Cursor::new();

        // Jump to next section
        cursor.next_section(&doc);
        assert!(matches!(
            cursor.target(&doc),
            Some(CursorTarget::SectionHeader(1))
        ));

        // Jump back
        cursor.prev_section(&doc);
        assert!(matches!(
            cursor.target(&doc),
            Some(CursorTarget::SectionHeader(0))
        ));
    }
}
