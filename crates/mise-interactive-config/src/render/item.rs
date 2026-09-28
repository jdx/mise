use super::*;

impl Renderer {
    pub(super) fn render_item(
        &self,
        doc: &TomlDocument,
        target: &CursorTarget,
        is_cursor: bool,
        mode: &Mode,
        styles: &RenderStyles,
    ) -> String {
        let section_style = &styles.section;
        let key_style = &styles.key;
        let value_style = &styles.value;
        let cursor_style = &styles.cursor;
        let dim_style = &styles.dim;
        let add_style = &styles.add;

        match target {
            CursorTarget::Comment(text) => {
                // Comments are rendered in dim green style (not selectable)
                let comment_style = Style::new().dim().green();
                format!("{}", comment_style.apply_to(text))
            }

            CursorTarget::SectionHeader(section_idx) => {
                let section = &doc.sections[*section_idx];
                let arrow = if section.expanded { "▼" } else { "▶" };
                let count = if section.entries.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", section.entries.len())
                };
                // Display "(root)" for the root section (empty name)
                let section_label = if section.name.is_empty() {
                    "(root)".to_string()
                } else {
                    format!("[{}]", section.name)
                };
                let text = format!("{} {}{}", arrow, section_label, count);
                if is_cursor {
                    format!("{}", cursor_style.apply_to(&text))
                } else {
                    format!("{}", section_style.apply_to(&text))
                }
            }

            CursorTarget::Entry(section_idx, entry_idx) => {
                let entry = &doc.sections[*section_idx].entries[*entry_idx];

                // Check if we're editing this entry
                if is_cursor {
                    if let Mode::Edit(edit) = mode {
                        // Render with inline edit cursor
                        let key_part = format!("    {} = ", key_style.apply_to(&entry.key));
                        let cursor_pos = edit.cursor();
                        let buffer = edit.buffer();

                        // Split buffer at cursor position
                        let chars: Vec<char> = buffer.chars().collect();
                        let before: String = chars[..cursor_pos].iter().collect();
                        let at_cursor = chars.get(cursor_pos).copied().unwrap_or(' ');
                        let after: String = chars
                            .get(cursor_pos + 1..)
                            .map(|c| c.iter().collect())
                            .unwrap_or_default();

                        return format!(
                            "{}\"{}{}{}\"",
                            key_part,
                            value_style.apply_to(&before),
                            cursor_style.apply_to(at_cursor),
                            value_style.apply_to(&after)
                        );
                    }

                    // Check if we're renaming the key
                    if let Mode::RenameKey(s_idx, e_idx, edit) = mode
                        && *s_idx == *section_idx
                        && *e_idx == *entry_idx
                    {
                        let cursor_pos = edit.cursor();
                        let buffer = edit.buffer();
                        let chars: Vec<char> = buffer.chars().collect();
                        let before: String = chars[..cursor_pos].iter().collect();
                        let at_cursor = chars.get(cursor_pos).copied().unwrap_or(' ');
                        let after: String = chars
                            .get(cursor_pos + 1..)
                            .map(|c| c.iter().collect())
                            .unwrap_or_default();

                        let value_display = match &entry.value {
                            EntryValue::Simple(s) => format!("\"{}\"", s),
                            EntryValue::Array(_) => "[...]".to_string(),
                            EntryValue::InlineTable(_) => "{...}".to_string(),
                        };

                        return format!(
                            "    {}{}{} = {}",
                            key_style.apply_to(&before),
                            cursor_style.apply_to(at_cursor),
                            key_style.apply_to(&after),
                            value_style.apply_to(&value_display)
                        );
                    }

                    // Check if we're selecting a version
                    if let Mode::VersionSelect(vs) = mode
                        && vs.section_idx == *section_idx
                        && vs.entry_idx == *entry_idx
                    {
                        let key_part = format!("    {} = ", key_style.apply_to(&entry.key));
                        // Show all variants with current one highlighted
                        let variants_display: Vec<String> = vs
                            .variants
                            .iter()
                            .enumerate()
                            .map(|(i, v)| {
                                if i == vs.selected {
                                    format!("{}", cursor_style.apply_to(format!("[{}]", v)))
                                } else {
                                    format!("{}", dim_style.apply_to(v))
                                }
                            })
                            .collect();
                        return format!("{}{}", key_part, variants_display.join("  "));
                    }

                    // Check if we're selecting a boolean (for existing entries being edited)
                    if let Mode::BooleanSelect(bs) = mode
                        && let Some(editing_entry_idx) = bs.entry_idx
                        && bs.section_idx == *section_idx
                        && editing_entry_idx == *entry_idx
                    {
                        let key_part = format!("    {} = ", key_style.apply_to(&entry.key));
                        let true_display = if bs.selected {
                            format!("{}", cursor_style.apply_to("[true]"))
                        } else {
                            format!("{}", dim_style.apply_to("true"))
                        };
                        let false_display = if !bs.selected {
                            format!("{}", cursor_style.apply_to("[false]"))
                        } else {
                            format!("{}", dim_style.apply_to("false"))
                        };
                        return format!("{}{}  {}", key_part, true_display, false_display);
                    }
                }

                let value_display = match &entry.value {
                    EntryValue::Simple(s) => {
                        // Don't quote booleans
                        if s == "true" || s == "false" {
                            s.clone()
                        } else {
                            format!("\"{}\"", s)
                        }
                    }
                    EntryValue::Array(_) if entry.expanded => "▼".to_string(),
                    EntryValue::Array(items) => {
                        let preview: Vec<_> =
                            items.iter().take(3).map(|s| format!("\"{}\"", s)).collect();
                        let suffix = if items.len() > 3 { ", ..." } else { "" };
                        format!("▶ [{}{}]", preview.join(", "), suffix)
                    }
                    EntryValue::InlineTable(_) if entry.expanded => "▼".to_string(),
                    EntryValue::InlineTable(pairs) => {
                        let preview: Vec<_> = pairs
                            .iter()
                            .take(2)
                            .map(|(k, v)| {
                                // Don't quote booleans in inline table preview
                                if v == "true" || v == "false" {
                                    format!("{} = {}", k, v)
                                } else {
                                    format!("{} = \"{}\"", k, v)
                                }
                            })
                            .collect();
                        let suffix = if pairs.len() > 2 { ", ..." } else { "" };
                        format!("▶ {{ {}{} }}", preview.join(", "), suffix)
                    }
                };

                if is_cursor {
                    format!(
                        "  {} {} = {}",
                        cursor_style.apply_to(">"),
                        key_style.apply_to(&entry.key),
                        value_style.apply_to(&value_display)
                    )
                } else {
                    format!(
                        "    {} = {}",
                        key_style.apply_to(&entry.key),
                        value_style.apply_to(&value_display)
                    )
                }
            }

            CursorTarget::ArrayItem(section_idx, entry_idx, array_idx) => {
                let entry = &doc.sections[*section_idx].entries[*entry_idx];
                if let EntryValue::Array(items) = &entry.value {
                    let value = &items[*array_idx];

                    // Check if we're editing this item
                    if is_cursor && let Mode::Edit(edit) = mode {
                        let cursor_pos = edit.cursor();
                        let buffer = edit.buffer();
                        let chars: Vec<char> = buffer.chars().collect();
                        let before: String = chars[..cursor_pos].iter().collect();
                        let at_cursor = chars.get(cursor_pos).copied().unwrap_or(' ');
                        let after: String = chars
                            .get(cursor_pos + 1..)
                            .map(|c| c.iter().collect())
                            .unwrap_or_default();

                        return format!(
                            "        \"{}{}{}\"",
                            value_style.apply_to(&before),
                            cursor_style.apply_to(at_cursor),
                            value_style.apply_to(&after)
                        );
                    }

                    let text = format!("\"{}\"", value);
                    if is_cursor {
                        format!(
                            "      {} {}",
                            cursor_style.apply_to(">"),
                            value_style.apply_to(&text)
                        )
                    } else {
                        format!("        {}", dim_style.apply_to(&text))
                    }
                } else {
                    String::new()
                }
            }

            CursorTarget::InlineTableField(section_idx, entry_idx, field_idx) => {
                let entry = &doc.sections[*section_idx].entries[*entry_idx];
                if let EntryValue::InlineTable(pairs) = &entry.value {
                    let (key, value) = &pairs[*field_idx];
                    let is_boolean = value == "true" || value == "false";

                    // Check if we're editing this field with boolean selector
                    if is_cursor
                        && let Mode::BooleanSelect(bs) = mode
                        && let Some(f_idx) = bs.field_idx
                        && bs.section_idx == *section_idx
                        && bs.entry_idx == Some(*entry_idx)
                        && f_idx == *field_idx
                    {
                        let key_part = format!("        {} = ", key_style.apply_to(key));
                        let true_display = if bs.selected {
                            format!("{}", cursor_style.apply_to("[true]"))
                        } else {
                            format!("{}", dim_style.apply_to("true"))
                        };
                        let false_display = if !bs.selected {
                            format!("{}", cursor_style.apply_to("[false]"))
                        } else {
                            format!("{}", dim_style.apply_to("false"))
                        };
                        return format!("{}{}  {}", key_part, true_display, false_display);
                    }

                    // Check if we're editing this field with text editor
                    if is_cursor && let Mode::Edit(edit) = mode {
                        let prefix = format!("        {} = ", key_style.apply_to(key));
                        let cursor_pos = edit.cursor();
                        let buffer = edit.buffer();
                        let chars: Vec<char> = buffer.chars().collect();
                        let before: String = chars[..cursor_pos].iter().collect();
                        let at_cursor = chars.get(cursor_pos).copied().unwrap_or(' ');
                        let after: String = chars
                            .get(cursor_pos + 1..)
                            .map(|c| c.iter().collect())
                            .unwrap_or_default();

                        return format!(
                            "{}\"{}{}{}\"",
                            prefix,
                            value_style.apply_to(&before),
                            cursor_style.apply_to(at_cursor),
                            value_style.apply_to(&after)
                        );
                    }

                    // Display value - don't quote booleans
                    let value_display = if is_boolean {
                        value.clone()
                    } else {
                        format!("\"{}\"", value)
                    };
                    let text = format!("{} = {}", key, value_display);
                    if is_cursor {
                        format!(
                            "      {} {}",
                            cursor_style.apply_to(">"),
                            value_style.apply_to(&text)
                        )
                    } else {
                        format!(
                            "        {} = {}",
                            key_style.apply_to(key),
                            value_style.apply_to(&value_display)
                        )
                    }
                } else {
                    String::new()
                }
            }

            CursorTarget::AddButton(kind) => {
                let label = match kind {
                    AddButtonKind::Section => "[+ Add section]",
                    AddButtonKind::Entry(_) => "    [+ Add entry]",
                    AddButtonKind::ToolRegistry(_) => "    [+ Add tool from registry]",
                    AddButtonKind::ToolBackend(_) => "    [+ Add tool from backend]",
                    AddButtonKind::EnvPath(_) => "    [+ Add PATH]",
                    AddButtonKind::EnvDotenv(_) => "    [+ Load .env]",
                    AddButtonKind::EnvSource(_) => "    [+ Source script]",
                    AddButtonKind::EnvVariable(_) => "    [+ Add variable]",
                    AddButtonKind::Task(_) => "    [+ Add task]",
                    AddButtonKind::Deps(_) => "    [+ Add deps provider]",
                    AddButtonKind::Setting(_) => "    [+ Add setting]",
                    AddButtonKind::Hook(_) => "    [+ Add hook]",
                    AddButtonKind::TaskConfig(_) => "    [+ Add task config]",
                    AddButtonKind::Monorepo(_) => "    [+ Add monorepo config]",
                    AddButtonKind::ArrayItem(_, _) => "        [+ Add item]",
                    AddButtonKind::InlineTableField(_, _) => "        [+ Add field]",
                };

                // Check if we're entering a backend tool name (shows as "backend:_")
                if is_cursor && let Mode::BackendToolName(backend_name, _, edit) = mode {
                    let cursor_pos = edit.cursor();
                    let buffer = edit.buffer();
                    let chars: Vec<char> = buffer.chars().collect();
                    let before: String = chars[..cursor_pos].iter().collect();
                    let at_cursor = chars.get(cursor_pos).copied().unwrap_or(' ');
                    let after: String = chars
                        .get(cursor_pos + 1..)
                        .map(|c| c.iter().collect())
                        .unwrap_or_default();

                    return format!(
                        "    {}:{}{}{}",
                        key_style.apply_to(backend_name),
                        value_style.apply_to(&before),
                        cursor_style.apply_to(at_cursor),
                        value_style.apply_to(&after)
                    );
                }

                // Check if we're entering a new key
                if is_cursor && let Mode::NewKey(edit) = mode {
                    let prefix = match kind {
                        AddButtonKind::Entry(_)
                        | AddButtonKind::ToolRegistry(_)
                        | AddButtonKind::ToolBackend(_)
                        | AddButtonKind::EnvPath(_)
                        | AddButtonKind::EnvDotenv(_)
                        | AddButtonKind::EnvSource(_)
                        | AddButtonKind::EnvVariable(_)
                        | AddButtonKind::Task(_)
                        | AddButtonKind::Deps(_)
                        | AddButtonKind::Setting(_)
                        | AddButtonKind::Hook(_)
                        | AddButtonKind::TaskConfig(_)
                        | AddButtonKind::Monorepo(_) => "    ",
                        AddButtonKind::ArrayItem(_, _) | AddButtonKind::InlineTableField(_, _) => {
                            "        "
                        }
                        AddButtonKind::Section => "",
                    };
                    let prompt = match kind {
                        AddButtonKind::Section => "Section name: ",
                        AddButtonKind::Entry(_) => "Key: ",
                        AddButtonKind::ToolRegistry(_) => "Tool: ",
                        AddButtonKind::ToolBackend(_) => "Tool (e.g. cargo:ripgrep): ",
                        AddButtonKind::EnvPath(_) => "Path: ",
                        AddButtonKind::EnvDotenv(_) => "File: ",
                        AddButtonKind::EnvSource(_) => "Script: ",
                        AddButtonKind::EnvVariable(_) => "KEY=value: ",
                        AddButtonKind::Task(_) => "Task name: ",
                        AddButtonKind::Deps(_) => "Provider name: ",
                        AddButtonKind::Setting(_) => "Setting: ",
                        AddButtonKind::Hook(_) => "Hook name: ",
                        AddButtonKind::TaskConfig(_) => "Config key: ",
                        AddButtonKind::Monorepo(_) => "Config key: ",
                        AddButtonKind::ArrayItem(_, _) => "Value: ",
                        AddButtonKind::InlineTableField(_, _) => "Field name: ",
                    };
                    let cursor_pos = edit.cursor();
                    let buffer = edit.buffer();
                    let chars: Vec<char> = buffer.chars().collect();
                    let before: String = chars[..cursor_pos].iter().collect();
                    let at_cursor = chars.get(cursor_pos).copied().unwrap_or(' ');
                    let after: String = chars
                        .get(cursor_pos + 1..)
                        .map(|c| c.iter().collect())
                        .unwrap_or_default();

                    return format!(
                        "{}{}{}{}{}",
                        prefix,
                        dim_style.apply_to(prompt),
                        value_style.apply_to(&before),
                        cursor_style.apply_to(at_cursor),
                        value_style.apply_to(&after)
                    );
                }

                // Check if we're showing a boolean picker for a new entry
                if is_cursor
                    && let Mode::BooleanSelect(bs) = mode
                    && bs.entry_idx.is_none()
                {
                    // New entry - show key = [true] false or key = true [false]
                    let (true_display, false_display) = if bs.selected {
                        (
                            cursor_style.apply_to("[true]").to_string(),
                            dim_style.apply_to("false").to_string(),
                        )
                    } else {
                        (
                            dim_style.apply_to("true").to_string(),
                            cursor_style.apply_to("[false]").to_string(),
                        )
                    };
                    return format!(
                        "    {} = {}  {}",
                        key_style.apply_to(&bs.key),
                        true_display,
                        false_display
                    );
                }

                if is_cursor {
                    format!("{}", cursor_style.apply_to(label))
                } else {
                    format!("{}", add_style.apply_to(label))
                }
            }
        }
    }
}
