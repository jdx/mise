//! Render: Terminal output with colors and scrolling

mod item;
mod state;

pub(crate) use state::{BooleanSelectState, Mode, PickerKind, VersionSelectState};

use console::{Style, Term};
use std::io::{self, Write};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::cursor::{AddButtonKind, Cursor, CursorTarget};
use crate::document::{EntryValue, TomlDocument};
use crate::picker::PickerState;

fn truncate_picker_description(name: &str, description: &str, terminal_width: u16) -> String {
    const PADDING_WIDTH: usize = 10;
    const ELLIPSIS: &str = "...";

    let available_width =
        usize::from(terminal_width).saturating_sub(name.width().saturating_add(PADDING_WIDTH));
    if description.width() <= available_width {
        return description.to_owned();
    }
    if available_width <= ELLIPSIS.len() {
        return ".".repeat(available_width);
    }

    let content_width = available_width - ELLIPSIS.len();
    let mut used_width: usize = 0;
    let mut end = 0;
    for (index, character) in description.char_indices() {
        let character_width = character.width().unwrap_or(0);
        if used_width.saturating_add(character_width) > content_width {
            break;
        }
        used_width += character_width;
        end = index + character.len_utf8();
    }
    format!("{}{ELLIPSIS}", &description[..end])
}

pub(crate) struct RenderOptions<'a> {
    pub(crate) title: &'a str,
    pub(crate) path: &'a str,
    pub(crate) dry_run: bool,
    pub(crate) can_undo: bool,
}

struct RenderStyles {
    header: Style,
    section: Style,
    key: Style,
    value: Style,
    cursor: Style,
    dim: Style,
    add: Style,
}

impl RenderStyles {
    fn new() -> Self {
        Self {
            header: Style::new().cyan().bold(),
            section: Style::new().yellow().bold(),
            key: Style::new().green(),
            value: Style::new().white(),
            cursor: Style::new().reverse(),
            dim: Style::new().dim(),
            add: Style::new().blue(),
        }
    }
}

/// Renderer for the interactive config editor
pub(crate) struct Renderer {
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
    pub(crate) fn new() -> Self {
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
    pub(crate) fn term(&self) -> &Term {
        &self.term
    }

    /// Clear previously rendered lines
    pub(crate) fn clear(&mut self) -> io::Result<()> {
        if self.last_rendered_lines > 0 {
            self.term.clear_last_lines(self.last_rendered_lines)?;
        }
        self.last_rendered_lines = 0;
        Ok(())
    }

    /// Update viewport height
    pub(crate) fn update_size(&mut self) {
        let (height, _) = self.term.size();
        self.visible_height = height.saturating_sub(6) as usize;
    }

    /// Render the document
    pub(crate) fn render(
        &mut self,
        doc: &TomlDocument,
        cursor: &Cursor,
        mode: &Mode,
        options: RenderOptions<'_>,
    ) -> io::Result<()> {
        self.clear()?;
        self.update_size();

        let mut output = Vec::new();

        let styles = RenderStyles::new();

        // Header
        let dry_run_str = if options.dry_run { " [dry-run]" } else { "" };
        output.push(format!("{}", styles.header.apply_to(options.title)));
        output.push(format!(
            "{}{}",
            styles.dim.apply_to(options.path),
            styles.dim.apply_to(dry_run_str)
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
            let line = self.render_item(doc, target, is_cursor, mode, &styles);
            output.push(line);
        }

        // Scroll indicators
        if self.scroll_offset > 0 {
            output.insert(3, format!("{}", styles.dim.apply_to("  ↑ more above")));
        }
        if visible_end < items.len() {
            output.push(format!("{}", styles.dim.apply_to("  ↓ more below")));
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
                if options.can_undo {
                    parts.push("u undo");
                }
                if !options.dry_run {
                    parts.push("s save");
                }
                parts.push(if options.dry_run { "q done" } else { "q quit" });
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
        output.push(format!("{}", styles.dim.apply_to(&footer)));

        // Write output
        for line in &output {
            writeln!(self.term, "{}", line)?;
        }
        self.last_rendered_lines = output.len();

        self.term.flush()?;
        Ok(())
    }

    /// Show a message briefly
    pub(crate) fn flash_message(&mut self, message: &str) -> io::Result<()> {
        let style = Style::new().yellow().bold();
        writeln!(self.term, "{}", style.apply_to(message))?;
        self.term.flush()?;
        std::thread::sleep(std::time::Duration::from_millis(500));
        self.term.clear_last_lines(1)?;
        Ok(())
    }

    /// Render a loading indicator
    pub(crate) fn render_loading(
        &mut self,
        message: &str,
        title: &str,
        path: &str,
    ) -> io::Result<()> {
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
    pub(crate) fn render_picker(
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
            let truncated_desc = truncate_picker_description(name, desc, width);

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

#[cfg(test)]
mod tests {
    use super::truncate_picker_description;

    #[test]
    fn truncates_picker_descriptions_at_character_boundaries() {
        assert_eq!(
            truncate_picker_description("tool", "café au lait", 21),
            "café..."
        );
        assert_eq!(
            truncate_picker_description("tool", "日本語 description", 23),
            "日本語..."
        );
    }

    #[test]
    fn picker_description_truncation_handles_narrow_terminals() {
        assert_eq!(
            truncate_picker_description("a very long name", "description", 10),
            ""
        );
        assert_eq!(
            truncate_picker_description("tool", "description", 17),
            "..."
        );
    }

    #[test]
    fn picker_description_truncation_preserves_text_that_fits() {
        assert_eq!(
            truncate_picker_description("tool", "short description", 40),
            "short description"
        );
    }
}
