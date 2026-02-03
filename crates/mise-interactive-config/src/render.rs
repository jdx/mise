//! Render: Terminal output with colors and scrolling

use console::{Style, Term};
use std::io::{self, Write};

use crate::cursor::{AddButtonKind, Cursor, CursorTarget};
use crate::document::{EntryValue, TomlDocument};
use crate::inline_edit::InlineEdit;
use crate::picker::PickerState;

/// What kind of picker is currently active
#[derive(Debug, Clone)]
pub enum PickerKind {
    /// Picking a tool from registry to add
    Tool(usize), // section_idx
    /// Picking a backend type for a tool
    Backend(usize), // section_idx
    /// Picking a setting to add
    Setting(usize), // section_idx
    /// Picking a hook to add
    Hook(usize), // section_idx
    /// Picking a task_config key to add
    TaskConfig(usize), // section_idx
    /// Picking a monorepo key to add
    Monorepo(usize), // section_idx
    /// Picking a section to add
    Section,
}

/// State for version selection mode
#[derive(Debug, Clone)]
pub struct VersionSelectState {
    /// Tool name being edited
    #[allow(dead_code)]
    pub tool: String,
    /// Available version variants (e.g., ["latest", "3", "3.12", "3.12.4"])
    pub variants: Vec<String>,
    /// Currently selected variant index
    pub selected: usize,
    /// Section and entry indices
    pub section_idx: usize,
    pub entry_idx: usize,
}

impl VersionSelectState {
    /// Create a new version select state
    pub fn new(tool: String, variants: Vec<String>, section_idx: usize, entry_idx: usize) -> Self {
        Self {
            tool,
            variants,
            selected: 0,
            section_idx,
            entry_idx,
        }
    }

    /// Get the currently selected version
    pub fn current(&self) -> &str {
        &self.variants[self.selected]
    }

    /// Move to previous variant (more general)
    pub fn prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Move to next variant (more specific)
    pub fn next(&mut self) {
        if self.selected + 1 < self.variants.len() {
            self.selected += 1;
        }
    }
}

/// State for boolean selection mode
#[derive(Debug, Clone)]
pub struct BooleanSelectState {
    /// Key being set
    pub key: String,
    /// Currently selected value (true or false)
    pub selected: bool,
    /// Section index
    pub section_idx: usize,
    /// Entry index (if editing existing) or None (if adding new)
    pub entry_idx: Option<usize>,
    /// Field index for inline table fields (if editing a field within an entry)
    pub field_idx: Option<usize>,
}

impl BooleanSelectState {
    /// Create a new boolean select state for a new entry
    pub fn new_entry(key: String, section_idx: usize) -> Self {
        Self {
            key,
            selected: true, // Default to true
            section_idx,
            entry_idx: None,
            field_idx: None,
        }
    }

    /// Create a new boolean select state for editing existing entry
    pub fn edit_entry(key: String, current: bool, section_idx: usize, entry_idx: usize) -> Self {
        Self {
            key,
            selected: current,
            section_idx,
            entry_idx: Some(entry_idx),
            field_idx: None,
        }
    }

    /// Create a new boolean select state for editing an inline table field
    pub fn edit_inline_table_field(
        key: String,
        current: bool,
        section_idx: usize,
        entry_idx: usize,
        field_idx: usize,
    ) -> Self {
        Self {
            key,
            selected: current,
            section_idx,
            entry_idx: Some(entry_idx),
            field_idx: Some(field_idx),
        }
    }

    /// Toggle the selection
    pub fn toggle(&mut self) {
        self.selected = !self.selected;
    }

    /// Get the current selection as a string
    pub fn value_str(&self) -> &'static str {
        if self.selected { "true" } else { "false" }
    }
}

/// Editor mode
#[derive(Debug, Clone)]
pub enum Mode {
    /// Navigating the document
    Navigate,
    /// Editing a value inline
    Edit(InlineEdit),
    /// Entering a new key name
    NewKey(InlineEdit),
    /// Renaming a key (section_idx, entry_idx, edit)
    RenameKey(usize, usize, InlineEdit),
    /// Confirming quit with unsaved changes
    ConfirmQuit,
    /// Picking from a list (tool picker, setting picker)
    Picker(PickerKind, Box<PickerState>),
    /// Selecting a version for a tool (arrow left/right to cycle)
    VersionSelect(VersionSelectState),
    /// Entering a tool name after selecting a backend (backend_name, section_idx, edit)
    BackendToolName(String, usize, InlineEdit),
    /// Selecting a boolean value (true/false)
    BooleanSelect(BooleanSelectState),
    /// Loading indicator during async operations
    Loading(String),
}

/// Renderer for the interactive config editor
pub struct Renderer {
    term: Term,
    /// Number of lines rendered in the last frame
    last_rendered_lines: usize,
    /// Viewport scroll offset
    scroll_offset: usize,
    /// Visible height (terminal height minus header/footer)
    visible_height: usize,
}

impl Renderer {
    /// Create a new renderer
    pub fn new() -> Self {
        let term = Term::stderr();
        let (height, _) = term.size();
        Self {
            term,
            last_rendered_lines: 0,
            scroll_offset: 0,
            visible_height: height.saturating_sub(6) as usize, // Reserve for header/footer
        }
    }

    /// Get terminal reference
    pub fn term(&self) -> &Term {
        &self.term
    }

    /// Clear previously rendered lines
    pub fn clear(&mut self) -> io::Result<()> {
        if self.last_rendered_lines > 0 {
            self.term.clear_last_lines(self.last_rendered_lines)?;
        }
        self.last_rendered_lines = 0;
        Ok(())
    }

    /// Update viewport height
    pub fn update_size(&mut self) {
        let (height, _) = self.term.size();
        self.visible_height = height.saturating_sub(6) as usize;
    }

    /// Render the document
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        doc: &TomlDocument,
        cursor: &Cursor,
        mode: &Mode,
        title: &str,
        path: &str,
        dry_run: bool,
        can_undo: bool,
    ) -> io::Result<()> {
        self.clear()?;
        self.update_size();

        let mut output = Vec::new();

        // Styles
        let header_style = Style::new().cyan().bold();
        let section_style = Style::new().yellow().bold();
        let key_style = Style::new().green();
        let value_style = Style::new().white();
        let cursor_style = Style::new().reverse();
        let dim_style = Style::new().dim();
        let add_style = Style::new().blue();

        // Header
        let dry_run_str = if dry_run { " [dry-run]" } else { "" };
        output.push(format!("{}", header_style.apply_to(title)));
        output.push(format!(
            "{}{}",
            dim_style.apply_to(path),
            dim_style.apply_to(dry_run_str)
        ));
        output.push(String::new());

        // Build visible items
        let items = Cursor::build_visible_items(doc);
        let cursor_idx = cursor.index();

        // Adjust scroll offset to keep cursor visible
        if cursor_idx < self.scroll_offset {
            self.scroll_offset = cursor_idx;
        } else if cursor_idx >= self.scroll_offset + self.visible_height {
            self.scroll_offset = cursor_idx.saturating_sub(self.visible_height - 1);
        }

        // Render items
        let visible_start = self.scroll_offset;
        let visible_end = (self.scroll_offset + self.visible_height).min(items.len());

        for (idx, target) in items
            .iter()
            .enumerate()
            .skip(visible_start)
            .take(visible_end - visible_start)
        {
            let is_cursor = idx == cursor_idx;
            let line = self.render_item(
                doc,
                target,
                is_cursor,
                mode,
                &section_style,
                &key_style,
                &value_style,
                &cursor_style,
                &dim_style,
                &add_style,
            );
            output.push(line);
        }

        // Scroll indicators
        if self.scroll_offset > 0 {
            output.insert(3, format!("{}", dim_style.apply_to("  ↑ more above")));
        }
        if visible_end < items.len() {
            output.push(format!("{}", dim_style.apply_to("  ↓ more below")));
        }

        // Footer
        output.push(String::new());
        let footer: String = match mode {
            Mode::Navigate => {
                // Build context-sensitive footer based on cursor position
                let target = cursor.target(doc);

                // Determine Enter action based on target
                let enter_action = match &target {
                    Some(CursorTarget::SectionHeader(idx)) => {
                        if doc.sections[*idx].expanded {
                            "Enter collapse"
                        } else {
                            "Enter expand"
                        }
                    }
                    Some(CursorTarget::Entry(section_idx, entry_idx)) => {
                        let entry = &doc.sections[*section_idx].entries[*entry_idx];
                        match &entry.value {
                            EntryValue::Simple(_) => "Enter edit",
                            _ if entry.expanded => "Enter collapse",
                            _ => "Enter expand",
                        }
                    }
                    Some(CursorTarget::ArrayItem(_, _, _))
                    | Some(CursorTarget::InlineTableField(_, _, _)) => "Enter edit",
                    Some(CursorTarget::AddButton(_)) => "Enter add",
                    _ => "Enter",
                };

                // Check if "o options" is available (Entry with Simple value)
                let can_add_options = matches!(&target, Some(CursorTarget::Entry(section_idx, entry_idx))
                    if matches!(doc.sections[*section_idx].entries[*entry_idx].value, EntryValue::Simple(_)));

                // Check if "backspace remove" is available
                let can_remove = matches!(
                    &target,
                    Some(CursorTarget::Entry(_, _))
                        | Some(CursorTarget::ArrayItem(_, _, _))
                        | Some(CursorTarget::InlineTableField(_, _, _))
                        | Some(CursorTarget::SectionHeader(_))
                );

                // Check if "r rename" is available (Entry or InlineTableField)
                let can_rename = matches!(
                    &target,
                    Some(CursorTarget::Entry(_, _)) | Some(CursorTarget::InlineTableField(_, _, _))
                );

                let mut parts = vec!["↑/↓/←/→ navigate", enter_action];
                if can_add_options {
                    parts.push("o options");
                }
                if can_rename {
                    parts.push("r rename");
                }
                if can_remove {
                    parts.push("backspace remove");
                }
                if can_undo {
                    parts.push("u undo");
                }
                if !dry_run {
                    parts.push("s save");
                }
                parts.push(if dry_run { "q done" } else { "q quit" });
                parts.join(" • ")
            }
            Mode::Edit(_)
            | Mode::NewKey(_)
            | Mode::BackendToolName(_, _, _)
            | Mode::RenameKey(_, _, _) => "Enter confirm • Esc cancel • ←/→ cursor".to_string(),
            Mode::ConfirmQuit => "Unsaved changes. Save? y/n/Esc".to_string(),
            Mode::Picker(_, _) => {
                "Type to filter • ↑/↓ select • Enter add • Esc cancel".to_string()
            }
            Mode::VersionSelect(_) => "←/→ select version • Enter confirm • Esc cancel".to_string(),
            Mode::BooleanSelect(_) => "←/→ or t/f toggle • Enter confirm • Esc cancel".to_string(),
            Mode::Loading(_) => "Please wait...".to_string(),
        };
        output.push(format!("{}", dim_style.apply_to(&footer)));

        // Write output
        for line in &output {
            writeln!(self.term, "{}", line)?;
        }
        self.last_rendered_lines = output.len();

        self.term.flush()?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn render_item(
        &self,
        doc: &TomlDocument,
        target: &CursorTarget,
        is_cursor: bool,
        mode: &Mode,
        section_style: &Style,
        key_style: &Style,
        value_style: &Style,
        cursor_style: &Style,
        dim_style: &Style,
        add_style: &Style,
    ) -> String {
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
                    AddButtonKind::Prepare(_) => "    [+ Add prepare provider]",
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
                        | AddButtonKind::Prepare(_)
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
                        AddButtonKind::Prepare(_) => "Provider name: ",
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

    /// Show a message briefly
    pub fn flash_message(&mut self, message: &str) -> io::Result<()> {
        let style = Style::new().yellow().bold();
        writeln!(self.term, "{}", style.apply_to(message))?;
        self.term.flush()?;
        std::thread::sleep(std::time::Duration::from_millis(500));
        self.term.clear_last_lines(1)?;
        Ok(())
    }

    /// Render a loading indicator
    pub fn render_loading(&mut self, message: &str, title: &str, path: &str) -> io::Result<()> {
        self.clear()?;

        let mut output = Vec::new();

        // Styles
        let header_style = Style::new().cyan().bold();
        let dim_style = Style::new().dim();
        let loading_style = Style::new().yellow();

        // Header
        output.push(format!("{}", header_style.apply_to(title)));
        output.push(format!("{}", dim_style.apply_to(path)));
        output.push(String::new());

        // Loading message with spinner character
        output.push(format!("{} {}", loading_style.apply_to("⠋"), message));
        output.push(String::new());

        // Footer hint
        output.push(format!(
            "{}",
            dim_style.apply_to("Fetching version information...")
        ));

        // Write output
        for line in &output {
            writeln!(self.term, "{}", line)?;
        }
        self.last_rendered_lines = output.len();

        self.term.flush()?;
        Ok(())
    }

    /// Render the picker overlay
    pub fn render_picker(
        &mut self,
        picker: &PickerState,
        kind: &PickerKind,
        title: &str,
    ) -> io::Result<()> {
        self.clear()?;
        self.update_size();

        let mut output = Vec::new();

        // Styles
        let header_style = Style::new().cyan().bold();
        let cursor_style = Style::new().reverse();
        let dim_style = Style::new().dim();
        let name_style = Style::new().green();
        let desc_style = Style::new().white().dim();

        // Header with picker type
        let picker_title = match kind {
            PickerKind::Tool(_) => "Add Tool from Registry",
            PickerKind::Backend(_) => "Select Backend",
            PickerKind::Setting(_) => "Add Setting",
            PickerKind::Hook(_) => "Add Hook",
            PickerKind::TaskConfig(_) => "Add Task Config",
            PickerKind::Monorepo(_) => "Add Monorepo Config",
            PickerKind::Section => "Add Section",
        };
        output.push(format!("{}", header_style.apply_to(picker_title)));
        output.push(format!("{}", dim_style.apply_to(title)));
        output.push(String::new());

        // Filter input line
        let filter = picker.filter();
        let filter_display = if filter.is_empty() {
            format!(
                "{}{}",
                dim_style.apply_to("Filter: "),
                cursor_style.apply_to(" ")
            )
        } else {
            format!(
                "{}{}{}",
                dim_style.apply_to("Filter: "),
                filter,
                cursor_style.apply_to(" ")
            )
        };
        output.push(filter_display);
        output.push(String::new());

        // Scroll indicator above
        if picker.has_more_above() {
            output.push(format!("{}", dim_style.apply_to("  ↑ more above")));
        }

        // Render visible items
        for visible in picker.visible_items() {
            let name = &visible.item.name;
            let desc = visible.item.description.as_deref().unwrap_or("");

            // Truncate description if too long
            let (_, width) = self.term.size();
            let max_desc_len = width.saturating_sub(name.len() as u16 + 10) as usize;
            let truncated_desc = if desc.len() > max_desc_len && max_desc_len > 3 {
                format!("{}...", &desc[..max_desc_len.saturating_sub(3)])
            } else {
                desc.to_string()
            };

            let line = if visible.is_selected {
                format!(
                    "{} {} {}",
                    cursor_style.apply_to(">"),
                    name_style.apply_to(name),
                    desc_style.apply_to(&truncated_desc)
                )
            } else {
                format!(
                    "  {} {}",
                    name_style.apply_to(name),
                    desc_style.apply_to(&truncated_desc)
                )
            };
            output.push(line);
        }

        // Scroll indicator below
        if picker.has_more_below() {
            output.push(format!("{}", dim_style.apply_to("  ↓ more below")));
        }

        // Footer
        output.push(String::new());
        let footer = "Type to filter • ↑/↓ select • Enter add • Esc cancel";
        output.push(format!("{}", dim_style.apply_to(footer)));

        // Write output
        for line in &output {
            writeln!(self.term, "{}", line)?;
        }
        self.last_rendered_lines = output.len();

        self.term.flush()?;
        Ok(())
    }
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}
