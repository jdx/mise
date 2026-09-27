use super::*;

impl InteractiveConfig {
    pub(in crate::editor) async fn handle_enter(&mut self) -> io::Result<()> {
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
                AddButtonKind::Deps(_) => {
                    // Standard key=value flow for deps providers
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
}
