use std::path::PathBuf;

use eyre::{Result, bail};
use toml_edit::{Item, Value};

use crate::config::Config;
use crate::file::{self, display_path};
use crate::system::files::FileMode;
use crate::system::history::tracked::{TrackedSet, normalize_target};

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
        let _declarations = super::track::declaration_lock()?;
        let config = Config::get().await?;
        let managed = crate::system::files::files_from_config(&config)?;
        let tracked = TrackedSet::effective().await?;
        let global = crate::config::global_shared_config_path();
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
            match managed
                .iter()
                .find(|req| req.target == target && req.mode == FileMode::Track)
            {
                Some(req) => {
                    let declared_in = &req.origin.config;
                    if crate::config::is_system_config(declared_in)
                        || !crate::config::is_global_config(declared_in)
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
                            table.remove(&req.target_raw);
                        }
                        file::write(declared_in, doc.to_string())?;
                        info!("dotfiles: {key} removed from {}", display_path(declared_in));
                        touched.push(declared_in.clone());
                    }
                }
                None => {
                    // A child of an explicitly tracked directory: exclude it.
                    let Some(owner) = tracked.entry_for(&path) else {
                        bail!("{target_raw} is not tracked");
                    };
                    if owner.path == path {
                        // Keep explicit intent visible to an enclosing capture
                        // too, even when enrollment came only from Git.
                        let mut doc = super::track::read_document(&local)?;
                        let mut table = toml_edit::InlineTable::new();
                        table.insert("mode", Value::from("track"));
                        table.insert("enabled", Value::from(false));
                        doc["dotfiles"][&key] = Item::Value(Value::InlineTable(table));
                        if let Some(parent) = local.parent() {
                            file::create_dir_all(parent)?;
                        }
                        file::write(&local, doc.to_string())?;
                        touched.push(local.clone());
                        continue;
                    }
                    let glob = if path.is_dir() {
                        format!("{key}/**")
                    } else {
                        key.clone()
                    };
                    super::track::edit_exclude(&glob, true)?;
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
                .filter(|entry| entry.path == path)
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
                    if tracked
                        .entry_for(&path)
                        .is_none_or(|entry| entry.path != path)
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
                    .is_some_and(|entry| entry.path == path)
                {
                    bail!("dotfiles: {} is still tracked", display_path(&path));
                }
            }
        }
        // A capture command may invoke untrack itself. Its parent holds the
        // operation lock and records the new enrollment in its outcome; do
        // not wait on that parent or commit a competing boundary here.
        if std::env::var_os(crate::system::history::scope::ENV_VAR).is_some()
            || crate::system::history::scope::is_active()
        {
            info!(
                "dotfiles: tracking stopped; the enclosing operation will record the change. Live files and earlier committed versions remain"
            );
            return Ok(());
        }
        let store = crate::system::history::checkpoint::Store::open()?;
        let _operation = crate::system::history::scope::take_operation_lock(&store, &tracked)?;
        let mut draft = crate::system::history::checkpoint::Draft::new(
            crate::system::history::store::Trigger::Save,
        );
        draft.description = Some("stop tracking files".into());
        draft.untrack = self
            .targets
            .iter()
            .map(|path| normalize_target(&crate::system::files::resolve_target_arg(path)))
            .collect();
        if let crate::system::history::checkpoint::Outcome::Unavailable(reason) =
            store.attempt(&tracked, draft)?
        {
            bail!("tracking declarations updated, but could not commit the change: {reason}");
        }
        info!(
            "dotfiles: live files were left in place; their previously committed versions remain in Git"
        );
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise bootstrap dotfiles untrack ~/.zshrc</bold>
"#
);
