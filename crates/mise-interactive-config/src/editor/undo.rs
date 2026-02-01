//! Undo system for the interactive editor

use crate::document::{Entry, EntryValue, Section};

/// An action that can be undone
#[derive(Debug, Clone)]
pub enum UndoAction {
    /// Deleted an entry (section_idx, entry_idx, entry)
    DeleteEntry(usize, usize, Entry),
    /// Deleted an array item (section_idx, entry_idx, array_idx, value)
    DeleteArrayItem(usize, usize, usize, String),
    /// Deleted an inline table field (section_idx, entry_idx, field_idx, key, value)
    DeleteInlineTableField(usize, usize, usize, String, String),
    /// Deleted a section (section_idx, section)
    DeleteSection(usize, Section),
    /// Added an entry (section_idx, entry_idx)
    AddEntry(usize, usize),
    /// Added an array item (section_idx, entry_idx, array_idx)
    AddArrayItem(usize, usize, usize),
    /// Added an inline table field (section_idx, entry_idx, field_idx)
    AddInlineTableField(usize, usize, usize),
    /// Added a section (section_idx)
    AddSection(usize),
    /// Edited an entry value (section_idx, entry_idx, old_value)
    EditEntry(usize, usize, EntryValue),
    /// Edited an array item (section_idx, entry_idx, array_idx, old_value)
    EditArrayItem(usize, usize, usize, String),
    /// Edited an inline table field value (section_idx, entry_idx, field_idx, old_value)
    EditInlineTableField(usize, usize, usize, String),
    /// Renamed an entry key (section_idx, entry_idx, old_key)
    RenameEntry(usize, usize, String),
    /// Converted entry to inline table (section_idx, entry_idx, old_simple_value)
    ConvertToInlineTable(usize, usize, String),
}
