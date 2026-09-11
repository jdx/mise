use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use eyre::{Result, bail};
use toml_edit::{Array, DocumentMut, InlineTable, Item, Value};

use crate::config::{Config, Settings};
use crate::file::{self, display_path};
use crate::path::PathExt;
use crate::system::files::{FileMode, FileRequest};
use crate::system::history::checkpoint::{Draft, Outcome, Store};
use crate::system::history::select::Variant;
use crate::system::history::store::Trigger;
use crate::system::history::tracked::{TrackedSet, normalize_target};

/// Track a file or directory in place
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

    /// Accept without prompting
    #[usage(long, short)]
    yes: bool,
}

impl DotfilesTrack {
    pub(crate) async fn run(self) -> Result<()> {
        let _declarations = declaration_lock()?;
        let config = Config::get().await?;
        let managed = crate::system::files::composed_files_from_config(&config)?;
        let global = declaration_file(false)?;
        let mut edits: BTreeMap<PathBuf, DeclarationEdit> = BTreeMap::new();
        let mut locations = BTreeMap::new();
        let mut declared: Vec<(String, PathBuf)> = vec![];
        let mut manual = vec![];
        for target_raw in &self.targets {
            let target = crate::system::files::resolve_target_arg(target_raw)
                .components()
                .collect::<PathBuf>();
            if target.is_relative() {
                bail!("{target_raw}: target must be absolute or start with ~/");
            }
            crate::system::history::tracked::ensure_portable_ancestors(&target)?;
            let target_key = normalized_target(&target);
            if !target.exists() && !target.is_symlink() {
                warn!(
                    "dotfiles: {} does not exist yet; it is captured once it does",
                    target.display_user()
                );
            }
            let existing = managed
                .iter()
                .find(|req| req.target == target && req.mode == FileMode::Track);
            let config_path = if let Some(existing) = existing
                && crate::config::is_global_config(&existing.origin.config)
                && !crate::config::is_system_config(&existing.origin.config)
            {
                existing.origin.config.clone()
            } else if managed
                .iter()
                .any(|req| req.target == target && req.mode != FileMode::Track)
            {
                global
                    .parent()
                    .unwrap_or(Path::new("."))
                    .join("conf.d/dotfiles-tracking.toml")
            } else {
                global.clone()
            };
            let edit = match edits.entry(config_path.clone()) {
                std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(DeclarationEdit::read(&config_path)?)
                }
            };
            let doc = &mut edit.document;
            let declaration_key = existing
                .filter(|req| req.origin.config == config_path)
                .map_or(target_key.as_str(), |req| req.target_raw.as_str());
            locations.insert(target_key.clone(), config_path);
            let entry = self.entry(existing);
            if entry.get("autosave").and_then(Value::as_bool) == Some(false) {
                manual.push(target_key.clone());
            }
            let dotfiles = doc
                .entry("dotfiles")
                .or_insert(Item::Table(toml_edit::Table::new()));
            if let Some(table) = dotfiles.as_table_mut() {
                table.set_implicit(false);
                table.insert(declaration_key, Item::Value(Value::InlineTable(entry)));
            } else {
                doc["dotfiles"][declaration_key] = Item::Value(Value::InlineTable(entry));
            }
            declared.push((target_key, target));
        }
        if !self.yes && !Settings::get().yes && console::user_attended_stderr() {
            let list = declared
                .iter()
                .map(|(key, _)| key.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            if !crate::ui::prompt::confirm(format!("dotfiles: track {list}?"))?.is_yes() {
                info!("dotfiles: skipped");
                return Ok(());
            }
        }
        let result = async {
            for (path, edit) in &mut edits {
                edit.write(path)?;
            }
            activate_and_baseline(&declared).await
        }
        .await;
        if let Err(error) = result {
            for (path, edit) in edits.iter().rev() {
                if let Err(recovery) = edit.restore(path) {
                    warn!(
                        "dotfiles: could not restore {}: {recovery:#}",
                        display_path(path)
                    );
                }
            }
            return Err(error);
        }
        for (key, _) in &declared {
            info!(
                "dotfiles: tracking {key} (declared in {})",
                display_path(&locations[key])
            );
        }
        if !manual.is_empty() {
            info!(
                "history: manual saving selected for {}; run `mise bootstrap dotfiles save <path>` after editing",
                manual.join(", ")
            );
        }
        if manual.len() < declared.len() {
            crate::cli::dotfiles::capture_health::report().await;
        }
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
        if policy.encrypt {
            table.insert("encrypt", Value::Boolean(toml_edit::Formatted::new(true)));
        }
        if !policy.autosave {
            table.insert("autosave", Value::Boolean(toml_edit::Formatted::new(false)));
        }
        let mut variants: Vec<Variant> =
            existing.map(|req| req.variants.clone()).unwrap_or_default();
        if self.os.is_some() || self.profile.is_some() {
            // Adding a specialization must not remove the stream that
            // already serves machines without that specialization.
            if existing.is_some() && variants.is_empty() {
                variants.push(Variant {
                    os: vec![],
                    profile: None,
                    default: true,
                });
            }
            let variant = Variant {
                os: self.os.iter().cloned().collect(),
                profile: self.profile.clone(),
                default: false,
            };
            if !variants.iter().any(|existing| {
                existing.os == variant.os
                    && existing.profile == variant.profile
                    && existing.default == variant.default
            }) {
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
                array.push(Value::InlineTable(item));
            }
            table.insert("variants", Value::Array(array));
        }
        table
    }
}

/// Keep declaration edits separate from deployment ownership. Only restore
/// our own written version if enrollment fails; never clobber a concurrent edit.
struct DeclarationEdit {
    document: DocumentMut,
    original: Option<String>,
    written: Option<String>,
}

impl DeclarationEdit {
    fn read(path: &Path) -> Result<Self> {
        let original = match std::fs::read_to_string(path) {
            Ok(body) => Some(body),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            document: original.as_deref().unwrap_or("").parse()?,
            original,
            written: None,
        })
    }

    fn write(&mut self, path: &Path) -> Result<()> {
        if let Some(table) = self.document["dotfiles"].as_table_mut() {
            table.sort_values();
        }
        let body = self.document.to_string();
        if let Some(parent) = path.parent() {
            file::create_dir_all(parent)?;
        }
        let prepared = file::prepare_atomic_write(path, &body)?;
        commit_declaration(path, self.original.as_deref(), prepared)?;
        self.written = Some(body);
        Ok(())
    }

    fn restore(&self, path: &Path) -> Result<()> {
        let Some(written) = &self.written else {
            return Ok(());
        };
        match &self.original {
            Some(original) => {
                let prepared = file::prepare_atomic_write(path, original)?;
                commit_declaration(path, Some(written), prepared)
            }
            None => {
                check_declaration(path, Some(written))?;
                Ok(std::fs::remove_file(path)?)
            }
        }
    }
}

/// Serialize tracking declaration commands across their entire read/edit/
/// baseline/recovery interval. Use one lock for multiple config files, and
/// fail promptly rather than blocking an async runtime worker.
pub(super) fn declaration_lock() -> Result<fslock::LockFile> {
    declaration_lock_for(&crate::config::global_shared_config_path())
}

fn declaration_lock_for(config: &Path) -> Result<fslock::LockFile> {
    crate::lock_file::LockFile::new(&config.with_extension("dotfiles-declarations.lock"))
        .try_lock()?
        .ok_or_else(|| {
            eyre::eyre!("another tracking declaration command is running; retry shortly")
        })
}

fn check_declaration(path: &Path, expected: Option<&str>) -> Result<()> {
    let current = match std::fs::read_to_string(path) {
        Ok(body) => Some(body),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    if current.as_deref() != expected {
        bail!(
            "{} changed while preparing enrollment; concurrent declaration edit preserved; retry",
            display_path(path)
        );
    }
    Ok(())
}

fn commit_declaration(
    path: &Path,
    expected: Option<&str>,
    prepared: file::PreparedAtomicWrite,
) -> Result<()> {
    // Check after formatting, directory creation, writing, and fsync. The
    // declaration lock coordinates mise writers; unrelated editors do not
    // participate, so this is not a filesystem compare-and-swap guarantee.
    check_declaration(path, expected)?;
    prepared.commit()
}

/// Checks that every declared entry is active and saves their baseline.
async fn activate_and_baseline(declared: &[(String, PathBuf)]) -> Result<()> {
    let config = Config::reset().await?;
    let tracked = TrackedSet::from_config(&config)?;
    for (key, target) in declared {
        let path = normalize_target(target);
        let active = tracked
            .entry_for(&path)
            .is_some_and(|entry| entry.path == path);
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
    baseline(&tracked, declared).await?;
    for (key, target) in declared {
        if let Ok(source) = std::fs::read_link(target) {
            let source = crate::system::history::tracked::normalize(
                &target.parent().unwrap_or(Path::new("/")).join(source),
            );
            if !tracked.would_capture(&source)? {
                warn!(
                    "dotfiles: {key} is a symlink; history saves and syncs the link, not its contents. Its source {} is not tracked for capture; track the source with `mise bootstrap dotfiles track {}` (and check any exclusions) to include its contents",
                    display_path(&source),
                    shell_words::quote(&source.to_string_lossy()),
                );
            }
        }
    }
    Ok(())
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
    // A capture wrapper owns the operation lock until this child exits. It
    // reloads enrollment and explicitly saves all current tracked files in
    // its outcome (including manual entries), or during interruption recovery.
    if let Some(parent) = std::env::var_os(crate::system::history::scope::ENV_VAR)
        && crate::system::history::store::read_marker_in(&crate::dirs::STATE)?.is_some_and(
            |marker| {
                marker.kind == crate::system::history::store::OperationKind::Capture
                    && parent == std::ffi::OsStr::new(&marker.uuid)
            },
        )
    {
        info!(
            "dotfiles: enrolled; the enclosing capture will save the baseline when the command finishes"
        );
        return Ok(());
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
    let tracked = tracked.clone();
    tokio::task::spawn_blocking(move || {
        // Lock waits and filesystem capture must not block a Tokio worker.
        let _operation = crate::system::history::scope::take_operation_lock(&store, &tracked)?;
        match store.attempt(&tracked, draft)? {
            Outcome::Created(entry) => {
                info!("history: saved baseline checkpoint {}", entry.id);
                Ok(())
            }
            Outcome::Unchanged => Ok(()),
            Outcome::Unavailable(reason) => bail!("dotfiles: cannot save the baseline: {reason}"),
        }
    })
    .await?
}

/// `config.toml`, or `config.local.toml` next to it for machine-only
/// declarations.
pub(crate) fn declaration_file(local: bool) -> Result<PathBuf> {
    let global = crate::config::global_shared_config_path();
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

/// The `[dotfiles]` key of a path: `~/…` with forward slashes on every
/// platform, so it matches a hand-written key and the one `untrack` looks
/// for.
pub(crate) fn normalized_target(target: &Path) -> String {
    match target.strip_prefix(*crate::dirs::HOME) {
        Ok(rel) if !rel.as_os_str().is_empty() => {
            let rel = rel.to_string_lossy();
            let rel = if cfg!(windows) {
                rel.replace('\\', "/")
            } else {
                rel.into_owned()
            };
            format!("~/{rel}")
        }
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
"#
);

/// Adds (or removes) a glob in `[history] exclude` of the global config.
/// Returns whether the file changed.
pub(crate) fn edit_exclude(glob: &str, add: bool) -> Result<bool> {
    use toml_edit::{Item, Value};
    let global = crate::config::global_shared_config_path();
    let mut doc = read_document(&global)?;
    let history = doc
        .entry("history")
        .or_insert(Item::Table(toml_edit::Table::new()));
    let Some(table) = history.as_table_mut() else {
        eyre::bail!("[history] in {} is not a table", display_path(&global));
    };
    table.set_implicit(false);
    let exclude = table
        .entry("exclude")
        .or_insert(Item::Value(Value::Array(toml_edit::Array::new())));
    let Some(array) = exclude.as_array_mut() else {
        eyre::bail!(
            "[history] exclude in {} is not an array",
            display_path(&global)
        );
    };
    let present = array.iter().any(|value| value.as_str() == Some(glob));
    let changed = if add && !present {
        array.push(Value::String(toml_edit::Formatted::new(glob.to_string())));
        true
    } else if !add && present {
        array.retain(|value| value.as_str() != Some(glob));
        true
    } else {
        false
    };
    if changed {
        crate::file::write(&global, doc.to_string())?;
    }
    Ok(changed)
}

#[cfg(test)]
mod declaration_tests {
    use super::*;

    #[test]
    fn declaration_commands_fail_promptly_on_contention() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("config.toml");
        let first = declaration_lock_for(&path).unwrap();
        assert!(declaration_lock_for(&path).is_err());
        drop(first);
        assert!(declaration_lock_for(&path).is_ok());
    }

    #[test]
    fn edits_during_replacement_preparation_are_preserved() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("config.toml");
        std::fs::write(&path, "# original\n").unwrap();
        let prepared = file::prepare_atomic_write(&path, "# mise replacement\n").unwrap();
        std::fs::write(&path, "# external editor\n").unwrap();
        assert!(commit_declaration(&path, Some("# original\n"), prepared).is_err());
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            "# external editor\n"
        );
    }

    #[test]
    fn failed_enrollment_restores_only_its_own_declaration_version() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("conf.d/dotfiles-tracking.toml");
        let mut edit = DeclarationEdit::read(&path).unwrap();
        edit.document = "[dotfiles]\n\"~/.zshrc\" = { mode = \"track\" }\n"
            .parse()
            .unwrap();
        edit.write(&path).unwrap();
        assert!(path.exists());
        std::fs::write(&path, "# concurrent user edit\n").unwrap();
        assert!(edit.restore(&path).is_err());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# concurrent user edit\n"
        );
    }

    #[test]
    fn concurrent_edit_before_enrollment_is_not_overwritten() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("config.toml");
        let mut edit = DeclarationEdit::read(&path).unwrap();
        edit.document = "[dotfiles]\n".parse().unwrap();
        std::fs::write(&path, "# newly created by user\n").unwrap();
        assert!(edit.write(&path).is_err());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# newly created by user\n"
        );
    }
}
