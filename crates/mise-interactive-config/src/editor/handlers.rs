//! Key handlers for different editor modes

use console::Key;
use std::io;

use super::undo::UndoAction;
use super::{ConfigResult, InteractiveConfig};
use crate::cursor::{AddButtonKind, CursorTarget};
use crate::document::EntryValue;
use crate::inline_edit::InlineEdit;
use crate::providers::version_variants;
use crate::render::{BooleanSelectState, Mode, PickerKind, VersionSelectState};

impl InteractiveConfig {
    /// Handle a key press. Returns Some(result) if the editor should exit.
    pub(super) async fn handle_key(&mut self, key: Key) -> io::Result<Option<ConfigResult>> {
        match &mut self.mode {
            Mode::Navigate => self.handle_navigate_key(key).await,
            Mode::Edit(_) => self.handle_edit_key(key),
            Mode::NewKey(_) => self.handle_new_key_key(key),
            Mode::ConfirmQuit => self.handle_confirm_quit_key(key),
            Mode::RenameKey(_, _, _) => self.handle_rename_key(key),
            Mode::Picker(_, _) => self.handle_picker_key(key).await,
            Mode::VersionSelect(_) => self.handle_version_select_key(key),
            Mode::BackendToolName(_, _, _) => self.handle_backend_tool_name_key(key),
            Mode::BooleanSelect(_) => self.handle_boolean_select_key(key),
            Mode::Loading(_) => {
                // Loading mode doesn't handle key presses - it's a transient state
                Ok(None)
            }
        }
    }

    pub(super) async fn handle_navigate_key(
        &mut self,
        key: Key,
    ) -> io::Result<Option<ConfigResult>> {
        match key {
            // Navigation
            Key::ArrowUp | Key::Char('k') => {
                self.cursor.up(&self.doc);
            }
            Key::ArrowDown | Key::Char('j') => {
                self.cursor.down(&self.doc);
            }
            Key::ArrowLeft | Key::Char('h') => {
                self.handle_collapse();
            }
            Key::ArrowRight | Key::Char('l') => {
                self.handle_expand();
            }
            Key::Tab => {
                self.cursor.next_section(&self.doc);
            }
            Key::BackTab => {
                self.cursor.prev_section(&self.doc);
            }

            // Actions
            Key::Enter => {
                self.handle_enter().await?;
            }
            Key::Backspace => {
                self.handle_remove()?;
            }
            Key::Char('o') => {
                self.handle_add_options()?;
            }
            Key::Char('u') => {
                self.undo();
            }
            Key::Char('r') => {
                self.handle_rename()?;
            }
            Key::Char('s') if !self.dry_run => {
                self.save()?;
                let content = self.doc.to_toml();
                return Ok(Some(ConfigResult::Saved(content)));
            }
            Key::Char('q') | Key::Escape => {
                if self.dry_run {
                    // In dry-run mode, just exit immediately
                    let content = self.doc.to_toml();
                    return Ok(Some(ConfigResult::Saved(content)));
                } else if self.doc.modified {
                    self.mode = Mode::ConfirmQuit;
                } else {
                    return Ok(Some(ConfigResult::Cancelled));
                }
            }

            _ => {}
        }
        Ok(None)
    }

    pub(super) fn handle_edit_key(&mut self, key: Key) -> io::Result<Option<ConfigResult>> {
        if let Mode::Edit(ref mut edit) = self.mode {
            match key {
                Key::Enter => {
                    let value = std::mem::replace(edit, InlineEdit::new("")).confirm();
                    self.apply_edit(value);
                    self.mode = Mode::Navigate;
                }
                Key::Escape => {
                    self.mode = Mode::Navigate;
                }
                Key::ArrowLeft => {
                    edit.left();
                }
                Key::ArrowRight => {
                    edit.right();
                }
                Key::Home => {
                    edit.home();
                }
                Key::End => {
                    edit.end();
                }
                Key::Backspace => {
                    edit.backspace();
                }
                Key::Del => {
                    edit.delete();
                }
                Key::Char(c) => {
                    edit.insert(c);
                }
                _ => {}
            }
        }
        Ok(None)
    }

    pub(super) fn handle_new_key_key(&mut self, key: Key) -> io::Result<Option<ConfigResult>> {
        if let Mode::NewKey(ref mut edit) = self.mode {
            match key {
                Key::Enter => {
                    // Check if we need KEY=value validation (for env variables)
                    let needs_key_value = matches!(
                        self.cursor.target(&self.doc),
                        Some(CursorTarget::AddButton(AddButtonKind::EnvVariable(_)))
                    );

                    let input = edit.buffer().to_string();

                    // Validate KEY=value format if needed
                    if needs_key_value && !input.contains('=') {
                        // Don't accept - needs KEY=value format
                        return Ok(None);
                    }

                    let key_name = std::mem::replace(edit, InlineEdit::new("")).confirm();
                    if !key_name.is_empty() {
                        self.apply_new_key(key_name);
                    }
                    self.mode = Mode::Navigate;
                }
                Key::Escape => {
                    self.mode = Mode::Navigate;
                }
                Key::ArrowLeft => {
                    edit.left();
                }
                Key::ArrowRight => {
                    edit.right();
                }
                Key::Home => {
                    edit.home();
                }
                Key::End => {
                    edit.end();
                }
                Key::Backspace => {
                    edit.backspace();
                }
                Key::Del => {
                    edit.delete();
                }
                Key::Char(c) => {
                    edit.insert(c);
                }
                _ => {}
            }
        }
        Ok(None)
    }

    pub(super) fn handle_confirm_quit_key(&mut self, key: Key) -> io::Result<Option<ConfigResult>> {
        match key {
            Key::Char('y') | Key::Char('Y') => {
                self.save()?;
                let content = self.doc.to_toml();
                return Ok(Some(ConfigResult::Saved(content)));
            }
            Key::Char('n') | Key::Char('N') => {
                return Ok(Some(ConfigResult::Cancelled));
            }
            Key::Escape => {
                self.mode = Mode::Navigate;
            }
            _ => {}
        }
        Ok(None)
    }

    pub(super) async fn handle_picker_key(&mut self, key: Key) -> io::Result<Option<ConfigResult>> {
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
                Key::ArrowUp | Key::Char('k') => {
                    picker.move_up();
                    self.mode = Mode::Picker(kind, Box::new(picker));
                }
                Key::ArrowDown | Key::Char('j') => {
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

    pub(super) fn handle_backend_tool_name_key(
        &mut self,
        key: Key,
    ) -> io::Result<Option<ConfigResult>> {
        // Take ownership of the mode
        let mode = std::mem::replace(&mut self.mode, Mode::Navigate);

        if let Mode::BackendToolName(backend_name, section_idx, mut edit) = mode {
            match key {
                Key::Escape => {
                    // Cancel and return to navigate mode
                    self.mode = Mode::Navigate;
                }
                Key::Enter => {
                    let tool_name = edit.confirm();
                    if !tool_name.is_empty() {
                        // Combine backend and tool name (e.g., "cargo:ripgrep")
                        let full_name = format!("{}:{}", backend_name, tool_name);
                        self.doc
                            .add_entry(section_idx, full_name, "latest".to_string());
                        // Move cursor to the new entry
                        let entry_idx = self.doc.sections[section_idx].entries.len() - 1;
                        // Track undo for added entry
                        self.undo_stack
                            .push(UndoAction::AddEntry(section_idx, entry_idx));
                        let target = CursorTarget::Entry(section_idx, entry_idx);
                        self.cursor.goto(&self.doc, &target);
                    }
                    self.mode = Mode::Navigate;
                }
                Key::ArrowLeft => {
                    edit.left();
                    self.mode = Mode::BackendToolName(backend_name, section_idx, edit);
                }
                Key::ArrowRight => {
                    edit.right();
                    self.mode = Mode::BackendToolName(backend_name, section_idx, edit);
                }
                Key::Home => {
                    edit.home();
                    self.mode = Mode::BackendToolName(backend_name, section_idx, edit);
                }
                Key::End => {
                    edit.end();
                    self.mode = Mode::BackendToolName(backend_name, section_idx, edit);
                }
                Key::Backspace => {
                    edit.backspace();
                    self.mode = Mode::BackendToolName(backend_name, section_idx, edit);
                }
                Key::Del => {
                    edit.delete();
                    self.mode = Mode::BackendToolName(backend_name, section_idx, edit);
                }
                Key::Char(c) => {
                    edit.insert(c);
                    self.mode = Mode::BackendToolName(backend_name, section_idx, edit);
                }
                _ => {
                    self.mode = Mode::BackendToolName(backend_name, section_idx, edit);
                }
            }
        }
        Ok(None)
    }

    pub(super) fn handle_version_select_key(
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

    pub(super) fn handle_boolean_select_key(
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

    pub(super) fn handle_rename_key(&mut self, key: Key) -> io::Result<Option<ConfigResult>> {
        // Take ownership of the mode
        let mode = std::mem::replace(&mut self.mode, Mode::Navigate);

        if let Mode::RenameKey(section_idx, entry_idx, mut edit) = mode {
            match key {
                Key::Escape => {
                    self.mode = Mode::Navigate;
                }
                Key::Enter => {
                    let new_key = edit.confirm();
                    if !new_key.is_empty() {
                        // Apply the rename
                        if let Some(section) = self.doc.sections.get_mut(section_idx)
                            && let Some(entry) = section.entries.get_mut(entry_idx)
                            && entry.key != new_key
                        {
                            // Save old key for undo
                            let old_key = entry.key.clone();
                            self.undo_stack.push(UndoAction::RenameEntry(
                                section_idx,
                                entry_idx,
                                old_key,
                            ));
                            entry.key = new_key;
                            self.doc.modified = true;
                        }
                    }
                    self.mode = Mode::Navigate;
                }
                Key::ArrowLeft => {
                    edit.left();
                    self.mode = Mode::RenameKey(section_idx, entry_idx, edit);
                }
                Key::ArrowRight => {
                    edit.right();
                    self.mode = Mode::RenameKey(section_idx, entry_idx, edit);
                }
                Key::Home => {
                    edit.home();
                    self.mode = Mode::RenameKey(section_idx, entry_idx, edit);
                }
                Key::End => {
                    edit.end();
                    self.mode = Mode::RenameKey(section_idx, entry_idx, edit);
                }
                Key::Backspace => {
                    edit.backspace();
                    self.mode = Mode::RenameKey(section_idx, entry_idx, edit);
                }
                Key::Del => {
                    edit.delete();
                    self.mode = Mode::RenameKey(section_idx, entry_idx, edit);
                }
                Key::Char(c) => {
                    edit.insert(c);
                    self.mode = Mode::RenameKey(section_idx, entry_idx, edit);
                }
                _ => {
                    self.mode = Mode::RenameKey(section_idx, entry_idx, edit);
                }
            }
        }
        Ok(None)
    }
}
