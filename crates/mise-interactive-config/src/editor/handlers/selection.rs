use super::*;

impl InteractiveConfig {
    pub(in crate::editor) fn handle_version_select_key(
        &mut self,
        key: Key,
    ) -> io::Result<Option<ConfigResult>> {
        use crate::providers::VERSION_CUSTOM_MARKER;

        // Take ownership of the mode
        let mode = std::mem::replace(&mut self.mode, Mode::Navigate);

        if let Mode::VersionSelect(mut vs) = mode {
            match key {
                Key::Escape => {
                    // Cancel and return to navigate mode
                    self.mode = Mode::Navigate;
                }
                Key::Enter => {
                    let version = vs.current().to_string();
                    if version == VERSION_CUSTOM_MARKER {
                        // Switch to inline edit for custom version entry
                        // Move cursor to the entry we're editing
                        let target = CursorTarget::Entry(vs.section_idx, vs.entry_idx);
                        self.cursor.goto(&self.doc, &target);
                        // Get current value to pre-fill the edit
                        let current = if let Some(entry) = self
                            .doc
                            .sections
                            .get(vs.section_idx)
                            .and_then(|s| s.entries.get(vs.entry_idx))
                        {
                            match &entry.value {
                                EntryValue::Simple(v) => v.clone(),
                                _ => String::new(),
                            }
                        } else {
                            String::new()
                        };
                        self.mode = Mode::Edit(InlineEdit::new(&current));
                    } else {
                        // Confirm selection and update the entry
                        // Save old value for undo
                        let old_value = self.doc.sections[vs.section_idx].entries[vs.entry_idx]
                            .value
                            .clone();
                        self.undo_stack.push(UndoAction::EditEntry(
                            vs.section_idx,
                            vs.entry_idx,
                            old_value,
                        ));
                        self.doc
                            .update_entry(vs.section_idx, vs.entry_idx, version.clone());
                        // Remember this specificity for future tools
                        self.preferred_specificity = vs.selected;
                        self.mode = Mode::Navigate;
                    }
                }
                Key::ArrowLeft | Key::Char('h') => {
                    vs.prev();
                    self.mode = Mode::VersionSelect(vs);
                }
                Key::ArrowRight | Key::Char('l') => {
                    vs.next();
                    self.mode = Mode::VersionSelect(vs);
                }
                _ => {
                    // Keep current state
                    self.mode = Mode::VersionSelect(vs);
                }
            }
        }
        Ok(None)
    }

    pub(in crate::editor) fn handle_boolean_select_key(
        &mut self,
        key: Key,
    ) -> io::Result<Option<ConfigResult>> {
        // Take ownership of the mode
        let mode = std::mem::replace(&mut self.mode, Mode::Navigate);

        if let Mode::BooleanSelect(mut bs) = mode {
            match key {
                Key::Escape => {
                    // Cancel and return to navigate mode
                    self.mode = Mode::Navigate;
                }
                Key::Enter => {
                    // Confirm selection
                    let value = bs.value_str().to_string();
                    if let (Some(entry_idx), Some(field_idx)) = (bs.entry_idx, bs.field_idx) {
                        // Editing inline table field
                        if let Some(section) = self.doc.sections.get_mut(bs.section_idx)
                            && let Some(entry) = section.entries.get_mut(entry_idx)
                            && let EntryValue::InlineTable(ref mut pairs) = entry.value
                            && let Some((_, v)) = pairs.get_mut(field_idx)
                        {
                            *v = value;
                            self.doc.modified = true;
                        }
                    } else if let Some(entry_idx) = bs.entry_idx {
                        // Editing existing entry
                        self.doc.update_entry(bs.section_idx, entry_idx, value);
                    } else {
                        // Adding new entry
                        self.doc.add_entry(bs.section_idx, bs.key.clone(), value);
                        let entry_idx = self.doc.sections[bs.section_idx].entries.len() - 1;
                        // Track undo for added entry
                        self.undo_stack
                            .push(UndoAction::AddEntry(bs.section_idx, entry_idx));
                        let target = CursorTarget::Entry(bs.section_idx, entry_idx);
                        self.cursor.goto(&self.doc, &target);
                    }
                    self.mode = Mode::Navigate;
                }
                Key::ArrowLeft | Key::ArrowRight | Key::Char('h') | Key::Char('l') | Key::Tab => {
                    // Toggle between true and false
                    bs.toggle();
                    self.mode = Mode::BooleanSelect(bs);
                }
                Key::Char('t') => {
                    // Quick select true
                    bs.selected = true;
                    self.mode = Mode::BooleanSelect(bs);
                }
                Key::Char('f') => {
                    // Quick select false
                    bs.selected = false;
                    self.mode = Mode::BooleanSelect(bs);
                }
                _ => {
                    // Keep current state
                    self.mode = Mode::BooleanSelect(bs);
                }
            }
        }
        Ok(None)
    }
}
