//! Interactive TOML config editor
//!
//! This module provides the main editor interface for interactively editing
//! mise configuration files.

mod actions;
mod handlers;
mod undo;

use std::io;
use std::path::PathBuf;

use crate::cursor::Cursor;
use crate::document::{EntryValue, TomlDocument};
use crate::providers::{
    BackendProvider, EmptyBackendProvider, EmptyToolProvider, EmptyVersionProvider, ToolProvider,
    VersionProvider,
};
use crate::render::{Mode, Renderer};

use undo::UndoAction;

/// Result of running the interactive config editor
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigResult {
    /// User saved changes (includes document content as TOML string)
    Saved(String),
    /// User quit without saving
    Cancelled,
}

/// Interactive TOML config editor
pub struct InteractiveConfig {
    path: PathBuf,
    pub(crate) doc: TomlDocument,
    pub(crate) cursor: Cursor,
    pub(crate) mode: Mode,
    dry_run: bool,
    title: String,
    pub(crate) renderer: Renderer,
    pub(crate) tool_provider: Box<dyn ToolProvider>,
    pub(crate) version_provider: Box<dyn VersionProvider>,
    pub(crate) backend_provider: Box<dyn BackendProvider>,
    /// Preferred version specificity index (0=latest, 1=major, 2=major.minor, 3=full, etc.)
    pub(crate) preferred_specificity: usize,
    /// Undo stack for deleted items
    pub(crate) undo_stack: Vec<UndoAction>,
    /// Cached display path for rendering
    pub(crate) path_display: String,
}

impl InteractiveConfig {
    /// Create a new config editor for a new file with default sections
    pub fn new(path: PathBuf) -> Self {
        let path_display = Self::compute_path_display(&path);
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
            path_display,
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
        let path_display = Self::compute_path_display(&path);

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
            path_display,
        })
    }

    /// Infer version specificity from the last tool in the document
    fn infer_specificity_from_doc(doc: &TomlDocument) -> usize {
        // Find the tools section
        let tools_section = doc.sections.iter().find(|s| s.name == "tools");
        if let Some(section) = tools_section {
            // Get the last entry's version
            if let Some(entry) = section.entries.last()
                && let EntryValue::Simple(version) = &entry.value
            {
                return Self::version_to_specificity(version);
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

    /// Compute the display path for rendering
    fn compute_path_display(path: &std::path::Path) -> String {
        let abs_path = if path.is_relative() {
            std::env::current_dir()
                .map(|cwd| cwd.join(path))
                .unwrap_or_else(|_| path.to_path_buf())
        } else {
            path.to_path_buf()
        };
        xx::file::display_path(&abs_path)
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
        // Initial render
        self.render_current_mode()?;

        loop {
            // Read key in blocking task to not block the async runtime
            let term = self.renderer.term().clone();
            let key = tokio::task::spawn_blocking(move || term.read_key())
                .await
                .map_err(io::Error::other)??;

            let should_exit = self.handle_key(key).await?;
            if let Some(result) = should_exit {
                // Clear the display before returning
                self.renderer.clear()?;
                return Ok(result);
            }

            // Re-render
            self.render_current_mode()?;
        }
    }

    /// Render based on current mode
    pub(crate) fn render_current_mode(&mut self) -> io::Result<()> {
        match &self.mode {
            Mode::Picker(kind, picker) => {
                self.renderer
                    .render_picker(picker, kind, &self.path_display)?;
            }
            Mode::Loading(message) => {
                self.renderer
                    .render_loading(message, &self.title, &self.path_display)?;
            }
            _ => {
                self.renderer.render(
                    &self.doc,
                    &self.cursor,
                    &self.mode,
                    &self.title,
                    &self.path_display,
                    self.dry_run,
                    !self.undo_stack.is_empty(),
                )?;
            }
        }
        Ok(())
    }
}
