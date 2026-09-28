use super::*;

impl InteractiveConfig {
    pub(in crate::editor) fn handle_remove(&mut self) -> io::Result<()> {
        let target = self.cursor.target(&self.doc);

        match target {
            Some(CursorTarget::Entry(section_idx, entry_idx)) => {
                // Save to undo stack before deleting
                let entry = self.doc.sections[section_idx].entries[entry_idx].clone();
                self.undo_stack
                    .push(UndoAction::DeleteEntry(section_idx, entry_idx, entry));
                self.doc.delete_entry(section_idx, entry_idx);
                self.cursor.clamp(&self.doc);
            }
            Some(CursorTarget::ArrayItem(section_idx, entry_idx, array_idx)) => {
                if let EntryValue::Array(items) =
                    &self.doc.sections[section_idx].entries[entry_idx].value
                {
                    let value = items[array_idx].clone();
                    self.undo_stack.push(UndoAction::DeleteArrayItem(
                        section_idx,
                        entry_idx,
                        array_idx,
                        value,
                    ));
                }
                self.doc
                    .delete_array_item(section_idx, entry_idx, array_idx);
                self.cursor.clamp(&self.doc);
            }
            Some(CursorTarget::InlineTableField(section_idx, entry_idx, field_idx)) => {
                if let Some(section) = self.doc.sections.get_mut(section_idx)
                    && let Some(entry) = section.entries.get_mut(entry_idx)
                    && let EntryValue::InlineTable(ref mut pairs) = entry.value
                    && field_idx < pairs.len()
                {
                    let (key, value) = pairs.remove(field_idx);
                    self.undo_stack.push(UndoAction::DeleteInlineTableField(
                        section_idx,
                        entry_idx,
                        field_idx,
                        key,
                        value,
                    ));
                    self.doc.modified = true;
                }
                self.cursor.clamp(&self.doc);
            }
            Some(CursorTarget::SectionHeader(section_idx)) => {
                // Save to undo stack before deleting
                let section = self.doc.sections[section_idx].clone();
                self.undo_stack
                    .push(UndoAction::DeleteSection(section_idx, section));
                self.doc.delete_section(section_idx);
                self.cursor.clamp(&self.doc);
            }
            _ => {}
        }

        Ok(())
    }

    pub(in crate::editor) fn undo(&mut self) {
        if let Some(action) = self.undo_stack.pop() {
            match action {
                UndoAction::DeleteEntry(section_idx, entry_idx, entry) => {
                    // Re-insert the entry at its original position
                    if section_idx < self.doc.sections.len() {
                        let entries = &mut self.doc.sections[section_idx].entries;
                        let insert_idx = entry_idx.min(entries.len());
                        entries.insert(insert_idx, entry);
                        self.doc.modified = true;
                        let target = CursorTarget::Entry(section_idx, insert_idx);
                        self.cursor.goto(&self.doc, &target);
                    }
                }
                UndoAction::DeleteArrayItem(section_idx, entry_idx, array_idx, value) => {
                    if let Some(section) = self.doc.sections.get_mut(section_idx)
                        && let Some(entry) = section.entries.get_mut(entry_idx)
                        && let EntryValue::Array(ref mut items) = entry.value
                    {
                        let insert_idx = array_idx.min(items.len());
                        items.insert(insert_idx, value);
                        self.doc.modified = true;
                        let target = CursorTarget::ArrayItem(section_idx, entry_idx, insert_idx);
                        self.cursor.goto(&self.doc, &target);
                    }
                }
                UndoAction::DeleteInlineTableField(
                    section_idx,
                    entry_idx,
                    field_idx,
                    key,
                    value,
                ) => {
                    if let Some(section) = self.doc.sections.get_mut(section_idx)
                        && let Some(entry) = section.entries.get_mut(entry_idx)
                        && let EntryValue::InlineTable(ref mut pairs) = entry.value
                    {
                        let insert_idx = field_idx.min(pairs.len());
                        pairs.insert(insert_idx, (key, value));
                        self.doc.modified = true;
                        let target =
                            CursorTarget::InlineTableField(section_idx, entry_idx, insert_idx);
                        self.cursor.goto(&self.doc, &target);
                    }
                }
                UndoAction::DeleteSection(section_idx, section) => {
                    let insert_idx = section_idx.min(self.doc.sections.len());
                    self.doc.sections.insert(insert_idx, section);
                    self.doc.modified = true;
                    let target = CursorTarget::SectionHeader(insert_idx);
                    self.cursor.goto(&self.doc, &target);
                }
                UndoAction::AddEntry(section_idx, entry_idx) => {
                    // Remove the added entry
                    if section_idx < self.doc.sections.len() {
                        let entries = &mut self.doc.sections[section_idx].entries;
                        if entry_idx < entries.len() {
                            entries.remove(entry_idx);
                            self.doc.modified = true;
                            self.cursor.clamp(&self.doc);
                        }
                    }
                }
                UndoAction::AddArrayItem(section_idx, entry_idx, array_idx) => {
                    if let Some(section) = self.doc.sections.get_mut(section_idx)
                        && let Some(entry) = section.entries.get_mut(entry_idx)
                        && let EntryValue::Array(ref mut items) = entry.value
                        && array_idx < items.len()
                    {
                        items.remove(array_idx);
                        self.doc.modified = true;
                        self.cursor.clamp(&self.doc);
                    }
                }
                UndoAction::AddInlineTableField(section_idx, entry_idx, field_idx) => {
                    if let Some(section) = self.doc.sections.get_mut(section_idx)
                        && let Some(entry) = section.entries.get_mut(entry_idx)
                        && let EntryValue::InlineTable(ref mut pairs) = entry.value
                        && field_idx < pairs.len()
                    {
                        pairs.remove(field_idx);
                        self.doc.modified = true;
                        self.cursor.clamp(&self.doc);
                    }
                }
                UndoAction::AddSection(section_idx) => {
                    if section_idx < self.doc.sections.len() {
                        self.doc.sections.remove(section_idx);
                        self.doc.modified = true;
                        self.cursor.clamp(&self.doc);
                    }
                }
                UndoAction::EditEntry(section_idx, entry_idx, old_value) => {
                    if let Some(section) = self.doc.sections.get_mut(section_idx)
                        && let Some(entry) = section.entries.get_mut(entry_idx)
                    {
                        entry.value = old_value;
                        self.doc.modified = true;
                        let target = CursorTarget::Entry(section_idx, entry_idx);
                        self.cursor.goto(&self.doc, &target);
                    }
                }
                UndoAction::EditArrayItem(section_idx, entry_idx, array_idx, old_value) => {
                    if let Some(section) = self.doc.sections.get_mut(section_idx)
                        && let Some(entry) = section.entries.get_mut(entry_idx)
                        && let EntryValue::Array(ref mut items) = entry.value
                        && array_idx < items.len()
                    {
                        items[array_idx] = old_value;
                        self.doc.modified = true;
                        let target = CursorTarget::ArrayItem(section_idx, entry_idx, array_idx);
                        self.cursor.goto(&self.doc, &target);
                    }
                }
                UndoAction::EditInlineTableField(section_idx, entry_idx, field_idx, old_value) => {
                    if let Some(section) = self.doc.sections.get_mut(section_idx)
                        && let Some(entry) = section.entries.get_mut(entry_idx)
                        && let EntryValue::InlineTable(ref mut pairs) = entry.value
                        && field_idx < pairs.len()
                    {
                        pairs[field_idx].1 = old_value;
                        self.doc.modified = true;
                        let target =
                            CursorTarget::InlineTableField(section_idx, entry_idx, field_idx);
                        self.cursor.goto(&self.doc, &target);
                    }
                }
                UndoAction::RenameEntry(section_idx, entry_idx, old_key) => {
                    if let Some(section) = self.doc.sections.get_mut(section_idx)
                        && let Some(entry) = section.entries.get_mut(entry_idx)
                    {
                        entry.key = old_key;
                        self.doc.modified = true;
                        let target = CursorTarget::Entry(section_idx, entry_idx);
                        self.cursor.goto(&self.doc, &target);
                    }
                }
                UndoAction::ConvertToInlineTable(section_idx, entry_idx, old_value) => {
                    if let Some(section) = self.doc.sections.get_mut(section_idx)
                        && let Some(entry) = section.entries.get_mut(entry_idx)
                    {
                        entry.value = EntryValue::Simple(old_value);
                        entry.expanded = false;
                        self.doc.modified = true;
                        let target = CursorTarget::Entry(section_idx, entry_idx);
                        self.cursor.goto(&self.doc, &target);
                    }
                }
            }
        }
    }
}
