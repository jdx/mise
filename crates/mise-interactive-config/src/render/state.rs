use crate::inline_edit::InlineEdit;
use crate::picker::PickerState;

/// What kind of picker is currently active
#[derive(Debug, Clone)]
pub(crate) enum PickerKind {
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
pub(crate) struct VersionSelectState {
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
    pub(crate) fn new(
        tool: String,
        variants: Vec<String>,
        section_idx: usize,
        entry_idx: usize,
    ) -> Self {
        Self {
            tool,
            variants,
            selected: 0,
            section_idx,
            entry_idx,
        }
    }

    /// Get the currently selected version
    pub(crate) fn current(&self) -> &str {
        &self.variants[self.selected]
    }

    /// Move to previous variant (more general)
    pub(crate) fn prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Move to next variant (more specific)
    pub(crate) fn next(&mut self) {
        if self.selected + 1 < self.variants.len() {
            self.selected += 1;
        }
    }
}

/// State for boolean selection mode
#[derive(Debug, Clone)]
pub(crate) struct BooleanSelectState {
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
    pub(crate) fn new_entry(key: String, section_idx: usize) -> Self {
        Self {
            key,
            selected: true, // Default to true
            section_idx,
            entry_idx: None,
            field_idx: None,
        }
    }

    /// Create a new boolean select state for editing existing entry
    pub(crate) fn edit_entry(
        key: String,
        current: bool,
        section_idx: usize,
        entry_idx: usize,
    ) -> Self {
        Self {
            key,
            selected: current,
            section_idx,
            entry_idx: Some(entry_idx),
            field_idx: None,
        }
    }

    /// Create a new boolean select state for editing an inline table field
    pub(crate) fn edit_inline_table_field(
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
    pub(crate) fn toggle(&mut self) {
        self.selected = !self.selected;
    }

    /// Get the current selection as a string
    pub(crate) fn value_str(&self) -> &'static str {
        if self.selected { "true" } else { "false" }
    }
}

/// Editor mode
#[derive(Debug, Clone)]
pub(crate) enum Mode {
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
