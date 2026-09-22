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
use crate::system::history::tracked::{
    CREDENTIAL_REASON, TrackedEntry, TrackedSet, capture_exclusion, normalize_target,
};

/// Track a file or directory in place
///
/// Adds a `[dotfiles]` entry with `mode = "track"`: the file stays where it
/// is, nothing is copied or linked, and history saves a checkpoint of it
/// right away. With the history watcher service running, later edits are
/// saved automatically; without it, `mise dot save` saves them.
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

    /// Save only on `mise dot save <path>`, never automatically
    #[usage(long)]
    no_autosave: bool,

    /// Encrypt contents before saving them to history (requires `[history.encryption].recipients`)
    #[usage(long)]
    encrypt: bool,

    /// Accept without prompting
    #[usage(long, short)]
    yes: bool,

    /// Show what each path expands to (files, size, what is left out) without tracking it
    #[usage(long, short = 'n')]
    dry_run: bool,
}

/// How many omitted or nested paths a dry run lists before it counts the
/// rest, so a tree full of them cannot flood the terminal.
const DRY_RUN_LINES: usize = 20;

impl DotfilesTrack {
    /// Write the requested declarations and capture their initial history baseline.
    pub(crate) async fn run(self) -> Result<()> {
        // a dry run reads and walks but writes nothing, so it must not
        // hold up (or be held up by) a declaration command
        let _declarations = if self.dry_run {
            None
        } else {
            Some(declaration_lock()?)
        };
        let config = Config::get().await?;
        if self.encrypt && !Settings::get().history.enabled {
            bail!("dotfiles: cannot enroll encrypted paths while history is disabled");
        }
        if self.encrypt && inside_capture()? {
            bail!(
                "dotfiles: cannot enroll encrypted paths inside an active history capture; run `mise dot track --encrypt` separately so its baseline can be verified"
            );
        }
        let managed = crate::system::files::composed_files_from_config(&config)?;
        let global = declaration_file(false)?;
        let mut edits: BTreeMap<PathBuf, DeclarationEdit> = BTreeMap::new();
        let mut locations = BTreeMap::new();
        let mut declared: Vec<(String, PathBuf)> = vec![];
        let mut manual = vec![];
        // what each path expands to, sized up before anything is written:
        // one walk of every target of this run beside the entries already
        // tracked, so nested targets partition instead of the outer one
        // counting the inner one's files too
        let exclude = crate::system::history::config::exclude_globs()?;
        let mut preview_set = TrackedSet {
            exclude: exclude.clone(),
            ..Default::default()
        };
        let mut resolved: Vec<PathBuf> = vec![];
        for target_raw in &self.targets {
            let target = crate::system::files::resolve_target_arg(target_raw)
                .components()
                .collect::<PathBuf>();
            if target.is_relative() {
                bail!("{target_raw}: target must be absolute or start with ~/");
            }
            crate::system::history::tracked::ensure_portable_ancestors(&target)?;
            let existing = managed
                .iter()
                .find(|req| req.target == target && req.mode == FileMode::Track);
            let mut entry =
                TrackedEntry::new(normalize_target(&target), "track", self.policy(existing));
            // re-tracking previews under the entry's own exclude list
            if let Some(existing) = existing {
                entry.exclude = existing.policy.explicit.exclude.then(|| {
                    existing
                        .exclude
                        .iter()
                        .map(|pattern| pattern.as_str().to_owned())
                        .collect()
                });
            }
            preview_set.push(entry);
            resolved.push(target);
        }
        // every declaration is in the set, so a target nested under one
        // of them is attributed the way a capture would attribute it
        for entry in TrackedSet::from_config(&config)?.entries {
            preview_set.push(entry);
        }
        // but only the targets are walked: the preview reports on them,
        // and walking the rest would re-stat every tracked directory on
        // the machine to print nothing about them
        let targets: Vec<usize> = resolved
            .iter()
            .filter_map(|target| preview_set.entry_index_for(&normalize_target(target)))
            .collect();
        let preview_walk = preview_set.walk_selected(&targets)?;
        preview_walk.report_warnings();
        let mut previews: Vec<String> = vec![];
        for target in resolved {
            let target_key = normalized_target(&target);
            let present = target.exists() || target.is_symlink();
            if !present {
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
            let policy = self.policy(existing);
            if !policy.autosave {
                manual.push(target_key.clone());
            }
            // a file the guard drops must not look protected once tracked:
            // say so before the declaration is written.
            //
            // **The kind is the declaration's, never `is_dir()` on a path
            // that is not there.** The guard reads a file's own name and
            // never a directory's, so a tracked directory is walked and
            // its files are decided one by one — but `is_dir()` is also
            // false for a path this command has just said is captured once
            // it exists. A directory named `credentials` or `oauth-apps`
            // would otherwise be promised a protection it never gets, and
            // the user would put a real secret inside it. A path with no
            // kind yet is told what happens to it as a file instead.
            if !target.is_dir()
                && let Some(reason) = capture_exclusion(&target, &policy)
            {
                let advice = if reason == CREDENTIAL_REASON {
                    "; `mise dot track --encrypt` saves it encrypted"
                } else {
                    ""
                };
                if present {
                    warn!(
                        "dotfiles: {target_key} will be omitted from every save ({reason}){advice}"
                    );
                } else {
                    warn!(
                        "dotfiles: {target_key} is omitted from every save if it is created as a file, never as a directory ({reason}){advice}"
                    );
                }
            }
            let set = &preview_set;
            let entry_index = set
                .entry_index_for(&normalize_target(&target))
                .expect("every target is an entry of the preview set");
            let preview = preview_walk.preview_of(set, entry_index);
            let summary = preview.summary();
            if self.dry_run {
                miseprintln!("{target_key}: {summary}");
                // what an exclusion glob left out is not walked at all, so
                // the globs in force are the only account of it
                for glob in &set.exclude {
                    miseprintln!("  exclude: {glob}");
                }
                for glob in set.entries[entry_index].exclude.iter().flatten() {
                    miseprintln!("  exclude ({target_key}): {glob}");
                }
                // nothing is enrolled yet, so `mise dot paths` cannot list
                // these until the path is tracked: a bounded list here
                let lines: Vec<String> = preview
                    .omitted
                    .iter()
                    .map(|omitted| format!("omitted: {} ({})", omitted.path, omitted.reason))
                    .chain(
                        preview
                            .nested
                            .iter()
                            .map(|nested| format!("nested: {} ({})", nested.path, nested.reason)),
                    )
                    .collect();
                for line in lines.iter().take(DRY_RUN_LINES) {
                    miseprintln!("  {line}");
                }
                if lines.len() > DRY_RUN_LINES {
                    miseprintln!(
                        "  ... {} more; `mise dot paths` lists them all once the path is tracked",
                        lines.len() - DRY_RUN_LINES
                    );
                }
                for incomplete in &preview.incomplete {
                    miseprintln!("  incomplete: {} ({})", incomplete.path, incomplete.reason);
                }
            }
            // a truncated tree must be visible before it is approved, since
            // the baseline walks under the same cap
            if !self.dry_run {
                for incomplete in &preview.incomplete {
                    warn!(
                        "dotfiles: {}: {}; the rest would not be captured either",
                        incomplete.path, incomplete.reason
                    );
                }
            }
            if preview.is_large() {
                warn!(
                    "dotfiles: {target_key} is a large tree ({summary}); exclude what does not belong in history, for example `mise dot exclude '{target_key}/<subdir>/**'`, or track its files individually"
                );
            }
            previews.push(summary);
            // the keys this file's declaration wrote, whether as an inline
            // table or a `[dotfiles."path"]` table
            let previous_table = doc
                .get("dotfiles")
                .and_then(|dotfiles| dotfiles.get(declaration_key))
                .and_then(Item::as_table_like);
            let previous: Vec<String> = previous_table
                .map(|table| table.iter().map(|(key, _)| key.to_string()).collect())
                .unwrap_or_default();
            // the list as this file wrote it, so a pattern the loader
            // could not parse (and warned about) is not silently dropped
            let previous_exclude = previous_table
                .and_then(|table| table.get("exclude"))
                .and_then(Item::as_array)
                .cloned();
            let entry = self.entry(existing, &previous, previous_exclude);
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
        if self.dry_run {
            info!("dotfiles: dry run; nothing was tracked");
            return Ok(());
        }
        if !self.yes && !Settings::get().yes && console::user_attended_stderr() {
            let list = declared
                .iter()
                .zip(&previews)
                .map(|((key, _), summary)| format!("{key} ({summary})"))
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
        for ((key, _), summary) in declared.iter().zip(&previews) {
            info!(
                "dotfiles: tracking {key} ({summary}; declared in {})",
                display_path(&locations[key])
            );
        }
        if !manual.is_empty() {
            info!(
                "history: manual saving selected for {}; run `mise dot save <path>` after editing",
                manual.join(", ")
            );
        }
        if manual.len() < declared.len() {
            crate::cli::dotfiles::capture_health::report().await;
        }
        Ok(())
    }

    /// The policy a target is tracked under: the existing entry's, with
    /// this command's flags on top.
    fn policy(&self, existing: Option<&FileRequest>) -> crate::system::files::FilePolicy {
        let mut policy = existing
            .map(|req| req.policy)
            .unwrap_or_else(|| crate::system::files::FilePolicy::for_mode(FileMode::Track));
        if self.no_autosave {
            policy.autosave = false;
        }
        if self.encrypt {
            policy.encrypt = true;
        }
        policy
    }

    /// The inline table for a target: an existing track entry's fields with
    /// this command's changes on top, so a local override keeps variants and
    /// the other policies. `previous` holds the keys the declaration this
    /// file held before wrote: a policy it wrote explicitly stays written,
    /// even at its default value, while one it inherited from another
    /// layer stays unwritten so that layer keeps deciding it.
    fn entry(
        &self,
        existing: Option<&FileRequest>,
        previous: &[String],
        previous_exclude: Option<Array>,
    ) -> InlineTable {
        let mut table = InlineTable::new();
        table.insert("mode", string("track"));
        let policy = self.policy(existing);
        // a policy is written when this command sets it or this file wrote
        // it before; one inherited from another layer stays unwritten so
        // that layer keeps deciding it
        let written = |key: &str| previous.iter().any(|written| written == key);
        if self.encrypt || written("encrypt") {
            table.insert(
                "encrypt",
                Value::Boolean(toml_edit::Formatted::new(policy.encrypt)),
            );
        }
        if self.no_autosave || written("autosave") {
            table.insert(
                "autosave",
                Value::Boolean(toml_edit::Formatted::new(policy.autosave)),
            );
        }
        // re-tracking keeps the entry's own exclude list as this file wrote
        // it (an explicitly empty one, or a pattern the loader rejected,
        // included); a list inherited from another layer stays with that
        // layer, like the policies
        if existing.is_some() && written("exclude") {
            let list = previous_exclude.unwrap_or_else(|| {
                let mut list = Array::new();
                for pattern in existing.iter().flat_map(|req| &req.exclude) {
                    list.push(string(pattern.as_str()));
                }
                list
            });
            table.insert("exclude", Value::Array(list));
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
        if target.is_symlink() {
            // This resolver is read-only, follows dangling chains, and bounds
            // traversal so cyclic links cannot hang an advisory check.
            let source = match resolve_symlink_source(target) {
                Ok(source) => source,
                Err(error) => {
                    warn!(
                        "dotfiles: {key} is a symlink; history saves and syncs the link, not its contents. Could not resolve its source: {error}"
                    );
                    continue;
                }
            };
            if !tracked.would_capture(&source)? {
                warn!(
                    "dotfiles: {key} is a symlink; history saves and syncs the link, not its contents. Its source {} is not tracked for capture; track the source with `mise dot track {}` (and check any exclusions) to include its contents",
                    display_path(&source),
                    shell_words::quote(&source.to_string_lossy()),
                );
            }
        }
    }
    Ok(())
}

/// Resolve link chains using the same path representation as tracked entries.
fn resolve_symlink_source(target: &Path) -> Result<PathBuf> {
    file::atomic_write_target(target).map(|source| normalize_target(&source))
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
    if inside_capture()? {
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

/// Whether this process is the command running inside a live capture wrapper.
fn inside_capture() -> Result<bool> {
    let Some(parent) = std::env::var_os(crate::system::history::scope::ENV_VAR) else {
        return Ok(false);
    };
    Ok(
        crate::system::history::store::read_marker_in(&crate::dirs::STATE)?.is_some_and(|marker| {
            marker.kind == crate::system::history::store::OperationKind::Capture
                && parent == std::ffi::OsStr::new(&marker.uuid)
        }),
    )
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

    $ <bold>mise dot track ~/.zshrc ~/.config/hypr</bold>
    $ <bold>mise dot track --dry-run ~/.codex</bold>
    $ <bold>mise dot track ~/.zshrc --os macos</bold>
    $ <bold>mise dot track ~/.config/app/credentials --encrypt</bold>
    $ <bold>mise dot track ~/.config/app/state.json --no-autosave</bold>
"#
);

/// Adds (or removes) a glob in `[history] exclude` of the global config.
/// Returns whether the file changed.
/// The `[history] exclude` rule that names exactly `key` and nothing
/// else.
///
/// **A path is a literal; the list holds globs.** `mise dot untrack`
/// writes the path it can no longer track, and written as-is a name
/// holding `[` is an unclosed character class the matcher refuses, while
/// one holding `*` or `?` silently matches the neighbours too. The glob
/// metacharacters are escaped with the escape of the matcher that
/// compiles the rule, so it means that one path. `$` has no escape in
/// this pattern language — it is refused outright rather than read as an
/// environment variable — so such a path is reported by
/// [`edit_exclude`] instead of being written.
pub(crate) fn exclude_rule_for_path(key: &str, directory: bool) -> String {
    let escaped = globset::escape(key);
    match directory {
        true => format!("{escaped}/**"),
        false => escaped,
    }
}

pub(crate) fn edit_exclude(glob: &str, add: bool) -> Result<bool> {
    use toml_edit::{Item, Value};
    // **mise never writes a rule mise would refuse to load.** Every
    // writer of this list arrives here — `mise dot exclude`, and
    // `mise dot untrack` covering a path it can no longer track — and a
    // rule the matcher cannot compile stops every later capture until
    // someone edits the file by hand. So the check belongs at the write,
    // not at one caller: `untrack` had no check, and untracking a file
    // whose name held a `[` or a `$` wrote configuration that disabled
    // `mise dot save`.
    if add
        && let Some(reason) = crate::system::history::tracked::unusable_pattern(
            glob.strip_prefix('!').unwrap_or(glob),
        )
    {
        bail!("{glob}: {reason}");
    }
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
    let changed = if add {
        append_rule(array, glob)
    } else {
        remove_argument(array, glob)
    };
    if changed {
        crate::file::write(&global, doc.to_string())?;
    }
    Ok(changed)
}

/// The entries of a list as plain strings, for comparing an edit's result
/// with what was there before.
fn list_entries(array: &toml_edit::Array) -> Vec<Option<String>> {
    array
        .iter()
        .map(|value| value.as_str().map(str::to_string))
        .collect()
}

/// Puts `glob` in force in a list the last matching pattern decides, and
/// reports whether that changed it.
///
/// **The rule: append the requested rule at the end, after removing any
/// earlier entry whose pattern string is exactly the requested one, in
/// either polarity. Report it as already in force only when that leaves
/// the list unchanged.**
///
/// This decides the question without reasoning about whether one glob
/// subsumes another. The list is last-match-wins, so the appended rule
/// decides every file the pattern matches whatever overlapping globs sit
/// earlier — `["foo", "!foo*"]` really does re-include `foo` until
/// `exclude foo` appends its own copy, and testing literal membership
/// would have called that "already excluded" and written nothing.
/// Dropping the earlier entries that spell the pattern exactly cannot
/// change any other path's outcome either, because a pattern bears only
/// on the paths it matches and the appended copy already decides those.
fn append_rule(array: &mut toml_edit::Array, glob: &str) -> bool {
    // compared without its polarity on both sides: `mise dot exclude
    // '!foo'` and `mise dot exclude foo` are the same rule written two
    // ways, and either one appended must take the other's earlier copy
    // with it — otherwise the list keeps a stale entry the doc above
    // says it removes
    let bare = |pattern: &str| pattern.strip_prefix('!').unwrap_or(pattern).to_string();
    let subject = bare(glob);
    let before = list_entries(array);
    array.retain(|value| match value.as_str() {
        Some(entry) => bare(entry) != subject,
        None => true,
    });
    array.push(string(glob));
    list_entries(array) != before
}

/// Removes `glob` from the list, and reports whether that changed
/// anything. This is not [`append_rule`] with a negation: `mise dot
/// include` takes back a glob the user wrote with `mise dot exclude`, so
/// it removes that entry rather than appending `!glob` beside it, and it
/// leaves a hand-written `!glob` alone — that entry already re-includes,
/// which is what the caller wants.
/// Takes a rule out of the list, in either spelling it may have been
/// written in.
///
/// **What the user names is a path or a glob; what the list holds may be
/// the escaped form of it.** `mise dot exclude` writes a glob as typed,
/// while `mise dot untrack` writes a literal path with its glob
/// metacharacters escaped, so the rule for `~/.codex/cache[1]` is on disk
/// as `~/.codex/cache[[]1[]]`. Matching only the exact string left
/// `mise dot include '~/.codex/cache[1]'` reporting that the path was not
/// excluded while the escaped rule went on matching it — the undo for
/// `untrack` simply did not work. Escaping a real glob produces something
/// no list holds, so trying both spellings cannot remove a rule the user
/// did not name.
/// Takes exactly `rule` out of the list. No spelling is inferred here:
/// what to remove is [`rules_for_argument`]'s answer.
fn drop_glob(array: &mut toml_edit::Array, rule: &str) -> bool {
    let before = list_entries(array);
    array.retain(|value| value.as_str() != Some(rule));
    list_entries(array) != before
}

/// Takes back whatever the list holds for `argument`, and reports
/// whether that changed anything.
///
/// **What the user typed wins over what it could be derived into.** A
/// rooted argument is both a possible glob and a possible path:
/// `~/.codex/foo*` is the rule a user typed with `mise dot exclude`, and
/// it is also what `mise dot untrack` would have written for a file
/// literally named `foo*` — escaped, as `~/.codex/foo[*]`. Removing both
/// derivations unconditionally meant taking back a glob silently
/// re-included a file someone had untracked by name, which is the same
/// collision `foo*` and `foo[*]` already had, surviving one spelling
/// further up in rooted form. No syntax tells the two apart, because
/// `cache[1]` is a valid glob as well as a real filename.
///
/// The list settles what syntax cannot: remove the entry that spells the
/// argument exactly, and derive the path spellings only when the list
/// holds no such entry — which is precisely the `untrack` undo the
/// derivation exists for.
fn remove_argument(array: &mut toml_edit::Array, argument: &str) -> bool {
    if drop_glob(array, argument) {
        return true;
    }
    // every rule that means this argument, not the first one found:
    // `any` would stop at the first removal and leave the rest
    let mut changed = false;
    for rule in path_rules_for_argument(argument) {
        changed |= drop_glob(array, &rule);
    }
    changed
}

/// The list entries [`exclude_rule_for_path`] could have written for an
/// argument that names a path.
///
/// `mise dot untrack` writes a path through that function, the single
/// one that turns a path into a rule, so the same function says what to
/// remove — both forms it can produce, since which one was written
/// depended on whether the path was a directory then, and the answer now
/// is not evidence about then.
///
/// Matching spellings against each other instead was wrong twice over:
/// it removed `foo[*]` when asked for `foo*`, which are different rules,
/// and it never removed the `/**` form at all, so a directory untrack
/// could not be undone.
fn path_rules_for_argument(argument: &str) -> Vec<String> {
    // A path is absolute or `~`-rooted; a glob like `sessions/**` is
    // neither, and deriving from it would invent a rule nobody wrote.
    let target = crate::system::files::resolve_target_arg(argument);
    if !target.is_absolute() {
        return vec![];
    }
    let key = normalized_target(&target);
    let mut rules = vec![];
    for directory in [false, true] {
        let rule = exclude_rule_for_path(&key, directory);
        if rule != argument && !rules.contains(&rule) {
            rules.push(rule);
        }
    }
    rules
}

#[cfg(test)]
mod exclude_list_tests {
    use super::*;

    /// The same rule written with either polarity is one rule: appending
    /// it takes the other spelling's earlier copy with it, whichever way
    /// round they were written.
    #[test]
    fn a_rule_replaces_its_own_negation_either_way_round() {
        for (existing, appended, expected) in [
            (vec!["foo"], "!foo", vec!["!foo"]),
            (vec!["!foo"], "foo", vec!["foo"]),
            (vec!["foo", "bar"], "!foo", vec!["bar", "!foo"]),
            (vec!["!foo", "bar"], "!foo", vec!["bar", "!foo"]),
            (vec!["bar"], "!foo", vec!["bar", "!foo"]),
        ] {
            let mut list = array(&existing);
            append_rule(&mut list, appended);
            assert_eq!(entries(&list), expected, "{existing:?} + {appended:?}");
        }
    }

    /// **A rooted glob and the escaped rule for a file of that name are
    /// two different rules, and taking one back must not take the
    /// other.** `~/.codex/foo*` is what `mise dot exclude` writes for a
    /// glob; `~/.codex/foo[*]` is what `mise dot untrack` writes for a
    /// file literally named `foo*`. Deriving both from the argument
    /// removed the literal rule too, so `mise dot include` on the glob
    /// silently re-included a file someone had untracked by name. This
    /// is the `foo*` / `foo[*]` collision one spelling further up: bare
    /// arguments never derive, so only the rooted form still had it.
    #[test]
    fn a_rooted_glob_does_not_take_the_literal_rule_with_it() {
        let glob = "~/.codex/foo*";
        let literal = exclude_rule_for_path(glob, false);
        assert_eq!(literal, "~/.codex/foo[*]", "escaping changed spelling");

        let mut list = array(&[glob, &literal]);
        assert!(remove_argument(&mut list, glob));
        assert_eq!(
            entries(&list),
            vec![literal.clone()],
            "taking back a rooted glob removed the rule for a file named `foo*`"
        );

        // and the same argument still undoes an `untrack` when that
        // escaped rule is all the list holds — which is what deriving
        // the path spellings is for
        let mut list = array(&[&literal]);
        assert!(remove_argument(&mut list, glob));
        assert!(
            entries(&list).is_empty(),
            "the untrack undo stopped working once nothing spelled the argument"
        );

        // the directory spelling is reached the same way, and only when
        // the argument itself is not in the list
        let directory = exclude_rule_for_path("~/.codex/logs[a]", true);
        assert_eq!(directory, "~/.codex/logs[[]a[]]/**");
        let mut list = array(&["~/.codex/logs[a]", &directory]);
        assert!(remove_argument(&mut list, "~/.codex/logs[a]"));
        assert_eq!(entries(&list), vec![directory.clone()]);
        let mut list = array(&[&directory]);
        assert!(remove_argument(&mut list, "~/.codex/logs[a]"));
        assert!(entries(&list).is_empty());
    }

    fn array(entries: &[&str]) -> toml_edit::Array {
        let mut array = toml_edit::Array::new();
        for entry in entries {
            array.push(string(entry));
        }
        array
    }

    fn entries(array: &toml_edit::Array) -> Vec<String> {
        array
            .iter()
            .filter_map(|value| value.as_str().map(str::to_string))
            .collect()
    }

    /// The list is last-match-wins, so an edit appends its rule and drops
    /// the pattern's earlier entries, and reports no change only when
    /// that leaves the list as it was.
    #[test]
    fn an_edit_appends_its_rule_and_reports_a_change_only_when_the_list_moves() {
        let mut list = array(&["foo"]);
        assert!(!append_rule(&mut list, "foo"));
        assert_eq!(entries(&list), ["foo"]);

        // the earlier `foo` is not the rule in force here: `!foo*` is
        let mut list = array(&["foo", "!foo*"]);
        assert!(append_rule(&mut list, "foo"));
        assert_eq!(entries(&list), ["!foo*", "foo"]);

        let mut list = array(&["foo", "!foo"]);
        assert!(append_rule(&mut list, "foo"));
        assert_eq!(entries(&list), ["foo"]);

        // every other rule keeps its place and its meaning
        let mut list = array(&["foo", "*.key", "!foo", "sessions/**"]);
        assert!(append_rule(&mut list, "foo"));
        assert_eq!(entries(&list), ["*.key", "sessions/**", "foo"]);
    }

    /// `mise dot include` takes back a glob `mise dot exclude` wrote, so
    /// it removes that entry rather than negating it.
    #[test]
    fn an_include_removes_the_glob_rather_than_negating_it() {
        let mut list = array(&["*.log", "cache"]);
        assert!(drop_glob(&mut list, "cache"));
        assert_eq!(entries(&list), ["*.log"]);
        assert!(!drop_glob(&mut list, "cache"));
        // a hand-written re-include is left alone
        let mut list = array(&["*.log", "!important.log"]);
        assert!(!drop_glob(&mut list, "important.log"));
        assert_eq!(entries(&list), ["*.log", "!important.log"]);
    }
}

#[cfg(test)]
mod declaration_tests {
    use super::*;

    #[test]
    fn rewritten_declarations_keep_their_own_explicit_policies_only() {
        use crate::system::files::{ExplicitFields, FilePolicy};
        use crate::system::resources::ResourceOrigin;
        let command = DotfilesTrack {
            targets: vec![],
            os: None,
            profile: None,
            no_autosave: false,
            encrypt: false,
            yes: true,
            dry_run: false,
        };
        let mut policy = FilePolicy::for_mode(FileMode::Track);
        policy.explicit = ExplicitFields {
            autosave: true,
            encrypt: true,
            ..Default::default()
        };
        let existing = FileRequest {
            target_raw: "~/.zshrc".into(),
            target: PathBuf::from("/home/test/.zshrc"),
            source: PathBuf::new(),
            content: None,
            mode: FileMode::Track,
            exclude: vec![],
            manifest: None,
            base: PathBuf::from("/home/test"),
            origin: ResourceOrigin {
                config: PathBuf::from("/home/test/.config/mise/config.toml"),
                config_root: PathBuf::from("/home/test/.config/mise"),
                environment: vec![],
                source: None,
            },
            policy,
            variants: vec![],
            enabled: true,
        };
        // this file wrote both fields: they stay written at their values
        let previous = ["mode", "autosave", "encrypt"].map(String::from);
        let table = command.entry(Some(&existing), &previous, None);
        assert_eq!(table.get("autosave").and_then(Value::as_bool), Some(true));
        assert_eq!(table.get("encrypt").and_then(Value::as_bool), Some(false));
        // another layer wrote them (the composed flags say explicit): this
        // file must not pin the inherited values
        let table = command.entry(Some(&existing), &["mode".to_string()], None);
        assert!(table.get("autosave").is_none());
        assert!(table.get("encrypt").is_none());
        let table = command.entry(None, &[], None);
        assert!(table.get("autosave").is_none());
        assert!(table.get("encrypt").is_none());
        // an inherited non-default value is not pinned either; this
        // command's own flag is
        let mut inherited = existing.clone();
        inherited.policy.autosave = false;
        inherited.policy.encrypt = true;
        let table = command.entry(Some(&inherited), &["mode".to_string()], None);
        assert!(table.get("autosave").is_none());
        assert!(table.get("encrypt").is_none());
        let flagged = DotfilesTrack {
            no_autosave: true,
            ..command
        };
        let table = flagged.entry(Some(&inherited), &["mode".to_string()], None);
        assert_eq!(table.get("autosave").and_then(Value::as_bool), Some(false));
        assert!(table.get("encrypt").is_none());
        // an inherited exclude list is not pinned either; one this file
        // wrote is kept, even when empty
        let mut listed = existing.clone();
        listed.exclude = vec![glob::Pattern::new("sessions").unwrap()];
        let table = flagged.entry(Some(&listed), &["mode".to_string()], None);
        assert!(table.get("exclude").is_none());
        let table = flagged.entry(
            Some(&listed),
            &["mode".to_string(), "exclude".to_string()],
            None,
        );
        assert_eq!(
            table
                .get("exclude")
                .and_then(Value::as_array)
                .map(|a| a.len()),
            Some(1)
        );
        // the list is carried as written, so a pattern the loader rejected
        // survives a rewrite
        let raw: Array = "[\"sessions\", \"[\"]"
            .parse::<Value>()
            .unwrap()
            .as_array()
            .cloned()
            .unwrap();
        let table = flagged.entry(
            Some(&listed),
            &["mode".to_string(), "exclude".to_string()],
            Some(raw),
        );
        assert_eq!(
            table
                .get("exclude")
                .and_then(Value::as_array)
                .map(|a| a.len()),
            Some(2)
        );
        let mut cleared = existing.clone();
        cleared.exclude = vec![];
        let table = flagged.entry(
            Some(&cleared),
            &["mode".to_string(), "exclude".to_string()],
            None,
        );
        assert_eq!(
            table
                .get("exclude")
                .and_then(Value::as_array)
                .map(|a| a.len()),
            Some(0)
        );
        // the keys are read from either table form
        for text in [
            "[dotfiles]\n\"~/.zshrc\" = { mode = \"track\", autosave = true }\n",
            "[dotfiles.\"~/.zshrc\"]\nmode = \"track\"\nautosave = true\n",
        ] {
            let doc: DocumentMut = text.parse().unwrap();
            let keys: Vec<String> = doc
                .get("dotfiles")
                .and_then(|dotfiles| dotfiles.get("~/.zshrc"))
                .and_then(Item::as_table_like)
                .map(|table| table.iter().map(|(key, _)| key.to_string()).collect())
                .unwrap_or_default();
            assert_eq!(keys, ["mode", "autosave"].map(String::from));
        }
    }

    #[test]
    fn resolved_sources_use_tracking_path_representation() {
        let temporary = tempfile::tempdir().unwrap();
        let source = temporary.path().join("source");
        std::fs::write(&source, "contents").unwrap();
        let tracked = normalize_target(&source);
        // Windows canonicalization adds a verbatim prefix. No symlink
        // privilege is needed to exercise the resolver's final path format.
        let canonical = source.canonicalize().unwrap();
        assert_eq!(resolve_symlink_source(&canonical).unwrap(), tracked);
    }

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
