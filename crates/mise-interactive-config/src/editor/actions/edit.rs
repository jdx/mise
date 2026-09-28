use super::*;

impl InteractiveConfig {
    pub(in crate::editor) fn apply_edit(&mut self, value: String) {
        let target = self.cursor.target(&self.doc);

        match target {
            Some(CursorTarget::Entry(section_idx, entry_idx)) => {
                // Save old value for undo
                let old_value = self.doc.sections[section_idx].entries[entry_idx]
                    .value
                    .clone();
                self.undo_stack
                    .push(UndoAction::EditEntry(section_idx, entry_idx, old_value));
                self.doc.update_entry(section_idx, entry_idx, value);
            }
            Some(CursorTarget::ArrayItem(section_idx, entry_idx, array_idx)) => {
                // Save old value for undo
                if let EntryValue::Array(items) =
                    &self.doc.sections[section_idx].entries[entry_idx].value
                {
                    let old_value = items[array_idx].clone();
                    self.undo_stack.push(UndoAction::EditArrayItem(
                        section_idx,
                        entry_idx,
                        array_idx,
                        old_value,
                    ));
                }
                self.doc
                    .update_array_item(section_idx, entry_idx, array_idx, value);
            }
            Some(CursorTarget::InlineTableField(section_idx, entry_idx, field_idx)) => {
                if let Some(section) = self.doc.sections.get_mut(section_idx)
                    && let Some(entry) = section.entries.get_mut(entry_idx)
                    && let EntryValue::InlineTable(ref mut pairs) = entry.value
                    && let Some((_, v)) = pairs.get_mut(field_idx)
                {
                    // Save old value for undo
                    let old_value = v.clone();
                    self.undo_stack.push(UndoAction::EditInlineTableField(
                        section_idx,
                        entry_idx,
                        field_idx,
                        old_value,
                    ));
                    *v = value;
                    self.doc.modified = true;
                }
            }
            _ => {}
        }
    }

    pub(in crate::editor) fn apply_new_key(&mut self, key_name: String) {
        let target = self.cursor.target(&self.doc);

        match target {
            Some(CursorTarget::AddButton(AddButtonKind::Section)) => {
                let count_before = self.doc.sections.len();
                self.doc.add_section(key_name);
                // Only track undo if a section was actually added
                if self.doc.sections.len() > count_before {
                    let section_idx = self.doc.sections.len() - 1;
                    self.undo_stack.push(UndoAction::AddSection(section_idx));
                }
                // Move cursor to the new section
                self.cursor.clamp(&self.doc);
            }
            Some(CursorTarget::AddButton(AddButtonKind::Entry(section_idx)))
            | Some(CursorTarget::AddButton(AddButtonKind::Setting(section_idx))) => {
                self.doc.add_entry(section_idx, key_name, String::new());
                // Track undo for added entry
                let new_entry_idx = self.doc.sections[section_idx].entries.len() - 1;
                self.undo_stack
                    .push(UndoAction::AddEntry(section_idx, new_entry_idx));
                // Move cursor to the new entry to edit its value
                let target = CursorTarget::Entry(section_idx, new_entry_idx);
                self.cursor.goto(&self.doc, &target);
                // Start editing the value
                self.mode = Mode::Edit(InlineEdit::new(""));
            }
            Some(CursorTarget::AddButton(AddButtonKind::Task(section_idx))) => {
                // Create task as inline table with run field
                self.doc.sections[section_idx]
                    .entries
                    .push(crate::document::Entry {
                        key: key_name,
                        value: EntryValue::InlineTable(vec![("run".to_string(), String::new())]),
                        expanded: true,
                        comments: Vec::new(),
                        trailing_comment: None,
                    });
                self.doc.modified = true;
                // Track undo for added entry
                let new_entry_idx = self.doc.sections[section_idx].entries.len() - 1;
                self.undo_stack
                    .push(UndoAction::AddEntry(section_idx, new_entry_idx));
                // Move cursor to the run field and start editing
                let target = CursorTarget::InlineTableField(section_idx, new_entry_idx, 0);
                self.cursor.goto(&self.doc, &target);
                self.mode = Mode::Edit(InlineEdit::new(""));
            }
            Some(CursorTarget::AddButton(AddButtonKind::Deps(section_idx))) => {
                // Create deps provider as inline table (e.g., npm = { disable = true })
                self.doc.sections[section_idx]
                    .entries
                    .push(crate::document::Entry {
                        key: key_name,
                        value: EntryValue::InlineTable(Vec::new()),
                        expanded: true,
                        comments: Vec::new(),
                        trailing_comment: None,
                    });
                self.doc.modified = true;
                // Track undo for added entry
                let new_entry_idx = self.doc.sections[section_idx].entries.len() - 1;
                self.undo_stack
                    .push(UndoAction::AddEntry(section_idx, new_entry_idx));
                // Move cursor to the new entry
                let target = CursorTarget::Entry(section_idx, new_entry_idx);
                self.cursor.goto(&self.doc, &target);
                self.mode = Mode::Navigate;
            }
            Some(CursorTarget::AddButton(AddButtonKind::EnvVariable(section_idx))) => {
                // Parse KEY=value format
                if let Some((key, value)) = key_name.split_once('=') {
                    let key = key.trim().to_string();
                    let value = value.trim();
                    // Strip surrounding quotes (single or double) - must be at least 2 chars
                    let value = if value.len() >= 2
                        && ((value.starts_with('"') && value.ends_with('"'))
                            || (value.starts_with('\'') && value.ends_with('\'')))
                    {
                        value[1..value.len() - 1].to_string()
                    } else {
                        value.to_string()
                    };
                    if !key.is_empty() {
                        self.doc.add_entry(section_idx, key, value);
                        // Track undo for added entry
                        let new_entry_idx = self.doc.sections[section_idx].entries.len() - 1;
                        self.undo_stack
                            .push(UndoAction::AddEntry(section_idx, new_entry_idx));
                    }
                }
            }
            Some(CursorTarget::AddButton(AddButtonKind::ArrayItem(section_idx, entry_idx))) => {
                // key_name is actually the value for arrays
                self.doc.add_array_item(section_idx, entry_idx, key_name);
                // Track undo for added array item
                if let EntryValue::Array(items) =
                    &self.doc.sections[section_idx].entries[entry_idx].value
                {
                    let array_idx = items.len() - 1;
                    self.undo_stack.push(UndoAction::AddArrayItem(
                        section_idx,
                        entry_idx,
                        array_idx,
                    ));
                }
                self.cursor.clamp(&self.doc);
            }
            Some(CursorTarget::AddButton(AddButtonKind::InlineTableField(
                section_idx,
                entry_idx,
            ))) => {
                if let Some(section) = self.doc.sections.get_mut(section_idx)
                    && let Some(entry) = section.entries.get_mut(entry_idx)
                    && let EntryValue::InlineTable(ref mut pairs) = entry.value
                {
                    pairs.push((key_name, String::new()));
                    self.doc.modified = true;
                    // Track undo for added inline table field
                    let field_idx = pairs.len() - 1;
                    self.undo_stack.push(UndoAction::AddInlineTableField(
                        section_idx,
                        entry_idx,
                        field_idx,
                    ));
                }
                self.cursor.clamp(&self.doc);
                // Move to the new field and edit its value
                // TODO: implement moving to and editing the new field
            }
            Some(CursorTarget::AddButton(AddButtonKind::EnvDotenv(section_idx)))
                if !key_name.is_empty() =>
            {
                // key_name is the filename, add as mise.file = "<filename>"
                self.doc
                    .add_entry(section_idx, "mise.file".to_string(), key_name);
                // Track undo for added entry
                let new_entry_idx = self.doc.sections[section_idx].entries.len() - 1;
                self.undo_stack
                    .push(UndoAction::AddEntry(section_idx, new_entry_idx));
            }
            Some(CursorTarget::AddButton(AddButtonKind::EnvSource(section_idx)))
                if !key_name.is_empty() =>
            {
                // key_name is the script path, add as _.source = "<script>"
                self.doc
                    .add_entry(section_idx, "_.source".to_string(), key_name);
                // Track undo for added entry
                let new_entry_idx = self.doc.sections[section_idx].entries.len() - 1;
                self.undo_stack
                    .push(UndoAction::AddEntry(section_idx, new_entry_idx));
            }
            // ToolRegistry and EnvPath are handled in handle_enter directly
            Some(CursorTarget::AddButton(AddButtonKind::ToolRegistry(_)))
            | Some(CursorTarget::AddButton(AddButtonKind::EnvPath(_))) => {}
            Some(CursorTarget::AddButton(AddButtonKind::ToolBackend(section_idx)))
                if !key_name.is_empty() =>
            {
                // Add backend tool (e.g., cargo:ripgrep) with "latest" as default
                self.doc
                    .add_entry(section_idx, key_name, "latest".to_string());
                // Track undo for added entry
                let new_entry_idx = self.doc.sections[section_idx].entries.len() - 1;
                self.undo_stack
                    .push(UndoAction::AddEntry(section_idx, new_entry_idx));
            }
            Some(CursorTarget::AddButton(AddButtonKind::Hook(section_idx)))
                if !key_name.is_empty() =>
            {
                // Add custom hook entry (e.g., custom_hook = "echo hello")
                self.doc.add_entry(section_idx, key_name, String::new());
                // Track undo for added entry
                let new_entry_idx = self.doc.sections[section_idx].entries.len() - 1;
                self.undo_stack
                    .push(UndoAction::AddEntry(section_idx, new_entry_idx));
                // Move cursor to the new entry to edit its value
                let target = CursorTarget::Entry(section_idx, new_entry_idx);
                self.cursor.goto(&self.doc, &target);
                self.mode = Mode::Edit(InlineEdit::new(""));
            }
            Some(CursorTarget::AddButton(AddButtonKind::TaskConfig(section_idx)))
                if !key_name.is_empty() =>
            {
                // Add custom task_config entry
                self.doc.add_entry(section_idx, key_name, String::new());
                // Track undo for added entry
                let new_entry_idx = self.doc.sections[section_idx].entries.len() - 1;
                self.undo_stack
                    .push(UndoAction::AddEntry(section_idx, new_entry_idx));
                // Move cursor to the new entry to edit its value
                let target = CursorTarget::Entry(section_idx, new_entry_idx);
                self.cursor.goto(&self.doc, &target);
                self.mode = Mode::Edit(InlineEdit::new(""));
            }
            Some(CursorTarget::AddButton(AddButtonKind::Monorepo(section_idx)))
                if !key_name.is_empty() =>
            {
                // Add custom monorepo entry
                self.doc.add_entry(section_idx, key_name, String::new());
                // Track undo for added entry
                let new_entry_idx = self.doc.sections[section_idx].entries.len() - 1;
                self.undo_stack
                    .push(UndoAction::AddEntry(section_idx, new_entry_idx));
                // Move cursor to the new entry to edit its value
                let target = CursorTarget::Entry(section_idx, new_entry_idx);
                self.cursor.goto(&self.doc, &target);
                self.mode = Mode::Edit(InlineEdit::new(""));
            }
            _ => {}
        }
    }

    /// Get a type-appropriate default value for a schema type.
    /// Returns (value, needs_edit) where needs_edit indicates if the user should be prompted.
    pub(in crate::editor) fn type_appropriate_default(
        schema_type: Option<crate::schema::SchemaType>,
    ) -> (EntryValue, bool) {
        use crate::schema::SchemaType;
        match schema_type {
            Some(SchemaType::Boolean) => {
                // Booleans default to true, no edit needed
                (EntryValue::Simple("true".to_string()), false)
            }
            Some(SchemaType::Array) => {
                // Arrays start empty
                (EntryValue::Array(Vec::new()), false)
            }
            Some(SchemaType::Object) => {
                // Objects start as empty inline tables
                (EntryValue::InlineTable(Vec::new()), false)
            }
            Some(SchemaType::Integer) | Some(SchemaType::Number) => {
                // Numbers need user input, start with "0"
                (EntryValue::Simple("0".to_string()), true)
            }
            Some(SchemaType::String) | Some(SchemaType::Unknown) | None => {
                // Strings need user input
                (EntryValue::Simple(String::new()), true)
            }
        }
    }
}
