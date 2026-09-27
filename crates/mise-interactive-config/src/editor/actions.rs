//! Action handlers for the interactive editor

mod edit;
mod enter;
mod undo;

use std::io;

use super::InteractiveConfig;
use super::undo::UndoAction;
use crate::cursor::{AddButtonKind, CursorTarget};
use crate::document::EntryValue;
use crate::inline_edit::InlineEdit;
use crate::picker::{PickerItem, PickerState};
use crate::providers::version_variants;
use crate::render::{BooleanSelectState, Mode, PickerKind, VersionSelectState};

impl InteractiveConfig {
    pub(super) fn handle_add_options(&mut self) -> io::Result<()> {
        let target = self.cursor.target(&self.doc);

        // Only works on simple entries (not already inline tables or arrays)
        if let Some(CursorTarget::Entry(section_idx, entry_idx)) = target {
            let entry = &self.doc.sections[section_idx].entries[entry_idx];
            if let EntryValue::Simple(old_value) = &entry.value {
                // Save old value for undo
                self.undo_stack.push(UndoAction::ConvertToInlineTable(
                    section_idx,
                    entry_idx,
                    old_value.clone(),
                ));
                // Convert to inline table with version key
                self.doc.convert_to_inline_table(section_idx, entry_idx);
                // The entry is now expanded, cursor stays on it
            }
        }

        Ok(())
    }

    pub(super) fn handle_rename(&mut self) -> io::Result<()> {
        let target = self.cursor.target(&self.doc);

        match target {
            Some(CursorTarget::Entry(section_idx, entry_idx)) => {
                let key = self.doc.sections[section_idx].entries[entry_idx]
                    .key
                    .clone();
                self.mode = Mode::RenameKey(section_idx, entry_idx, InlineEdit::new(&key));
            }
            Some(CursorTarget::InlineTableField(section_idx, entry_idx, field_idx)) => {
                if let EntryValue::InlineTable(pairs) =
                    &self.doc.sections[section_idx].entries[entry_idx].value
                {
                    let key = pairs[field_idx].0.clone();
                    // For inline table fields, we use a different approach
                    // Store field info in a way we can use it
                    self.mode = Mode::RenameKey(section_idx, entry_idx, InlineEdit::new(&key));
                    // Note: We'll need to track the field_idx somehow
                    // For now, we only support entry rename
                }
            }
            _ => {}
        }

        Ok(())
    }

    pub(super) fn handle_expand(&mut self) {
        let target = self.cursor.target(&self.doc);

        match target {
            Some(CursorTarget::SectionHeader(section_idx))
                if !self.doc.sections[section_idx].expanded =>
            {
                self.doc.sections[section_idx].expanded = true;
            }
            Some(CursorTarget::Entry(section_idx, entry_idx)) => {
                let entry = &self.doc.sections[section_idx].entries[entry_idx];
                // Only expand if it's a complex type (array or inline table)
                if !entry.expanded
                    && matches!(
                        entry.value,
                        EntryValue::Array(_) | EntryValue::InlineTable(_)
                    )
                {
                    self.doc.sections[section_idx].entries[entry_idx].expanded = true;
                }
            }
            _ => {}
        }
    }

    pub(super) fn handle_collapse(&mut self) {
        let target = self.cursor.target(&self.doc);

        match target {
            Some(CursorTarget::SectionHeader(section_idx))
                if self.doc.sections[section_idx].expanded =>
            {
                self.doc.sections[section_idx].expanded = false;
            }
            Some(CursorTarget::Entry(section_idx, entry_idx))
                if self.doc.sections[section_idx].entries[entry_idx].expanded =>
            {
                self.doc.sections[section_idx].entries[entry_idx].expanded = false;
            }
            // If on a child item (array item or inline table field), collapse the parent entry
            Some(CursorTarget::ArrayItem(section_idx, entry_idx, _))
            | Some(CursorTarget::InlineTableField(section_idx, entry_idx, _)) => {
                self.doc.sections[section_idx].entries[entry_idx].expanded = false;
                // Move cursor to the parent entry
                let target = CursorTarget::Entry(section_idx, entry_idx);
                self.cursor.goto(&self.doc, &target);
            }
            _ => {}
        }
    }

    pub(super) fn save(&mut self) -> io::Result<()> {
        if self.dry_run {
            self.renderer.flash_message("Dry-run mode: not saving")?;
        } else {
            self.doc.save(&self.path)?;
            self.doc.modified = false;
            self.renderer.flash_message("Saved")?;
        }
        Ok(())
    }
}
