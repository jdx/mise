use super::*;

impl InteractiveConfig {
    pub(in crate::editor) async fn handle_picker_key(
        &mut self,
        key: Key,
    ) -> io::Result<Option<ConfigResult>> {
        // We need to take ownership of the mode to modify the picker
        let mode = std::mem::replace(&mut self.mode, Mode::Navigate);

        if let Mode::Picker(kind, picker) = mode {
            let mut picker = *picker;
            match key {
                Key::Escape => {
                    // Cancel and return to navigate mode
                    self.mode = Mode::Navigate;
                }
                Key::Enter => {
                    // Select the current item
                    if let Some(selected) = picker.selected() {
                        let tool_name = selected.name.clone();
                        match &kind {
                            PickerKind::Tool(section_idx) => {
                                self.handle_picker_tool_select(&tool_name, *section_idx)
                                    .await;
                            }
                            PickerKind::Backend(section_idx) => {
                                // Transition to entering the tool name with the selected backend
                                let backend_name = tool_name;
                                self.mode = Mode::BackendToolName(
                                    backend_name,
                                    *section_idx,
                                    InlineEdit::new(""),
                                );
                            }
                            PickerKind::Setting(section_idx) => {
                                self.handle_picker_setting_select(&tool_name, *section_idx);
                            }
                            PickerKind::Hook(section_idx) => {
                                // Add the selected hook with empty value
                                self.doc.add_entry(*section_idx, tool_name, String::new());
                                let entry_idx = self.doc.sections[*section_idx].entries.len() - 1;
                                // Track undo for added entry
                                self.undo_stack
                                    .push(UndoAction::AddEntry(*section_idx, entry_idx));
                                let target = CursorTarget::Entry(*section_idx, entry_idx);
                                self.cursor.goto(&self.doc, &target);
                                self.mode = Mode::Edit(InlineEdit::new(""));
                            }
                            PickerKind::TaskConfig(section_idx) => {
                                self.handle_picker_task_config_select(&tool_name, *section_idx);
                            }
                            PickerKind::Monorepo(section_idx) => {
                                self.handle_picker_monorepo_select(&tool_name, *section_idx);
                            }
                            PickerKind::Section => {
                                self.handle_picker_section_select(&tool_name);
                            }
                        }
                    } else {
                        // No selection, return to navigate
                        self.mode = Mode::Navigate;
                    }
                }
                Key::ArrowUp => {
                    picker.move_up();
                    self.mode = Mode::Picker(kind, Box::new(picker));
                }
                Key::ArrowDown => {
                    picker.move_down();
                    self.mode = Mode::Picker(kind, Box::new(picker));
                }
                Key::Backspace => {
                    picker.backspace();
                    self.mode = Mode::Picker(kind, Box::new(picker));
                }
                Key::Char(c) => {
                    picker.type_char(c);
                    self.mode = Mode::Picker(kind, Box::new(picker));
                }
                _ => {
                    // Keep current state for unhandled keys
                    self.mode = Mode::Picker(kind, Box::new(picker));
                }
            }
        }
        Ok(None)
    }

    async fn handle_picker_tool_select(&mut self, tool_name: &str, section_idx: usize) {
        // Add the selected tool with default version
        self.doc
            .add_entry(section_idx, tool_name.to_string(), "latest".to_string());
        // Move cursor to the new entry
        let entry_idx = self.doc.sections[section_idx].entries.len() - 1;
        // Track undo for added entry
        self.undo_stack
            .push(UndoAction::AddEntry(section_idx, entry_idx));
        let target = CursorTarget::Entry(section_idx, entry_idx);
        self.cursor.goto(&self.doc, &target);

        // Show loading indicator while fetching version info
        self.mode = Mode::Loading(format!("Fetching versions for {}...", tool_name));
        let _ = self.render_current_mode();

        // Try to use version selector if we have version info
        if let Some(latest) = self.version_provider.latest_version(tool_name).await {
            let variants = version_variants(&latest);
            let mut vs = VersionSelectState::new(
                tool_name.to_string(),
                variants.clone(),
                section_idx,
                entry_idx,
            );
            // Use preferred specificity, clamped to valid range
            vs.selected = self
                .preferred_specificity
                .min(variants.len().saturating_sub(2));
            self.mode = Mode::VersionSelect(vs);
        } else {
            // Fall back to inline edit
            self.mode = Mode::Edit(InlineEdit::new("latest"));
        }
    }

    fn handle_picker_setting_select(&mut self, tool_name: &str, section_idx: usize) {
        // Check if this is a boolean setting
        let schema_type = crate::schema::setting_type(tool_name);
        if schema_type == Some(crate::schema::SchemaType::Boolean) {
            // Show boolean picker
            self.mode = Mode::BooleanSelect(BooleanSelectState::new_entry(
                tool_name.to_string(),
                section_idx,
            ));
        } else {
            // Add with type-appropriate value
            let (value, needs_edit) = Self::type_appropriate_default(schema_type);
            self.doc
                .add_entry_with_value(section_idx, tool_name.to_string(), value);
            let entry_idx = self.doc.sections[section_idx].entries.len() - 1;
            // Track undo for added entry
            self.undo_stack
                .push(UndoAction::AddEntry(section_idx, entry_idx));
            let target = CursorTarget::Entry(section_idx, entry_idx);
            self.cursor.goto(&self.doc, &target);
            if needs_edit {
                self.mode = Mode::Edit(InlineEdit::new(""));
            } else {
                self.mode = Mode::Navigate;
            }
        }
    }

    fn handle_picker_task_config_select(&mut self, tool_name: &str, section_idx: usize) {
        // Check if this is a boolean
        let schema_type = crate::schema::task_config_type(tool_name);
        if schema_type == Some(crate::schema::SchemaType::Boolean) {
            // Show boolean picker
            self.mode = Mode::BooleanSelect(BooleanSelectState::new_entry(
                tool_name.to_string(),
                section_idx,
            ));
        } else {
            // Add with type-appropriate value
            let (value, needs_edit) = Self::type_appropriate_default(schema_type);
            self.doc
                .add_entry_with_value(section_idx, tool_name.to_string(), value);
            let entry_idx = self.doc.sections[section_idx].entries.len() - 1;
            // Track undo for added entry
            self.undo_stack
                .push(UndoAction::AddEntry(section_idx, entry_idx));
            let target = CursorTarget::Entry(section_idx, entry_idx);
            self.cursor.goto(&self.doc, &target);
            if needs_edit {
                self.mode = Mode::Edit(InlineEdit::new(""));
            } else {
                self.mode = Mode::Navigate;
            }
        }
    }

    fn handle_picker_monorepo_select(&mut self, tool_name: &str, section_idx: usize) {
        // Check if this is a boolean
        let schema_type = crate::schema::monorepo_type(tool_name);
        if schema_type == Some(crate::schema::SchemaType::Boolean) {
            // Show boolean picker
            self.mode = Mode::BooleanSelect(BooleanSelectState::new_entry(
                tool_name.to_string(),
                section_idx,
            ));
        } else {
            // Add with type-appropriate value
            let (value, needs_edit) = Self::type_appropriate_default(schema_type);
            self.doc
                .add_entry_with_value(section_idx, tool_name.to_string(), value);
            let entry_idx = self.doc.sections[section_idx].entries.len() - 1;
            // Track undo for added entry
            self.undo_stack
                .push(UndoAction::AddEntry(section_idx, entry_idx));
            let target = CursorTarget::Entry(section_idx, entry_idx);
            self.cursor.goto(&self.doc, &target);
            if needs_edit {
                self.mode = Mode::Edit(InlineEdit::new(""));
            } else {
                self.mode = Mode::Navigate;
            }
        }
    }

    fn handle_picker_section_select(&mut self, tool_name: &str) {
        // Check if the selected item is a section or a top-level entry
        let is_section = crate::schema::is_valid_section(tool_name);

        if is_section {
            // Add as a new section
            let count_before = self.doc.sections.len();
            self.doc.add_section(tool_name.to_string());
            // Find and move cursor to the new section
            if let Some(idx) = self.doc.sections.iter().position(|s| s.name == tool_name) {
                // Only track undo if a section was actually added
                if self.doc.sections.len() > count_before {
                    self.undo_stack.push(UndoAction::AddSection(idx));
                }
                let target = CursorTarget::SectionHeader(idx);
                self.cursor.goto(&self.doc, &target);
            }
            self.mode = Mode::Navigate;
        } else {
            // Add as a top-level entry (in root section with empty name)
            // Find or create the root section
            let root_idx =
                if let Some(idx) = self.doc.sections.iter().position(|s| s.name.is_empty()) {
                    idx
                } else {
                    // Create root section at the beginning
                    // This shifts all section indices, so clear undo stack to avoid corruption
                    self.undo_stack.clear();
                    self.doc.sections.insert(
                        0,
                        crate::document::Section {
                            name: String::new(),
                            entries: Vec::new(),
                            expanded: true,
                            comments: Vec::new(),
                        },
                    );
                    self.doc.modified = true;
                    0
                };

            // Check if this is a boolean entry
            let schema_type = crate::schema::entry_type(tool_name);
            if schema_type == Some(crate::schema::SchemaType::Boolean) {
                // Show boolean picker
                self.mode = Mode::BooleanSelect(BooleanSelectState::new_entry(
                    tool_name.to_string(),
                    root_idx,
                ));
            } else {
                // Add the entry with type-appropriate value
                let (value, needs_edit) = Self::type_appropriate_default(schema_type);
                self.doc
                    .add_entry_with_value(root_idx, tool_name.to_string(), value);
                let entry_idx = self.doc.sections[root_idx].entries.len() - 1;
                // Track undo for added entry
                self.undo_stack
                    .push(UndoAction::AddEntry(root_idx, entry_idx));
                let target = CursorTarget::Entry(root_idx, entry_idx);
                self.cursor.goto(&self.doc, &target);
                if needs_edit {
                    self.mode = Mode::Edit(InlineEdit::new(""));
                } else {
                    self.mode = Mode::Navigate;
                }
            }
        }
    }
}
