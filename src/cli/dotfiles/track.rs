use std::path::{Path, PathBuf};

use eyre::{Result, bail};
use toml_edit::{Array, DocumentMut, InlineTable, Item, Value};

use crate::config::Config;
use crate::file::{self, display_path};
use crate::path::PathExt;
use crate::system::files::{FileMode, FileRequest};
use crate::system::history::checkpoint::{Draft, Outcome, Store};
use crate::system::history::select::Variant;
use crate::system::history::store::Trigger;
use crate::system::history::tracked::{EntryKind, TrackedSet, normalize_target};

/// Track a file or directory where it is
///
/// Adds a `[dotfiles]` entry with `mode = "track"`: the file stays where it
/// is, nothing is copied or linked, and history saves a checkpoint of it
/// right away. With the history watcher service running, later edits are
/// saved automatically; without it, `mise bootstrap dotfiles save` saves them.
///
/// `--os` and `--profile` declare a variant: a separate shared stream for
/// machines matching that platform or mise environment, so a Mac and a
/// Linux box can share the same live path with different contents.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub(crate) struct DotfilesTrack {
    /// Paths to track (absolute or starting with ~/)
    #[usage(value_name = "PATH", required = true)]
    targets: Vec<String>,

    /// Declare a variant for this platform (macos, linux, linux/arm64, …)
    #[usage(long, value_name = "OS")]
    os: Option<String>,

    /// Declare a variant for this mise environment
    #[usage(long, value_name = "PROFILE")]
    profile: Option<String>,

    /// Save only on `mise bootstrap dotfiles save <path>`, never automatically
    #[usage(long)]
    no_autosave: bool,

    /// Keep the file out of the shared setup (still backed up)
    #[usage(long)]
    no_share: bool,

    /// Keep the file out of remote backups (still protected locally)
    #[usage(long)]
    no_backup: bool,

    /// Write to config.local.toml (this machine only) instead of config.toml
    #[usage(long)]
    local: bool,

    /// Accept without prompting
    #[usage(long, short)]
    yes: bool,
}

impl DotfilesTrack {
    pub(crate) async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let managed = crate::system::files::files_from_config(&config)?;
        let config_path = declaration_file(self.local)?;
        let mut doc = read_document(&config_path)?;
        let original = doc.to_string();
        let existed = config_path.exists();
        let mut declared: Vec<(String, PathBuf)> = vec![];
        for target_raw in &self.targets {
            let target = crate::system::files::resolve_target_arg(target_raw)
                .components()
                .collect::<PathBuf>();
            if target.is_relative() {
                bail!("{target_raw}: target must be absolute or start with ~/");
            }
            let target_key = normalized_target(&target);
            if !target.exists() && !target.is_symlink() {
                warn!(
                    "dotfiles: {} does not exist yet; it is captured once it does",
                    target.display_user()
                );
            }
            let existing = managed.iter().find(|req| req.target == target);
            if let Some(existing) = existing
                && existing.mode != FileMode::Track
            {
                bail!(
                    "{target_raw} is managed by a `{}` entry in {} ({}); tracking is for files edited in place — untrack it there first or change its mode",
                    existing.mode.name(),
                    display_path(&existing.origin.config),
                    existing.source.display_user()
                );
            }
            let entry = self.entry(existing);
            let dotfiles = doc
                .entry("dotfiles")
                .or_insert(Item::Table(toml_edit::Table::new()));
            if let Some(table) = dotfiles.as_table_mut() {
                table.set_implicit(false);
                table.insert(&target_key, Item::Value(Value::InlineTable(entry)));
            } else {
                doc["dotfiles"][&target_key] = Item::Value(Value::InlineTable(entry));
            }
            declared.push((target_key, target));
        }
        if !self.yes && console::user_attended() {
            let list = declared
                .iter()
                .map(|(key, _)| key.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            if !crate::ui::prompt::confirm(format!(
                "dotfiles: track {list} in {}?",
                display_path(&config_path)
            ))?
            .is_yes()
            {
                info!("dotfiles: skipped");
                return Ok(());
            }
        }
        if let Some(table) = doc["dotfiles"].as_table_mut() {
            table.sort_values();
        }
        file::write(&config_path, doc.to_string())?;
        for (key, _) in &declared {
            info!(
                "dotfiles: tracking {key} (declared in {})",
                display_path(&config_path)
            );
        }

        // a declaration that ends up protecting nothing must not stay: the
        // write is undone when the entry is not active or no baseline could
        // be saved
        if let Err(err) = activate_and_baseline(&declared).await {
            if existed {
                file::write(&config_path, &original)?;
            } else {
                let _ = std::fs::remove_file(&config_path);
            }
            return Err(err.wrap_err(format!("{} was left unchanged", display_path(&config_path))));
        }
        crate::cli::dotfiles::capture_health::report();
        Ok(())
    }

    /// The inline table for a target: an existing track entry's fields with
    /// this command's changes on top, so a local override keeps variants and
    /// the other policies.
    fn entry(&self, existing: Option<&FileRequest>) -> InlineTable {
        let mut table = InlineTable::new();
        table.insert("mode", string("track"));
        let mut policy = existing
            .map(|req| req.policy)
            .unwrap_or_else(|| crate::system::files::FilePolicy::for_mode(FileMode::Track));
        if self.no_autosave {
            policy.autosave = false;
        }
        if self.no_share {
            policy.share = false;
        }
        if self.no_backup {
            policy.backup = false;
        }
        if !policy.autosave {
            table.insert("autosave", Value::Boolean(toml_edit::Formatted::new(false)));
        }
        if !policy.share {
            table.insert("share", Value::Boolean(toml_edit::Formatted::new(false)));
        }
        if !policy.backup {
            table.insert("backup", Value::Boolean(toml_edit::Formatted::new(false)));
        }
        let mut variants: Vec<Variant> =
            existing.map(|req| req.variants.clone()).unwrap_or_default();
        if self.os.is_some() || self.profile.is_some() {
            let variant = Variant {
                os: self.os.iter().cloned().collect(),
                profile: self.profile.clone(),
                default: false,
                share: None,
            };
            if !variants.contains(&variant) {
                variants.push(variant);
            }
        }
        if !variants.is_empty() {
            let mut array = Array::new();
            for variant in &variants {
                let mut item = InlineTable::new();
                match variant.os.as_slice() {
                    [] => {}
                    [os] => {
                        item.insert("os", string(os));
                    }
                    many => {
                        let mut list = Array::new();
                        for os in many {
                            list.push(string(os));
                        }
                        item.insert("os", Value::Array(list));
                    }
                }
                if let Some(profile) = &variant.profile {
                    item.insert("profile", string(profile));
                }
                if variant.default {
                    item.insert("default", Value::Boolean(toml_edit::Formatted::new(true)));
                }
                if let Some(share) = variant.share {
                    item.insert("share", Value::Boolean(toml_edit::Formatted::new(share)));
                }
                array.push(Value::InlineTable(item));
            }
            table.insert("variants", Value::Array(array));
        }
        table
    }
}

/// Checks that every declared entry is active and saves their baseline.
async fn activate_and_baseline(declared: &[(String, PathBuf)]) -> Result<()> {
    let config = Config::reset().await?;
    let tracked = TrackedSet::from_config(&config)?;
    for (key, target) in declared {
        let path = normalize_target(target);
        let active = tracked
            .entry_for(&path)
            .is_some_and(|entry| entry.kind == EntryKind::Track && entry.path == path);
        if !active {
            let reason = tracked
                .invalid
                .iter()
                .find(|invalid| invalid.path == display_path(&path))
                .map(|invalid| invalid.reason.clone())
                .unwrap_or_else(|| "the declaration was not loaded".into());
            bail!("dotfiles: {key} could not be tracked: {reason}");
        }
    }
    baseline(&tracked, declared).await
}

/// Saves the baseline checkpoint of newly tracked paths; a failure fails
/// the enrollment, since an untracked file must never look protected.
async fn baseline(tracked: &TrackedSet, declared: &[(String, PathBuf)]) -> Result<()> {
    if !crate::config::Settings::get().history.enabled {
        warn!("dotfiles: history is disabled (history.enabled = false); no baseline saved");
        return Ok(());
    }
    let store = Store::open()?;
    if let Some(reason) = store.unavailable() {
        bail!("dotfiles: cannot save the baseline: {reason}");
    }
    let names = declared
        .iter()
        .map(|(key, _)| key.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let mut draft = Draft::new(Trigger::Baseline);
    draft.explicit_paths = declared
        .iter()
        .map(|(_, path)| normalize_target(path))
        .collect();
    draft.description = Some(format!("tracked {names}"));
    // a write like a save: never interleaved with a running operation
    let _operation = crate::system::history::scope::take_operation_lock(&store, tracked)?;
    match store.attempt(tracked, draft)? {
        Outcome::Created(entry) => {
            info!("history: saved baseline checkpoint {}", entry.id);
            Ok(())
        }
        Outcome::Unchanged => Ok(()),
        Outcome::Unavailable(reason) => bail!("dotfiles: cannot save the baseline: {reason}"),
    }
}

/// `config.toml`, or `config.local.toml` next to it for machine-only
/// declarations.
pub(crate) fn declaration_file(local: bool) -> Result<PathBuf> {
    let global = crate::config::global_config_path();
    if !local {
        return Ok(global);
    }
    let dir = global.parent().unwrap_or(Path::new("."));
    Ok(dir.join("config.local.toml"))
}

pub(crate) fn read_document(path: &Path) -> Result<DocumentMut> {
    if path.exists() {
        let text = file::read_to_string(path)?;
        Ok(text
            .parse::<DocumentMut>()
            .map_err(|err| eyre::eyre!("parsing {}: {err}", display_path(path)))?)
    } else {
        Ok(DocumentMut::new())
    }
}

pub(crate) fn normalized_target(target: &Path) -> String {
    match target.strip_prefix(*crate::dirs::HOME) {
        Ok(rel) if !rel.as_os_str().is_empty() => format!("~/{}", rel.display()),
        Ok(_) => "~".to_string(),
        Err(_) => target.to_string_lossy().to_string(),
    }
}

fn string(text: &str) -> Value {
    Value::String(toml_edit::Formatted::new(text.to_string()))
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise bootstrap dotfiles track ~/.zshrc ~/.config/hypr</bold>
    $ <bold>mise bootstrap dotfiles track ~/.zshrc --os macos</bold>
    $ <bold>mise bootstrap dotfiles track ~/.config/app/state.json --no-autosave</bold>
    $ <bold>mise bootstrap dotfiles track ~/.ssh/config --no-share</bold>
"#
);
