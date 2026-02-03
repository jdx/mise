//! Action handlers for the interactive editor

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
    pub(super) async fn handle_enter(&mut self) -> io::Result<()> {
        let target = self.cursor.target(&self.doc);

        match target {
            Some(CursorTarget::SectionHeader(section_idx)) => {
                self.doc.toggle_section(section_idx);
                // If we just expanded, move cursor to first entry or add button
                if self.doc.sections[section_idx].expanded {
                    self.cursor.down(&self.doc);
                }
            }

            Some(CursorTarget::Entry(section_idx, entry_idx)) => {
                let section_name = self.doc.sections[section_idx].name.clone();
                let entry = &self.doc.sections[section_idx].entries[entry_idx];
                let tool_name = entry.key.clone();
                let current_value_opt = match &entry.value {
                    EntryValue::Simple(v) => Some(v.clone()),
                    _ => None,
                };
                let is_complex = matches!(
                    entry.value,
                    EntryValue::Array(_) | EntryValue::InlineTable(_)
                );

                if let Some(current_value) = current_value_opt {
                    // For tools section, try to use version selector
                    if section_name == "tools" {
                        // Show loading indicator while fetching version info
                        self.mode =
                            Mode::Loading(format!("Fetching versions for {}...", tool_name));
                        let _ = self.render_current_mode();
                        if let Some(latest) = self.version_provider.latest_version(&tool_name).await
                        {
                            let variants = version_variants(&latest);
                            // Find which variant matches the current value, if any
                            let mut vs = VersionSelectState::new(
                                tool_name,
                                variants.clone(),
                                section_idx,
                                entry_idx,
                            );
                            // Try to match current value to a variant, or select "other..."
                            if let Some(pos) = variants.iter().position(|v| v == &current_value) {
                                vs.selected = pos;
                            } else {
                                // Current value is custom, select "other..."
                                vs.selected = variants.len().saturating_sub(1);
                            }
                            self.mode = Mode::VersionSelect(vs);
                        } else {
                            // Fall back to inline edit if no version info available
                            self.mode = Mode::Edit(InlineEdit::new(&current_value));
                        }
                    } else {
                        // Non-tools section: check if it's a boolean setting
                        let schema_type = match section_name.as_str() {
                            "settings" => crate::schema::setting_type(&tool_name),
                            "task_config" => crate::schema::task_config_type(&tool_name),
                            "monorepo" => crate::schema::monorepo_type(&tool_name),
                            "" => crate::schema::entry_type(&tool_name),
                            _ => None,
                        };

                        if schema_type == Some(crate::schema::SchemaType::Boolean) {
                            // Use boolean selector for boolean settings
                            let current_bool = current_value == "true";
                            self.mode = Mode::BooleanSelect(BooleanSelectState::edit_entry(
                                tool_name,
                                current_bool,
                                section_idx,
                                entry_idx,
                            ));
                        } else {
                            // Use regular inline edit
                            self.mode = Mode::Edit(InlineEdit::new(&current_value));
                        }
                    }
                } else if is_complex {
                    // Toggle expansion for arrays/inline tables
                    self.doc.toggle_entry(section_idx, entry_idx);
                }
            }

            Some(CursorTarget::ArrayItem(section_idx, entry_idx, array_idx)) => {
                if let EntryValue::Array(items) =
                    &self.doc.sections[section_idx].entries[entry_idx].value
                {
                    let value = items[array_idx].clone();
                    self.mode = Mode::Edit(InlineEdit::new(&value));
                }
            }

            Some(CursorTarget::InlineTableField(section_idx, entry_idx, field_idx)) => {
                if let EntryValue::InlineTable(pairs) =
                    &self.doc.sections[section_idx].entries[entry_idx].value
                {
                    let (key, value) = &pairs[field_idx];
                    // Check if it's a boolean value
                    if value == "true" || value == "false" {
                        let current_bool = value == "true";
                        self.mode =
                            Mode::BooleanSelect(BooleanSelectState::edit_inline_table_field(
                                key.clone(),
                                current_bool,
                                section_idx,
                                entry_idx,
                                field_idx,
                            ));
                    } else {
                        self.mode = Mode::Edit(InlineEdit::new(value));
                    }
                }
            }

            Some(CursorTarget::AddButton(kind)) => match kind {
                AddButtonKind::Section => {
                    // Open section picker with valid sections AND top-level entries from schema
                    // We include both so users can add things like min_version at the top level
                    let mut items: Vec<PickerItem> = crate::schema::SCHEMA_SECTIONS
                        .iter()
                        .filter(|(name, _)| {
                            // Filter out sections that already exist
                            !self.doc.sections.iter().any(|s| s.name == *name)
                        })
                        .map(|(name, desc)| PickerItem::new(*name).with_description(*desc))
                        .collect();
                    // Also add top-level entries (like min_version, redactions)
                    // These get added at the file level, not as sections
                    let entry_items: Vec<PickerItem> = crate::schema::SCHEMA_ENTRIES
                        .iter()
                        .filter(|(name, _, _)| {
                            // Filter out entries that already exist in any section at root level
                            // (top-level entries are stored in a virtual "" section or handled specially)
                            !self.doc.sections.iter().any(|s| {
                                s.name.is_empty() && s.entries.iter().any(|e| e.key == *name)
                            })
                        })
                        .map(|(name, desc, _)| PickerItem::new(*name).with_description(*desc))
                        .collect();
                    items.extend(entry_items);
                    items.sort_by(|a, b| a.name.cmp(&b.name));
                    if items.is_empty() {
                        // All sections already exist, fall back to manual entry
                        self.mode = Mode::NewKey(InlineEdit::new(""));
                    } else {
                        let picker = PickerState::new(items).with_visible_height(10);
                        self.mode = Mode::Picker(PickerKind::Section, Box::new(picker));
                    }
                }
                AddButtonKind::Entry(_) => {
                    self.mode = Mode::NewKey(InlineEdit::new(""));
                }
                AddButtonKind::ToolRegistry(section_idx) => {
                    // Open tool picker
                    let tools = self.tool_provider.list_tools();
                    if tools.is_empty() {
                        // Fall back to manual entry if no tools available
                        self.mode = Mode::NewKey(InlineEdit::new(""));
                    } else {
                        let items: Vec<PickerItem> = tools
                            .into_iter()
                            .map(|t| {
                                let mut item = PickerItem::new(&t.name);
                                if let Some(desc) = t.description {
                                    item = item.with_description(desc);
                                }
                                item
                            })
                            .collect();
                        let picker = PickerState::new(items).with_visible_height(10);
                        self.mode = Mode::Picker(PickerKind::Tool(section_idx), Box::new(picker));
                    }
                }
                AddButtonKind::ToolBackend(section_idx) => {
                    // Open backend picker
                    let backends = self.backend_provider.list_backends();
                    if backends.is_empty() {
                        // Fall back to manual entry if no backends available
                        self.mode = Mode::NewKey(InlineEdit::new(""));
                    } else {
                        let items: Vec<PickerItem> = backends
                            .into_iter()
                            .map(|b| {
                                let mut item = PickerItem::new(&b.name);
                                if let Some(desc) = b.description {
                                    item = item.with_description(desc);
                                }
                                item
                            })
                            .collect();
                        let picker = PickerState::new(items).with_visible_height(10);
                        self.mode =
                            Mode::Picker(PickerKind::Backend(section_idx), Box::new(picker));
                    }
                }
                AddButtonKind::EnvPath(section_idx) => {
                    // Add _.path array entry
                    self.doc
                        .add_entry(section_idx, "_.path".to_string(), String::new());
                    // Convert to array and start editing
                    let entry_idx = self.doc.sections[section_idx].entries.len() - 1;
                    // Track undo for added entry
                    self.undo_stack
                        .push(UndoAction::AddEntry(section_idx, entry_idx));
                    self.doc.sections[section_idx].entries[entry_idx].value =
                        EntryValue::Array(vec!["./bin".to_string()]);
                    self.doc.sections[section_idx].entries[entry_idx].expanded = true;
                    self.doc.modified = true;
                    // Move cursor to the new entry
                    let target = CursorTarget::Entry(section_idx, entry_idx);
                    self.cursor.goto(&self.doc, &target);
                }
                AddButtonKind::EnvDotenv(_) => {
                    // Prompt for filename with ".env" as default
                    self.mode = Mode::NewKey(InlineEdit::new(".env"));
                }
                AddButtonKind::EnvSource(_) => {
                    // Prompt for script path
                    self.mode = Mode::NewKey(InlineEdit::new(""));
                }
                AddButtonKind::EnvVariable(_) => {
                    // Standard key=value flow
                    self.mode = Mode::NewKey(InlineEdit::new(""));
                }
                AddButtonKind::Task(_) => {
                    // Standard key=value flow for now
                    self.mode = Mode::NewKey(InlineEdit::new(""));
                }
                AddButtonKind::Prepare(_) => {
                    // Standard key=value flow for prepare providers
                    self.mode = Mode::NewKey(InlineEdit::new(""));
                }
                AddButtonKind::Setting(section_idx) => {
                    // Open setting picker with valid settings from schema
                    let existing_keys: std::collections::HashSet<_> = self.doc.sections
                        [section_idx]
                        .entries
                        .iter()
                        .map(|e| e.key.as_str())
                        .collect();
                    let items: Vec<PickerItem> = crate::schema::SCHEMA_SETTINGS
                        .iter()
                        .filter(|(name, _, _)| !existing_keys.contains(*name))
                        .map(|(name, desc, _)| PickerItem::new(*name).with_description(*desc))
                        .collect();
                    if items.is_empty() {
                        // All settings already exist, fall back to manual entry
                        self.mode = Mode::NewKey(InlineEdit::new(""));
                    } else {
                        let picker = PickerState::new(items).with_visible_height(10);
                        self.mode =
                            Mode::Picker(PickerKind::Setting(section_idx), Box::new(picker));
                    }
                }
                AddButtonKind::Hook(section_idx) => {
                    // Open hook picker with common hooks from schema
                    let existing_keys: std::collections::HashSet<_> = self.doc.sections
                        [section_idx]
                        .entries
                        .iter()
                        .map(|e| e.key.as_str())
                        .collect();
                    let items: Vec<PickerItem> = crate::schema::SCHEMA_HOOKS
                        .iter()
                        .filter(|(name, _)| !existing_keys.contains(*name))
                        .map(|(name, desc)| PickerItem::new(*name).with_description(*desc))
                        .collect();
                    if items.is_empty() {
                        // All common hooks already exist, fall back to manual entry
                        self.mode = Mode::NewKey(InlineEdit::new(""));
                    } else {
                        let picker = PickerState::new(items).with_visible_height(10);
                        self.mode = Mode::Picker(PickerKind::Hook(section_idx), Box::new(picker));
                    }
                }
                AddButtonKind::TaskConfig(section_idx) => {
                    // Open task_config picker with valid keys from schema
                    let existing_keys: std::collections::HashSet<_> = self.doc.sections
                        [section_idx]
                        .entries
                        .iter()
                        .map(|e| e.key.as_str())
                        .collect();
                    let items: Vec<PickerItem> = crate::schema::SCHEMA_TASK_CONFIG
                        .iter()
                        .filter(|(name, _, _)| !existing_keys.contains(*name))
                        .map(|(name, desc, _)| PickerItem::new(*name).with_description(*desc))
                        .collect();
                    if items.is_empty() {
                        // All task_config keys already exist, fall back to manual entry
                        self.mode = Mode::NewKey(InlineEdit::new(""));
                    } else {
                        let picker = PickerState::new(items).with_visible_height(10);
                        self.mode =
                            Mode::Picker(PickerKind::TaskConfig(section_idx), Box::new(picker));
                    }
                }
                AddButtonKind::Monorepo(section_idx) => {
                    // Open monorepo picker with valid keys from schema
                    let existing_keys: std::collections::HashSet<_> = self.doc.sections
                        [section_idx]
                        .entries
                        .iter()
                        .map(|e| e.key.as_str())
                        .collect();
                    let items: Vec<PickerItem> = crate::schema::SCHEMA_MONOREPO
                        .iter()
                        .filter(|(name, _, _)| !existing_keys.contains(*name))
                        .map(|(name, desc, _)| PickerItem::new(*name).with_description(*desc))
                        .collect();
                    if items.is_empty() {
                        // All monorepo keys already exist, fall back to manual entry
                        self.mode = Mode::NewKey(InlineEdit::new(""));
                    } else {
                        let picker = PickerState::new(items).with_visible_height(10);
                        self.mode =
                            Mode::Picker(PickerKind::Monorepo(section_idx), Box::new(picker));
                    }
                }
                AddButtonKind::ArrayItem(_, _) => {
                    // For arrays, go straight to value entry
                    self.mode = Mode::NewKey(InlineEdit::new(""));
                }
                AddButtonKind::InlineTableField(_, _) => {
                    self.mode = Mode::NewKey(InlineEdit::new(""));
                }
            },

            // Comments are not interactive
            Some(CursorTarget::Comment(_)) => {}

            None => {}
        }

        Ok(())
    }

    pub(super) fn handle_remove(&mut self) -> io::Result<()> {
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

    pub(super) fn undo(&mut self) {
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
            Some(CursorTarget::SectionHeader(section_idx)) => {
                if !self.doc.sections[section_idx].expanded {
                    self.doc.sections[section_idx].expanded = true;
                }
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
            Some(CursorTarget::SectionHeader(section_idx)) => {
                if self.doc.sections[section_idx].expanded {
                    self.doc.sections[section_idx].expanded = false;
                }
            }
            Some(CursorTarget::Entry(section_idx, entry_idx)) => {
                if self.doc.sections[section_idx].entries[entry_idx].expanded {
                    self.doc.sections[section_idx].entries[entry_idx].expanded = false;
                }
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

    pub(super) fn apply_edit(&mut self, value: String) {
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

    pub(super) fn apply_new_key(&mut self, key_name: String) {
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
            Some(CursorTarget::AddButton(AddButtonKind::Prepare(section_idx))) => {
                // Create prepare provider as inline table (e.g., npm = { disable = true })
                self.doc.sections[section_idx]
                    .entries
                    .push(crate::document::Entry {
                        key: key_name,
                        value: EntryValue::InlineTable(Vec::new()),
                        expanded: true,
                        comments: Vec::new(),
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
            Some(CursorTarget::AddButton(AddButtonKind::EnvDotenv(section_idx))) => {
                // key_name is the filename, add as mise.file = "<filename>"
                if !key_name.is_empty() {
                    self.doc
                        .add_entry(section_idx, "mise.file".to_string(), key_name);
                    // Track undo for added entry
                    let new_entry_idx = self.doc.sections[section_idx].entries.len() - 1;
                    self.undo_stack
                        .push(UndoAction::AddEntry(section_idx, new_entry_idx));
                }
            }
            Some(CursorTarget::AddButton(AddButtonKind::EnvSource(section_idx))) => {
                // key_name is the script path, add as _.source = "<script>"
                if !key_name.is_empty() {
                    self.doc
                        .add_entry(section_idx, "_.source".to_string(), key_name);
                    // Track undo for added entry
                    let new_entry_idx = self.doc.sections[section_idx].entries.len() - 1;
                    self.undo_stack
                        .push(UndoAction::AddEntry(section_idx, new_entry_idx));
                }
            }
            // ToolRegistry and EnvPath are handled in handle_enter directly
            Some(CursorTarget::AddButton(AddButtonKind::ToolRegistry(_)))
            | Some(CursorTarget::AddButton(AddButtonKind::EnvPath(_))) => {}
            Some(CursorTarget::AddButton(AddButtonKind::ToolBackend(section_idx))) => {
                // Add backend tool (e.g., cargo:ripgrep) with "latest" as default
                if !key_name.is_empty() {
                    self.doc
                        .add_entry(section_idx, key_name, "latest".to_string());
                    // Track undo for added entry
                    let new_entry_idx = self.doc.sections[section_idx].entries.len() - 1;
                    self.undo_stack
                        .push(UndoAction::AddEntry(section_idx, new_entry_idx));
                }
            }
            Some(CursorTarget::AddButton(AddButtonKind::Hook(section_idx))) => {
                // Add custom hook entry (e.g., custom_hook = "echo hello")
                if !key_name.is_empty() {
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
            }
            Some(CursorTarget::AddButton(AddButtonKind::TaskConfig(section_idx))) => {
                // Add custom task_config entry
                if !key_name.is_empty() {
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
            }
            Some(CursorTarget::AddButton(AddButtonKind::Monorepo(section_idx))) => {
                // Add custom monorepo entry
                if !key_name.is_empty() {
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

    /// Get a type-appropriate default value for a schema type.
    /// Returns (value, needs_edit) where needs_edit indicates if the user should be prompted.
    pub(super) fn type_appropriate_default(
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
