use std::path::PathBuf;

use eyre::{Result, bail};
use toml_edit::{Item, Value};

use crate::config::Config;
use crate::file::{self, display_path};
use crate::system::files::FileMode;
use crate::system::history::tracked::{EntryKind, TrackedSet, normalize_target};

/// Stop tracking a file or directory
///
/// Removes the `[dotfiles]` track entry (or switches an inherited one off in
/// config.local.toml) and stops future captures. The file itself and its
/// existing checkpoints are left exactly as they are.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub(crate) struct DotfilesUntrack {
    /// Paths to stop tracking
    #[usage(value_name = "PATH", required = true)]
    targets: Vec<String>,
}

impl DotfilesUntrack {
    pub(crate) async fn run(self) -> Result<()> {
        crate::system::history::ensure_experimental()?;
        let config = Config::get().await?;
        let managed = crate::system::files::files_from_config(&config)?;
        let tracked = TrackedSet::from_config(&config)?;
        let global = crate::config::global_config_path();
        let local = super::track::declaration_file(true)?;
        let mut touched: Vec<PathBuf> = vec![];
        for target_raw in &self.targets {
            let target = crate::system::files::resolve_target_arg(target_raw)
                .components()
                .collect::<PathBuf>();
            if target.is_relative() {
                bail!("{target_raw}: target must be absolute or start with ~/");
            }
            let key = super::track::normalized_target(&target);
            let path = normalize_target(&target);
            match managed.iter().find(|req| req.target == target) {
                Some(req) if req.mode == FileMode::Track => {
                    let declared_in = &req.origin.config;
                    if crate::config::is_system_config(declared_in)
                        || (declared_in != &global && declared_in != &local)
                    {
                        // inherited: switch it off on this machine
                        let mut doc = super::track::read_document(&local)?;
                        let mut table = toml_edit::InlineTable::new();
                        table.insert(
                            "mode",
                            Value::String(toml_edit::Formatted::new("track".into())),
                        );
                        table.insert("enabled", Value::Boolean(toml_edit::Formatted::new(false)));
                        doc["dotfiles"][&key] = Item::Value(Value::InlineTable(table));
                        file::write(&local, doc.to_string())?;
                        info!(
                            "dotfiles: {key} is declared in {}; switched off in {}",
                            display_path(declared_in),
                            display_path(&local)
                        );
                        touched.push(local.clone());
                    } else {
                        let mut doc = super::track::read_document(declared_in)?;
                        if let Some(table) = doc["dotfiles"].as_table_mut() {
                            table.remove(&key);
                        }
                        file::write(declared_in, doc.to_string())?;
                        info!("dotfiles: {key} removed from {}", display_path(declared_in));
                        touched.push(declared_in.clone());
                    }
                }
                Some(req) => bail!(
                    "{target_raw} is managed by a `{}` entry, not tracked; use `mise bootstrap dotfiles unapply` or edit {}",
                    req.mode.name(),
                    display_path(&req.origin.config)
                ),
                None => {
                    // a child of a tracked or implicit directory: exclude it
                    let Some(owner) = tracked.entry_for(&path) else {
                        bail!("{target_raw} is not tracked");
                    };
                    // an exclusion wins over everything, so one that covers
                    // a source a declaration still references would drop
                    // what the declaration uses
                    let sources: Vec<String> = tracked
                        .entries
                        .iter()
                        .filter(|entry| {
                            entry.kind == EntryKind::Source
                                && (entry.path.starts_with(&path) || path.starts_with(&entry.path))
                        })
                        .map(|entry| entry.display())
                        .collect();
                    if !sources.is_empty() {
                        bail!(
                            "{target_raw} holds or is part of the source of a `[dotfiles]` entry ({}); history captures a source while a declaration references it. Remove or change that entry instead",
                            sources.join(", ")
                        );
                    }
                    let glob = if path.is_dir() {
                        format!("{key}/**")
                    } else {
                        key.clone()
                    };
                    let mut doc = super::track::read_document(&global)?;
                    let history = doc
                        .entry("history")
                        .or_insert(Item::Table(toml_edit::Table::new()));
                    if let Some(table) = history.as_table_mut() {
                        table.set_implicit(false);
                        let exclude = table
                            .entry("exclude")
                            .or_insert(Item::Value(Value::Array(toml_edit::Array::new())));
                        if let Some(array) = exclude.as_array_mut()
                            && !array
                                .iter()
                                .any(|value| value.as_str() == Some(glob.as_str()))
                        {
                            array.push(Value::String(toml_edit::Formatted::new(glob.clone())));
                        }
                    }
                    file::write(&global, doc.to_string())?;
                    info!(
                        "dotfiles: {key} is covered by {} ({}); excluded it in {}",
                        owner.display(),
                        owner.mode,
                        display_path(&global)
                    );
                    touched.push(global.clone());
                }
            }
        }
        let mut config = Config::reset().await?;
        let mut tracked = TrackedSet::from_config(&config)?;
        for target_raw in &self.targets {
            let target = crate::system::files::resolve_target_arg(target_raw)
                .components()
                .collect::<PathBuf>();
            let key = super::track::normalized_target(&target);
            let path = normalize_target(&target);
            let still = tracked
                .entry_for(&path)
                .filter(|entry| entry.kind == EntryKind::Track && entry.path == path)
                .cloned();
            if let Some(entry) = still {
                // removing a local override exposed the declaration underneath:
                // the user's own global one goes too; an inherited one is
                // switched off on this machine
                if entry.declared_in.as_deref() == Some(global.as_path()) {
                    let mut doc = super::track::read_document(&global)?;
                    if let Some(table) = doc["dotfiles"].as_table_mut() {
                        table.remove(&key);
                    }
                    file::write(&global, doc.to_string())?;
                    info!("dotfiles: {key} removed from {}", display_path(&global));
                    config = Config::reset().await?;
                    tracked = TrackedSet::from_config(&config)?;
                    if !tracked
                        .entry_for(&path)
                        .is_some_and(|entry| entry.kind == EntryKind::Track && entry.path == path)
                    {
                        continue;
                    }
                }
                let mut doc = super::track::read_document(&local)?;
                let mut table = toml_edit::InlineTable::new();
                table.insert(
                    "mode",
                    Value::String(toml_edit::Formatted::new("track".into())),
                );
                table.insert("enabled", Value::Boolean(toml_edit::Formatted::new(false)));
                doc["dotfiles"][&key] = Item::Value(Value::InlineTable(table));
                file::write(&local, doc.to_string())?;
                info!(
                    "dotfiles: {key} is also declared in {}; switched off in {}",
                    entry
                        .declared_in
                        .as_deref()
                        .map(display_path)
                        .unwrap_or_else(|| "another layer".into()),
                    display_path(&local)
                );
                config = Config::reset().await?;
                tracked = TrackedSet::from_config(&config)?;
                if tracked
                    .entry_for(&path)
                    .is_some_and(|entry| entry.kind == EntryKind::Track && entry.path == path)
                {
                    bail!("dotfiles: {} is still tracked", display_path(&path));
                }
            }
        }
        info!("dotfiles: the files and their checkpoints were left in place");
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise bootstrap dotfiles untrack ~/.zshrc</bold>
"#
);
