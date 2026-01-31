//! Editor: Main event loop and key handling

use console::Key;
use std::io;
use std::path::PathBuf;

use crate::cursor::{AddButtonKind, Cursor, CursorTarget};
use crate::document::{Entry, EntryValue, Section, TomlDocument};
use crate::inline_edit::InlineEdit;
use crate::picker::{PickerItem, PickerState};
use crate::providers::{
    BackendProvider, EmptyBackendProvider, EmptyToolProvider, EmptyVersionProvider, ToolProvider,
    VersionProvider, version_variants,
};
use crate::render::{Mode, PickerKind, Renderer, VersionSelectState};

/// Result of running the interactive config editor
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigResult {
    /// User saved changes (includes document content as TOML string)
    Saved(String),
    /// User quit without saving
    Cancelled,
}

/// An action that can be undone
#[derive(Debug, Clone)]
enum UndoAction {
    /// Deleted an entry (section_idx, entry_idx, entry)
    DeleteEntry(usize, usize, Entry),
    /// Deleted an array item (section_idx, entry_idx, array_idx, value)
    DeleteArrayItem(usize, usize, usize, String),
    /// Deleted an inline table field (section_idx, entry_idx, field_idx, key, value)
    DeleteInlineTableField(usize, usize, usize, String, String),
    /// Deleted a section (section_idx, section)
    DeleteSection(usize, Section),
    /// Added an entry (section_idx, entry_idx)
    AddEntry(usize, usize),
    /// Added an array item (section_idx, entry_idx, array_idx)
    AddArrayItem(usize, usize, usize),
    /// Added an inline table field (section_idx, entry_idx, field_idx)
    AddInlineTableField(usize, usize, usize),
    /// Added a section (section_idx)
    AddSection(usize),
    /// Edited an entry value (section_idx, entry_idx, old_value)
    EditEntry(usize, usize, EntryValue),
    /// Edited an array item (section_idx, entry_idx, array_idx, old_value)
    EditArrayItem(usize, usize, usize, String),
    /// Edited an inline table field value (section_idx, entry_idx, field_idx, old_value)
    EditInlineTableField(usize, usize, usize, String),
    /// Renamed an entry key (section_idx, entry_idx, old_key)
    RenameEntry(usize, usize, String),
    /// Converted entry to inline table (section_idx, entry_idx, old_simple_value)
    ConvertToInlineTable(usize, usize, String),
}

/// Interactive TOML config editor
pub struct InteractiveConfig {
    path: PathBuf,
    doc: TomlDocument,
    cursor: Cursor,
    mode: Mode,
    dry_run: bool,
    title: String,
    renderer: Renderer,
    tool_provider: Box<dyn ToolProvider>,
    version_provider: Box<dyn VersionProvider>,
    backend_provider: Box<dyn BackendProvider>,
    /// Preferred version specificity index (0=latest, 1=major, 2=major.minor, 3=full, etc.)
    preferred_specificity: usize,
    /// Undo stack for deleted items
    undo_stack: Vec<UndoAction>,
}

impl InteractiveConfig {
    /// Create a new config editor for a new file with default sections
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            doc: TomlDocument::new(),
            cursor: Cursor::new(),
            mode: Mode::Navigate,
            dry_run: false,
            title: "mise config editor".to_string(),
            renderer: Renderer::new(),
            tool_provider: Box::new(EmptyToolProvider),
            version_provider: Box::new(EmptyVersionProvider),
            backend_provider: Box::new(EmptyBackendProvider),
            preferred_specificity: 0, // Default to "latest"
            undo_stack: Vec::new(),
        }
    }

    /// Open an existing config file for editing
    pub fn open(path: PathBuf) -> io::Result<Self> {
        let doc = if path.exists() {
            TomlDocument::load(&path)?
        } else {
            TomlDocument::new()
        };

        // Infer preferred specificity from last tool in the file
        let preferred_specificity = Self::infer_specificity_from_doc(&doc);

        Ok(Self {
            path,
            doc,
            cursor: Cursor::new(),
            mode: Mode::Navigate,
            dry_run: false,
            title: "mise config editor".to_string(),
            renderer: Renderer::new(),
            tool_provider: Box::new(EmptyToolProvider),
            version_provider: Box::new(EmptyVersionProvider),
            backend_provider: Box::new(EmptyBackendProvider),
            preferred_specificity,
            undo_stack: Vec::new(),
        })
    }

    /// Infer version specificity from the last tool in the document
    fn infer_specificity_from_doc(doc: &TomlDocument) -> usize {
        // Find the tools section
        let tools_section = doc.sections.iter().find(|s| s.name == "tools");
        if let Some(section) = tools_section {
            // Get the last entry's version
            if let Some(entry) = section.entries.last() {
                if let EntryValue::Simple(version) = &entry.value {
                    return Self::version_to_specificity(version);
                }
            }
        }
        0 // Default to "latest"
    }

    /// Convert a version string to a specificity index
    fn version_to_specificity(version: &str) -> usize {
        if version == "latest" {
            0
        } else {
            // Count dots to determine specificity
            // "22" -> 1 (major)
            // "22.13" -> 2 (major.minor)
            // "22.13.1" -> 3 (full)
            let dots = version.chars().filter(|c| *c == '.').count();
            dots + 1
        }
    }

    /// Set dry-run mode (no file writes)
    pub fn dry_run(mut self, dry_run: bool) -> Self {
        self.dry_run = dry_run;
        self
    }

    /// Set the title displayed in the header
    pub fn title(mut self, title: &str) -> Self {
        self.title = title.to_string();
        self
    }

    /// Set the tool provider for the tool picker
    pub fn with_tool_provider(mut self, provider: Box<dyn ToolProvider>) -> Self {
        self.tool_provider = provider;
        self
    }

    /// Set the version provider for version cycling
    pub fn with_version_provider(mut self, provider: Box<dyn VersionProvider>) -> Self {
        self.version_provider = provider;
        self
    }

    /// Set the backend provider for the backend picker
    pub fn with_backend_provider(mut self, provider: Box<dyn BackendProvider>) -> Self {
        self.backend_provider = provider;
        self
    }

    /// Add a tool to the tools section
    pub fn add_tool(&mut self, name: &str, version: &str) {
        if let Some(tools_idx) = self.doc.sections.iter().position(|s| s.name == "tools") {
            // Check if tool already exists
            if !self.doc.sections[tools_idx]
                .entries
                .iter()
                .any(|e| e.key == name)
            {
                self.doc
                    .add_entry(tools_idx, name.to_string(), version.to_string());
            }
        }
    }

    /// Add a prepare provider with auto = true
    pub fn add_prepare(&mut self, provider: &str) {
        // Find or create prepare section
        let prepare_idx =
            if let Some(idx) = self.doc.sections.iter().position(|s| s.name == "prepare") {
                idx
            } else {
                // Insert prepare section before settings
                let settings_idx = self.doc.sections.iter().position(|s| s.name == "settings");
                let insert_idx = settings_idx.unwrap_or(self.doc.sections.len());
                self.doc.sections.insert(
                    insert_idx,
                    crate::document::Section {
                        name: "prepare".to_string(),
                        entries: Vec::new(),
                        expanded: false,
                        comments: Vec::new(),
                    },
                );
                insert_idx
            };

        // Check if provider already exists
        if !self.doc.sections[prepare_idx]
            .entries
            .iter()
            .any(|e| e.key == provider)
        {
            // Add as inline table with auto = true
            self.doc.sections[prepare_idx]
                .entries
                .push(crate::document::Entry {
                    key: provider.to_string(),
                    value: EntryValue::InlineTable(vec![("auto".to_string(), "true".to_string())]),
                    expanded: false,
                    comments: Vec::new(),
                });
            self.doc.modified = true;
        }
    }

    /// Run the interactive editor
    pub async fn run(mut self) -> io::Result<ConfigResult> {
        // Absolutize the path for display
        let abs_path = if self.path.is_relative() {
            std::env::current_dir()
                .map(|cwd| cwd.join(&self.path))
                .unwrap_or_else(|_| self.path.clone())
        } else {
            self.path.clone()
        };
        let path_str = xx::file::display_path(&abs_path);

        // Initial render
        self.render_current_mode(&path_str)?;

        loop {
            // Read key in blocking task to not block the async runtime
            let term = self.renderer.term().clone();
            let key = tokio::task::spawn_blocking(move || term.read_key())
                .await
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))??;

            let should_exit = self.handle_key(key).await?;
            if let Some(result) = should_exit {
                // Clear the display before returning
                self.renderer.clear()?;
                return Ok(result);
            }

            // Re-render
            self.render_current_mode(&path_str)?;
        }
    }

    /// Render based on current mode
    fn render_current_mode(&mut self, path_str: &str) -> io::Result<()> {
        match &self.mode {
            Mode::Picker(kind, picker) => {
                self.renderer.render_picker(picker, kind, path_str)?;
            }
            _ => {
                self.renderer.render(
                    &self.doc,
                    &self.cursor,
                    &self.mode,
                    &self.title,
                    path_str,
                    self.dry_run,
                    !self.undo_stack.is_empty(),
                )?;
            }
        }
        Ok(())
    }

    /// Handle a key press. Returns Some(result) if the editor should exit.
    async fn handle_key(&mut self, key: Key) -> io::Result<Option<ConfigResult>> {
        match &mut self.mode {
            Mode::Navigate => self.handle_navigate_key(key).await,
            Mode::Edit(_) => self.handle_edit_key(key),
            Mode::NewKey(_) => self.handle_new_key_key(key),
            Mode::ConfirmQuit => self.handle_confirm_quit_key(key),
            Mode::RenameKey(_, _, _) => self.handle_rename_key(key),
            Mode::Picker(_, _) => self.handle_picker_key(key).await,
            Mode::VersionSelect(_) => self.handle_version_select_key(key),
            Mode::BackendToolName(_, _, _) => self.handle_backend_tool_name_key(key),
        }
    }

    async fn handle_navigate_key(&mut self, key: Key) -> io::Result<Option<ConfigResult>> {
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

    fn handle_edit_key(&mut self, key: Key) -> io::Result<Option<ConfigResult>> {
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

    fn handle_new_key_key(&mut self, key: Key) -> io::Result<Option<ConfigResult>> {
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

    fn handle_confirm_quit_key(&mut self, key: Key) -> io::Result<Option<ConfigResult>> {
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

    async fn handle_picker_key(&mut self, key: Key) -> io::Result<Option<ConfigResult>> {
        // We need to take ownership of the mode to modify the picker
        let mode = std::mem::replace(&mut self.mode, Mode::Navigate);

        if let Mode::Picker(kind, mut picker) = mode {
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
                                // Add the selected tool with default version
                                self.doc.add_entry(
                                    *section_idx,
                                    tool_name.clone(),
                                    "latest".to_string(),
                                );
                                // Move cursor to the new entry
                                let entry_idx = self.doc.sections[*section_idx].entries.len() - 1;
                                // Track undo for added entry
                                self.undo_stack
                                    .push(UndoAction::AddEntry(*section_idx, entry_idx));
                                let target = CursorTarget::Entry(*section_idx, entry_idx);
                                self.cursor.goto(&self.doc, &target);

                                // Try to use version selector if we have version info
                                if let Some(latest) =
                                    self.version_provider.latest_version(&tool_name).await
                                {
                                    let variants = version_variants(&latest);
                                    let mut vs = VersionSelectState::new(
                                        tool_name,
                                        variants.clone(),
                                        *section_idx,
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
                                // Add the selected setting with empty value
                                self.doc.add_entry(*section_idx, tool_name, String::new());
                                let entry_idx = self.doc.sections[*section_idx].entries.len() - 1;
                                // Track undo for added entry
                                self.undo_stack
                                    .push(UndoAction::AddEntry(*section_idx, entry_idx));
                                let target = CursorTarget::Entry(*section_idx, entry_idx);
                                self.cursor.goto(&self.doc, &target);
                                self.mode = Mode::Edit(InlineEdit::new(""));
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
                                // Add the selected task_config key with empty value
                                self.doc.add_entry(*section_idx, tool_name, String::new());
                                let entry_idx = self.doc.sections[*section_idx].entries.len() - 1;
                                // Track undo for added entry
                                self.undo_stack
                                    .push(UndoAction::AddEntry(*section_idx, entry_idx));
                                let target = CursorTarget::Entry(*section_idx, entry_idx);
                                self.cursor.goto(&self.doc, &target);
                                self.mode = Mode::Edit(InlineEdit::new(""));
                            }
                            PickerKind::Monorepo(section_idx) => {
                                // Add the selected monorepo key with empty value
                                self.doc.add_entry(*section_idx, tool_name, String::new());
                                let entry_idx = self.doc.sections[*section_idx].entries.len() - 1;
                                // Track undo for added entry
                                self.undo_stack
                                    .push(UndoAction::AddEntry(*section_idx, entry_idx));
                                let target = CursorTarget::Entry(*section_idx, entry_idx);
                                self.cursor.goto(&self.doc, &target);
                                self.mode = Mode::Edit(InlineEdit::new(""));
                            }
                            PickerKind::Section => {
                                // Add the selected section
                                self.doc.add_section(tool_name.clone());
                                // Find and move cursor to the new section
                                if let Some(idx) =
                                    self.doc.sections.iter().position(|s| s.name == tool_name)
                                {
                                    // Track undo for added section
                                    self.undo_stack.push(UndoAction::AddSection(idx));
                                    let target = CursorTarget::SectionHeader(idx);
                                    self.cursor.goto(&self.doc, &target);
                                }
                                self.mode = Mode::Navigate;
                            }
                        }
                    } else {
                        // No selection, return to navigate
                        self.mode = Mode::Navigate;
                    }
                }
                Key::ArrowUp | Key::Char('k') => {
                    picker.move_up();
                    self.mode = Mode::Picker(kind, picker);
                }
                Key::ArrowDown | Key::Char('j') => {
                    picker.move_down();
                    self.mode = Mode::Picker(kind, picker);
                }
                Key::Backspace => {
                    picker.backspace();
                    self.mode = Mode::Picker(kind, picker);
                }
                Key::Char(c) => {
                    picker.type_char(c);
                    self.mode = Mode::Picker(kind, picker);
                }
                _ => {
                    // Keep current state for unhandled keys
                    self.mode = Mode::Picker(kind, picker);
                }
            }
        }
        Ok(None)
    }

    fn handle_backend_tool_name_key(&mut self, key: Key) -> io::Result<Option<ConfigResult>> {
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

    fn handle_version_select_key(&mut self, key: Key) -> io::Result<Option<ConfigResult>> {
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

    async fn handle_enter(&mut self) -> io::Result<()> {
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

                match &entry.value {
                    EntryValue::Simple(current_value) => {
                        // For tools section, try to use version selector
                        if section_name == "tools" {
                            if let Some(latest) =
                                self.version_provider.latest_version(&tool_name).await
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
                                if let Some(pos) = variants.iter().position(|v| v == current_value)
                                {
                                    vs.selected = pos;
                                } else {
                                    // Current value is custom, select "other..."
                                    vs.selected = variants.len().saturating_sub(1);
                                }
                                self.mode = Mode::VersionSelect(vs);
                            } else {
                                // Fall back to inline edit if no version info available
                                self.mode = Mode::Edit(InlineEdit::new(current_value));
                            }
                        } else {
                            // Non-tools section: use regular inline edit
                            self.mode = Mode::Edit(InlineEdit::new(current_value));
                        }
                    }
                    EntryValue::Array(_) | EntryValue::InlineTable(_) => {
                        // Toggle expansion
                        self.doc.toggle_entry(section_idx, entry_idx);
                    }
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
                    let (_, value) = &pairs[field_idx];
                    self.mode = Mode::Edit(InlineEdit::new(value));
                }
            }

            Some(CursorTarget::AddButton(kind)) => match kind {
                AddButtonKind::Section => {
                    // Open section picker with valid sections from schema
                    let items: Vec<PickerItem> = crate::schema::SCHEMA_SECTIONS
                        .iter()
                        .filter(|(name, _)| {
                            // Filter out sections that already exist
                            !self.doc.sections.iter().any(|s| s.name == *name)
                        })
                        .map(|(name, desc)| PickerItem::new(*name).with_description(*desc))
                        .collect();
                    if items.is_empty() {
                        // All sections already exist, fall back to manual entry
                        self.mode = Mode::NewKey(InlineEdit::new(""));
                    } else {
                        let picker = PickerState::new(items).with_visible_height(10);
                        self.mode = Mode::Picker(PickerKind::Section, picker);
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
                        self.mode = Mode::Picker(PickerKind::Tool(section_idx), picker);
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
                        self.mode = Mode::Picker(PickerKind::Backend(section_idx), picker);
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
                        .filter(|(name, _)| !existing_keys.contains(*name))
                        .map(|(name, desc)| PickerItem::new(*name).with_description(*desc))
                        .collect();
                    if items.is_empty() {
                        // All settings already exist, fall back to manual entry
                        self.mode = Mode::NewKey(InlineEdit::new(""));
                    } else {
                        let picker = PickerState::new(items).with_visible_height(10);
                        self.mode = Mode::Picker(PickerKind::Setting(section_idx), picker);
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
                        self.mode = Mode::Picker(PickerKind::Hook(section_idx), picker);
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
                        .filter(|(name, _)| !existing_keys.contains(*name))
                        .map(|(name, desc)| PickerItem::new(*name).with_description(*desc))
                        .collect();
                    if items.is_empty() {
                        // All task_config keys already exist, fall back to manual entry
                        self.mode = Mode::NewKey(InlineEdit::new(""));
                    } else {
                        let picker = PickerState::new(items).with_visible_height(10);
                        self.mode = Mode::Picker(PickerKind::TaskConfig(section_idx), picker);
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
                        .filter(|(name, _)| !existing_keys.contains(*name))
                        .map(|(name, desc)| PickerItem::new(*name).with_description(*desc))
                        .collect();
                    if items.is_empty() {
                        // All monorepo keys already exist, fall back to manual entry
                        self.mode = Mode::NewKey(InlineEdit::new(""));
                    } else {
                        let picker = PickerState::new(items).with_visible_height(10);
                        self.mode = Mode::Picker(PickerKind::Monorepo(section_idx), picker);
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

    fn handle_remove(&mut self) -> io::Result<()> {
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
                if let Some(section) = self.doc.sections.get_mut(section_idx) {
                    if let Some(entry) = section.entries.get_mut(entry_idx) {
                        if let EntryValue::InlineTable(ref mut pairs) = entry.value {
                            if field_idx < pairs.len() {
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
                        }
                    }
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

    fn undo(&mut self) {
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
                    if let Some(section) = self.doc.sections.get_mut(section_idx) {
                        if let Some(entry) = section.entries.get_mut(entry_idx) {
                            if let EntryValue::Array(ref mut items) = entry.value {
                                let insert_idx = array_idx.min(items.len());
                                items.insert(insert_idx, value);
                                self.doc.modified = true;
                                let target =
                                    CursorTarget::ArrayItem(section_idx, entry_idx, insert_idx);
                                self.cursor.goto(&self.doc, &target);
                            }
                        }
                    }
                }
                UndoAction::DeleteInlineTableField(
                    section_idx,
                    entry_idx,
                    field_idx,
                    key,
                    value,
                ) => {
                    if let Some(section) = self.doc.sections.get_mut(section_idx) {
                        if let Some(entry) = section.entries.get_mut(entry_idx) {
                            if let EntryValue::InlineTable(ref mut pairs) = entry.value {
                                let insert_idx = field_idx.min(pairs.len());
                                pairs.insert(insert_idx, (key, value));
                                self.doc.modified = true;
                                let target = CursorTarget::InlineTableField(
                                    section_idx,
                                    entry_idx,
                                    insert_idx,
                                );
                                self.cursor.goto(&self.doc, &target);
                            }
                        }
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
                    if let Some(section) = self.doc.sections.get_mut(section_idx) {
                        if let Some(entry) = section.entries.get_mut(entry_idx) {
                            if let EntryValue::Array(ref mut items) = entry.value {
                                if array_idx < items.len() {
                                    items.remove(array_idx);
                                    self.doc.modified = true;
                                    self.cursor.clamp(&self.doc);
                                }
                            }
                        }
                    }
                }
                UndoAction::AddInlineTableField(section_idx, entry_idx, field_idx) => {
                    if let Some(section) = self.doc.sections.get_mut(section_idx) {
                        if let Some(entry) = section.entries.get_mut(entry_idx) {
                            if let EntryValue::InlineTable(ref mut pairs) = entry.value {
                                if field_idx < pairs.len() {
                                    pairs.remove(field_idx);
                                    self.doc.modified = true;
                                    self.cursor.clamp(&self.doc);
                                }
                            }
                        }
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
                    if let Some(section) = self.doc.sections.get_mut(section_idx) {
                        if let Some(entry) = section.entries.get_mut(entry_idx) {
                            entry.value = old_value;
                            self.doc.modified = true;
                            let target = CursorTarget::Entry(section_idx, entry_idx);
                            self.cursor.goto(&self.doc, &target);
                        }
                    }
                }
                UndoAction::EditArrayItem(section_idx, entry_idx, array_idx, old_value) => {
                    if let Some(section) = self.doc.sections.get_mut(section_idx) {
                        if let Some(entry) = section.entries.get_mut(entry_idx) {
                            if let EntryValue::Array(ref mut items) = entry.value {
                                if array_idx < items.len() {
                                    items[array_idx] = old_value;
                                    self.doc.modified = true;
                                    let target =
                                        CursorTarget::ArrayItem(section_idx, entry_idx, array_idx);
                                    self.cursor.goto(&self.doc, &target);
                                }
                            }
                        }
                    }
                }
                UndoAction::EditInlineTableField(section_idx, entry_idx, field_idx, old_value) => {
                    if let Some(section) = self.doc.sections.get_mut(section_idx) {
                        if let Some(entry) = section.entries.get_mut(entry_idx) {
                            if let EntryValue::InlineTable(ref mut pairs) = entry.value {
                                if field_idx < pairs.len() {
                                    pairs[field_idx].1 = old_value;
                                    self.doc.modified = true;
                                    let target = CursorTarget::InlineTableField(
                                        section_idx,
                                        entry_idx,
                                        field_idx,
                                    );
                                    self.cursor.goto(&self.doc, &target);
                                }
                            }
                        }
                    }
                }
                UndoAction::RenameEntry(section_idx, entry_idx, old_key) => {
                    if let Some(section) = self.doc.sections.get_mut(section_idx) {
                        if let Some(entry) = section.entries.get_mut(entry_idx) {
                            entry.key = old_key;
                            self.doc.modified = true;
                            let target = CursorTarget::Entry(section_idx, entry_idx);
                            self.cursor.goto(&self.doc, &target);
                        }
                    }
                }
                UndoAction::ConvertToInlineTable(section_idx, entry_idx, old_value) => {
                    if let Some(section) = self.doc.sections.get_mut(section_idx) {
                        if let Some(entry) = section.entries.get_mut(entry_idx) {
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

    fn handle_add_options(&mut self) -> io::Result<()> {
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

    fn handle_rename(&mut self) -> io::Result<()> {
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

    fn handle_rename_key(&mut self, key: Key) -> io::Result<Option<ConfigResult>> {
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
                        if let Some(section) = self.doc.sections.get_mut(section_idx) {
                            if let Some(entry) = section.entries.get_mut(entry_idx) {
                                if entry.key != new_key {
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

    fn handle_expand(&mut self) {
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

    fn handle_collapse(&mut self) {
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

    fn apply_edit(&mut self, value: String) {
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
                if let Some(section) = self.doc.sections.get_mut(section_idx) {
                    if let Some(entry) = section.entries.get_mut(entry_idx) {
                        if let EntryValue::InlineTable(ref mut pairs) = entry.value {
                            if let Some((_, v)) = pairs.get_mut(field_idx) {
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
                    }
                }
            }
            _ => {}
        }
    }

    fn apply_new_key(&mut self, key_name: String) {
        let target = self.cursor.target(&self.doc);

        match target {
            Some(CursorTarget::AddButton(AddButtonKind::Section)) => {
                self.doc.add_section(key_name);
                // Track undo for added section
                let section_idx = self.doc.sections.len() - 1;
                self.undo_stack.push(UndoAction::AddSection(section_idx));
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
            Some(CursorTarget::AddButton(AddButtonKind::EnvVariable(section_idx))) => {
                // Parse KEY=value format
                if let Some((key, value)) = key_name.split_once('=') {
                    let key = key.trim().to_string();
                    let value = value.trim();
                    // Strip surrounding quotes (single or double)
                    let value = if (value.starts_with('"') && value.ends_with('"'))
                        || (value.starts_with('\'') && value.ends_with('\''))
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
                if let Some(section) = self.doc.sections.get_mut(section_idx) {
                    if let Some(entry) = section.entries.get_mut(entry_idx) {
                        if let EntryValue::InlineTable(ref mut pairs) = entry.value {
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
                    }
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
            _ => {}
        }
    }

    fn save(&mut self) -> io::Result<()> {
        if self.dry_run {
            self.renderer.flash_message("Dry-run mode: not saving")?;
        } else {
            self.doc.save(&self.path)?;
            self.doc.modified = false;
            self.renderer.flash_message("Saved")?;
        }
        Ok(())
    }
}
