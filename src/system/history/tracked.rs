//! The effective tracked set: which paths a capture covers, under which
//! policies, and how they map onto the snapshot tree.
//!
//! mise's own walker decides what is captured — never git's ignore rules —
//! and hands the repository literal file pathspecs. Every file belongs to
//! exactly one entry, the most specific one covering it, whose policies
//! apply. Hard exclusions are only mise's internals: the history store, the
//! mise state/cache/data/installs/downloads/plugins directories, and `.git`
//! directories (a nested repository is captured as a gitlink).
//!
//! Only explicit `mode = "track"` declarations enroll paths. Deployment
//! declarations and symlink targets never enroll files implicitly.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use eyre::Result;
use globset::{Glob, GlobSet, GlobSetBuilder};
use walkdir::WalkDir;

use super::select::{self, Selection};
use super::shadow::{CaptureRoot, MAX_BYTES, MAX_FILE_BYTES, MAX_FILES};
use super::store::{Coverage, CoverageEntry, PathReason};
use crate::config::Config;
use crate::dirs;
use crate::file::{self, display_path};
use crate::system::files::{FileMode, FilePolicy};

/// Credential names excluded from capture by default.
const CREDENTIAL_NAMES: &[&str] = &["github_tokens.toml", "hosts.yml", "age.txt"];
const CREDENTIAL_GLOBS: &[&str] = &[
    ".netrc",
    "*.age",
    "*.key",
    "*.pem",
    "*.gpg",
    "*.kdbx",
    "id_*",
    "*token*",
    "*secret*",
    "credentials*",
    "oauth*",
];

pub(crate) type Policy = FilePolicy;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct TrackedEntry {
    /// Absolute, `~` expanded, lexically normalized.
    pub path: PathBuf,
    /// The explicit tracking declaration's mode.
    pub mode: String,
    pub policy: Policy,
    /// The shared stream of a tracked file with variants.
    pub variant: Option<String>,
    pub declared_in: Option<PathBuf>,
    /// The entry's own `exclude` globs, relative to its path, with the
    /// rules of a deployment entry's list (see
    /// [`crate::system::files::is_excluded`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
}

impl TrackedEntry {
    /// The compiled `exclude` patterns; an invalid one was already
    /// reported when the declaration was read.
    pub(crate) fn exclude_patterns(&self) -> Vec<glob::Pattern> {
        self.exclude
            .iter()
            .filter_map(|pattern| glob::Pattern::new(pattern).ok())
            .collect()
    }

    /// Whether the entry's own `exclude` list drops `path`: a path below
    /// the entry whose entry-relative form matches, as for a deployment
    /// entry. The entry path itself is never excluded by its own list.
    pub(crate) fn is_excluded(&self, path: &Path) -> bool {
        excluded_by_entry(&self.path, &self.exclude, path)
    }

    pub(crate) fn tree_path(&self, path: &Path) -> Result<String> {
        super::sync::layout::Roots::current()
            .branch_path(path, self.variant.as_deref())
            .ok_or_else(|| {
                eyre::eyre!(
                    "cannot represent tracked path {} portably",
                    display_path(path)
                )
            })
    }

    pub(crate) fn display(&self) -> String {
        display_path(&self.path)
    }

    pub(crate) fn new(path: PathBuf, mode: &str, policy: Policy) -> Self {
        Self {
            path,
            mode: mode.to_string(),
            policy,
            variant: None,
            declared_in: None,
            exclude: vec![],
        }
    }
}

/// The resolved tracked set for one capture.
#[derive(Clone, Debug, Default)]
pub(crate) struct TrackedSet {
    pub entries: Vec<TrackedEntry>,
    pub manifest: super::manifest::Manifest,
    /// Current explicit local declarations, before repository reconciliation.
    pub declarations: Option<super::manifest::Manifest>,
    /// Explicitly disabled tracking declarations also remove Git enrollment.
    pub disabled: Vec<PathBuf>,
    /// Deployment inputs to validate, not paths enrolled for observation.
    pub required_sources: Vec<PathBuf>,
    /// `[history] exclude` globs as written (with `~`).
    pub exclude: Vec<String>,
    /// Declarations that could not be honoured, so they are never mistaken
    /// for protection.
    pub invalid: Vec<PathReason>,
}

/// What a walk of the tracked set found.
#[derive(Debug, Default)]
pub(crate) struct Walk {
    pub manifest: super::manifest::Manifest,
    /// The explicit entries as walked.
    pub entries: Vec<TrackedEntry>,
    pub roots: Vec<CaptureRoot>,
    /// Every captured file with the entry that owns it and its policy.
    pub files: BTreeMap<PathBuf, (usize, Policy)>,
    pub omitted: Vec<PathReason>,
    /// The `[history] exclude` rules as they expanded for this walk.
    pub exclude_expanded: Vec<super::store::ExpandedRule>,
    /// Nested repositories, captured as a commit pointer without their files.
    pub nested: Vec<PathReason>,
    pub incomplete: Vec<PathReason>,
    pub warnings: Vec<String>,
}

impl TrackedSet {
    /// The effective tracked set for the loaded configuration.
    pub(crate) async fn effective() -> Result<Self> {
        let config = Config::get().await?;
        let declared = Self::from_config(&config)?;
        if !super::shadow::HistoryRepo::path_in(&dirs::STATE).is_dir() {
            return Ok(declared);
        }
        match super::shadow::HistoryRepo::open_or_init_in(&dirs::STATE)? {
            Some(repo) => super::enrollment::resolve(&dirs::STATE, &repo, &declared, &[], &[]),
            None => Ok(declared),
        }
    }

    /// Explicit tracking from the system and global configuration layers.
    pub(crate) fn from_config(config: &Config) -> Result<Self> {
        let mut set = Self {
            exclude: super::config::exclude_globs()?,
            ..Default::default()
        };
        let requests = crate::system::files::composed_files_from_config(config)?
            .into_iter()
            .filter(|request| crate::system::files::declaration_is_global(config, request));
        set.add_requests(requests);
        for invalid in crate::system::files::invalid_declarations() {
            if !crate::system::files::tracking_config_is_global(config, &invalid.config) {
                continue;
            }
            set.invalid.push(PathReason {
                path: invalid.target,
                reason: format!("{} ({})", invalid.reason, display_path(&invalid.config)),
            });
        }
        set.manifest.exclude = set.exclude.clone();
        if set.manifest.enrollment.iter().any(|entry| entry.encrypt) {
            set.manifest.recipients = super::config::file_recipients()?;
        }
        set.declarations = Some(set.manifest.clone());
        Ok(set)
    }

    /// Add already composed, global declarations. Shared by live discovery
    /// and the read-only incoming-configuration preflight.
    pub(crate) fn add_requests(
        &mut self,
        requests: impl IntoIterator<Item = crate::system::files::FileRequest>,
    ) {
        let set = self;
        let environments = select::active_environments();
        for request in requests {
            if !request.enabled && request.mode == FileMode::Track {
                set.disabled.push(normalize_target(&request.target));
            }
            if request.enabled
                && matches!(
                    request.mode,
                    FileMode::Symlink | FileMode::SymlinkEach | FileMode::Copy | FileMode::Template
                )
            {
                let source = normalize_target(&request.source);
                if !set.required_sources.contains(&source) {
                    set.required_sources.push(source);
                }
            }
            if !request.enabled || request.mode != FileMode::Track {
                continue;
            }
            if let Err(error) = ensure_portable_ancestors(&request.target) {
                set.invalid.push(PathReason {
                    path: display_path(&request.target),
                    reason: error.to_string(),
                });
                continue;
            }
            let target = normalize_target(&request.target);
            let portable =
                if let Ok(relative) = target.strip_prefix(normalize(&global_config_dir())) {
                    Some(
                        format!("config/{}", relative.to_string_lossy().replace('\\', "/"))
                            .trim_end_matches('/')
                            .to_owned(),
                    )
                } else if let Ok(relative) = target.strip_prefix(normalize(&dirs::HOME)) {
                    Some(format!(
                        "home/{}",
                        relative.to_string_lossy().replace('\\', "/")
                    ))
                } else {
                    None
                };
            let Some(path) = portable.filter(|path| super::sync::layout::is_safe_branch_path(path))
            else {
                set.invalid.push(PathReason { path: display_path(&target), reason: "tracking requires a portable path under home or the mise configuration directory".into() });
                continue;
            };
            if let Err(error) = select::validate(&request.variants) {
                set.invalid.push(PathReason {
                    path: display_path(&target),
                    reason: error.to_string(),
                });
                continue;
            }
            set.manifest.enrollment.retain(|entry| entry.path != path);
            set.manifest.enrollment.push(super::manifest::Enrollment {
                path,
                autosave: request.policy.autosave,
                encrypt: request.policy.encrypt,
                variants: request.variants.clone(),
                exclude: request
                    .exclude
                    .iter()
                    .map(|pattern| pattern.as_str().to_owned())
                    .collect(),
            });
            set.manifest.enrollment.sort_by(|a, b| a.path.cmp(&b.path));
            let declared_in = Some(request.origin.config.clone());
            match request.mode {
                FileMode::Track => {
                    let mut entry = TrackedEntry::new(
                        normalize_target(&request.target),
                        "track",
                        request.policy,
                    );
                    entry.declared_in = declared_in;
                    entry.exclude = request
                        .exclude
                        .iter()
                        .map(|pattern| pattern.as_str().to_owned())
                        .collect();
                    match select::select(&request.variants, &environments) {
                        Selection::Single => {}
                        Selection::Variant(variant) => {
                            entry.variant = Some(variant.name());
                        }
                        Selection::NoMatch => continue,
                        Selection::Ambiguous(variants) => {
                            let names: Vec<String> =
                                variants.iter().map(|variant| variant.name()).collect();
                            set.invalid.push(PathReason {
                                path: display_path(&request.target),
                                reason: format!(
                                    "ambiguous variant: {} match this machine equally",
                                    names.join(" and ")
                                ),
                            });
                            continue;
                        }
                    }
                    set.push(entry);
                }
                _ => unreachable!("only explicit tracking requests are enrolled"),
            }
        }
    }

    /// Adds explicit enrollment, rejecting conflicting encryption policies.
    pub(crate) fn push(&mut self, entry: TrackedEntry) {
        if let Some(existing) = self
            .entries
            .iter_mut()
            .find(|existing| existing.path == entry.path)
        {
            if existing.policy.encrypt != entry.policy.encrypt {
                self.invalid.push(PathReason {
                    path: display_path(&entry.path),
                    reason: "overlapping declarations disagree about encryption".into(),
                });
            }
            return;
        }
        self.entries.push(entry);
    }

    /// The most specific entry covering `path`.
    pub(crate) fn entry_for(&self, path: &Path) -> Option<&TrackedEntry> {
        owning_entry(&self.entries, path)
    }

    pub(crate) fn entry_index_for(&self, path: &Path) -> Option<usize> {
        let owner = owning_entry(&self.entries, path)?;
        self.entries
            .iter()
            .position(|entry| std::ptr::eq(entry, owner))
    }

    /// Whether a capture of this set would include `path`: under an entry,
    /// not excluded, not inside mise's own directories or a `.git`.
    pub(crate) fn would_capture(&self, path: &Path) -> Result<bool> {
        if !self.would_retain(path)? {
            return Ok(false);
        }
        if let Ok(meta) = std::fs::symlink_metadata(path)
            && !meta.is_dir()
            && classify_file(&meta).is_err()
        {
            return Ok(false);
        }
        Ok(true)
    }

    /// Enrollment and exclusion policy, independent of current readability or
    /// capture limits. An omitted saved file may be retained, never an excluded one.
    pub(crate) fn would_retain(&self, path: &Path) -> Result<bool> {
        let Some(owner) = self.entry_for(path) else {
            return Ok(false);
        };
        if capture_exclusion(path, &owner.policy).is_some() {
            return Ok(false);
        }
        if hard_exclusions().iter().any(|dir| path.starts_with(dir)) {
            return Ok(false);
        }
        if path
            .components()
            .any(|component| component.as_os_str() == ".git")
        {
            return Ok(false);
        }
        // a nested repository below the entry is a gitlink: nothing under
        // it is captured
        let nested = path
            .ancestors()
            .skip(1)
            .take_while(|ancestor| ancestor.starts_with(&owner.path) && *ancestor != owner.path)
            .any(|ancestor| ancestor.join(".git").exists());
        if nested {
            return Ok(false);
        }
        Ok(!self.excluded_by_lists(&self.exclude_set()?, path))
    }

    /// Whether the exclusion lists drop `path`: the global globs read
    /// against its owning entry's path, then that entry's own list, which
    /// a global `!glob` does not re-include.
    ///
    /// The one composition. `would_retain` adds the filesystem checks a
    /// capture also makes; the watcher asks this alone, because it is
    /// deciding what to watch rather than what a walk found. Both pick
    /// the owner with [`owning_entry`], so they cannot disagree.
    pub(crate) fn excluded_by_lists(&self, exclude: &ExcludeSet, path: &Path) -> bool {
        match self.entry_for(path) {
            Some(owner) => exclude.is_match(path, &owner.path) || owner.is_excluded(path),
            None => true,
        }
    }

    pub(crate) fn exclude_set(&self) -> Result<ExcludeSet> {
        ExcludeSet::new(&self.exclude)
    }

    /// Walks every entry and decides, file by file, what the capture holds.
    pub(crate) fn walk(&self) -> Result<Walk> {
        let set = self;
        let exclude = set.exclude_set()?;
        let hard = hard_exclusions();
        let home = normalize(&dirs::HOME);
        let mut walk = Walk {
            manifest: set.manifest.clone(),
            ..Default::default()
        };
        walk.manifest.exclude = set.exclude.clone();
        walk.exclude_expanded = exclude.expanded().to_vec();
        for (index, entry) in set.entries.iter().enumerate() {
            walk_entry(set, index, entry, &exclude, &hard, &mut walk);
        }
        // Protected files are excluded from capture itself, never kept in
        // a hidden local-only history. Explicit encryption permits key files
        // to be tracked without storing their plaintext.
        walk.files.retain(|path, (_, policy)| {
            if let Some(reason) = capture_exclusion(path, policy) {
                walk.omitted.push(PathReason {
                    path: display_path(path),
                    reason: reason.into(),
                });
                false
            } else {
                true
            }
        });
        walk.entries = set.entries.clone();
        let config = normalize(&global_config_dir());
        let mut roots: BTreeMap<String, CaptureRoot> = BTreeMap::new();
        for (path, (owner, _)) in &walk.files {
            if path.to_str().is_none() {
                eyre::bail!(
                    "history cannot represent a non-UTF-8 filename; refusing to change its bytes"
                );
            }
            let (label, base, relative) = if let Ok(relative) = path.strip_prefix(&config) {
                ("config", config.clone(), relative.to_path_buf())
            } else if let Ok(relative) = path.strip_prefix(&home) {
                ("home", home.clone(), relative.to_path_buf())
            } else {
                (
                    "fs",
                    PathBuf::from(std::path::MAIN_SEPARATOR.to_string()),
                    path.components()
                        .filter(|c| matches!(c, Component::Normal(_)))
                        .collect(),
                )
            };
            let label = walk.entries[*owner]
                .variant
                .as_ref()
                .map_or_else(|| label.to_string(), |variant| format!("{label}@{variant}"));
            let root = roots.entry(label.clone()).or_insert_with(|| CaptureRoot {
                label,
                path: base,
                files: vec![],
                bytes: 0,
            });
            root.files.push(relative);
            // a symlink's length is its target string and a nested
            // repository's its directory entry: neither is captured content
            root.bytes += std::fs::symlink_metadata(path)
                .ok()
                .filter(|m| m.is_file())
                .map_or(0, |m| m.len());
        }
        walk.roots = roots.into_values().collect();
        Ok(walk)
    }

    /// The rules this set captures under, for the checkpoint record.
    pub(crate) fn coverage(&self, walk: &Walk) -> Coverage {
        let entries: Vec<CoverageEntry> = walk
            .entries
            .iter()
            .map(|entry| CoverageEntry {
                path: entry.display(),
                mode: entry.mode.clone(),
                variant: entry.variant.clone(),
                autosave: entry.policy.autosave,
                encrypt: entry.policy.encrypt,
                state: "live".into(),
                declared_in: entry.declared_in.as_deref().map(display_path),
                exclude: entry.exclude.clone(),
            })
            .collect();
        let mut omitted = walk.omitted.clone();
        omitted.extend(self.invalid.iter().cloned());
        Coverage {
            entries,
            exclude: self.exclude.clone(),
            // a coverage built without a walk — a protective checkpoint,
            // say — still has to record what the patterns expanded to
            exclude_expanded: if walk.exclude_expanded.is_empty() {
                self.exclude_set()
                    .map(|exclude| exclude.expanded().to_vec())
                    .unwrap_or_default()
            } else {
                walk.exclude_expanded.clone()
            },
            incomplete: walk.incomplete.clone(),
            omitted,
            nested: walk.nested.clone(),
        }
    }
}

/// Walks one entry, recording its files, omissions, and symlinks.
fn walk_entry(
    set: &TrackedSet,
    index: usize,
    entry: &TrackedEntry,
    exclude: &ExcludeSet,
    hard: &[PathBuf],
    walk: &mut Walk,
) {
    let home = normalize(&dirs::HOME);
    let display = entry.display();
    if is_refused_root(&entry.path, &home) {
        walk.omitted.push(PathReason {
            path: display,
            reason: "refused: the home directory or above".into(),
        });
        return;
    }
    if hard.iter().any(|dir| entry.path.starts_with(dir)) {
        walk.omitted.push(PathReason {
            path: display,
            reason: "mise internal directory".into(),
        });
        return;
    }
    let meta = match std::fs::symlink_metadata(&entry.path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
        Err(err) => {
            walk.omitted.push(PathReason {
                path: display,
                reason: format!("unreadable: {err}"),
            });
            return;
        }
    };
    if !meta.is_dir() {
        // an exclusion wins over a direct declaration as it does over a
        // directory walk: what the user excluded never enters a snapshot.
        if exclude.is_match(&entry.path, &entry.path) {
            return;
        }
        match classify_file(&meta) {
            Ok(_) => {
                walk.files.insert(entry.path.clone(), (index, entry.policy));
            }
            Err(reason) => walk.omitted.push(PathReason {
                path: display,
                reason,
            }),
        }
        return;
    }
    let walker = WalkDir::new(&entry.path)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|candidate| {
            !(candidate.file_type().is_dir()
                && (candidate.file_name() == ".git"
                    || hard.iter().any(|dir| dir == candidate.path())))
        });
    let entry_exclude = entry.exclude_patterns();
    let mut files = 0u64;
    let mut bytes = 0u64;
    let mut walker = walker;
    while let Some(candidate) = walker.next() {
        let candidate = match candidate {
            Ok(candidate) => candidate,
            Err(err) => {
                let path = err
                    .path()
                    .map(display_path)
                    .unwrap_or_else(|| display.clone());
                walk.omitted.push(PathReason {
                    path,
                    reason: format!("unreadable: {err}"),
                });
                continue;
            }
        };
        let path = candidate.path();
        if path == entry.path {
            continue;
        }
        // a more specific entry owns this subtree and walks it itself
        if set
            .entry_index_for(path)
            .is_some_and(|owner| owner != index)
        {
            continue;
        }
        let file_type = candidate.file_type();
        // an excluded directory is not entered at all: `~/.codex/sessions`
        // can hold tens of thousands of files, and none of them can come
        // back into the capture
        if exclude.is_match(path, &entry.path) {
            if file_type.is_dir() && !exclude.may_reinclude_below(path) {
                walker.skip_current_dir();
            }
            continue;
        }
        // the entry's own exclusions: a matching directory is not entered
        if !entry_exclude.is_empty()
            && let Ok(rel) = path.strip_prefix(&entry.path)
            && crate::system::files::is_excluded(rel, &entry_exclude)
        {
            if file_type.is_dir() {
                walker.skip_current_dir();
            }
            continue;
        }
        if file_type.is_dir() {
            if path.join(".git").exists() {
                // captured as a gitlink; never descended into
                walk.files.insert(path.to_path_buf(), (index, entry.policy));
                walk.nested.push(PathReason {
                    path: display_path(path),
                    reason: NESTED_REPOSITORY_REASON.into(),
                });
                files += 1;
                walker.skip_current_dir();
            }
            continue;
        }
        let meta = match candidate.metadata() {
            Ok(meta) => meta,
            Err(err) => {
                walk.omitted.push(PathReason {
                    path: display_path(path),
                    reason: format!("unreadable: {err}"),
                });
                continue;
            }
        };
        match classify_file(&meta) {
            Ok(size) => {
                files += 1;
                bytes += size;
                if files > MAX_FILES || bytes > MAX_BYTES {
                    let reason = format!(
                        "scan stopped after {MAX_FILES} files or {} MiB",
                        MAX_BYTES / (1024 * 1024)
                    );
                    walk.warnings
                        .push(format!("{display}: {reason}; the rest was not captured"));
                    walk.incomplete.push(PathReason {
                        path: display,
                        reason,
                    });
                    return;
                }
                walk.files.insert(path.to_path_buf(), (index, entry.policy));
            }
            Err(reason) => walk.omitted.push(PathReason {
                path: display_path(path),
                reason,
            }),
        }
    }
}

/// Size of a capturable file, or why it is omitted.
fn classify_file(meta: &std::fs::Metadata) -> std::result::Result<u64, String> {
    let file_type = meta.file_type();
    if file_type.is_symlink() {
        return Ok(0);
    }
    if !file_type.is_file() {
        return Err("special file".into());
    }
    let size = meta.len();
    if size > MAX_FILE_BYTES {
        return Err(format!(
            "{} MiB is over the {} MiB limit",
            size / (1024 * 1024),
            MAX_FILE_BYTES / (1024 * 1024)
        ));
    }
    Ok(size)
}

/// Why the credential guard keeps a file out of capture.
pub(crate) const CREDENTIAL_REASON: &str = "credential store; encrypt the file before tracking it";

/// What a capture records for a directory with its own `.git`.
pub(crate) const NESTED_REPOSITORY_REASON: &str =
    "nested repository; its files are not saved, only its commit id";

/// Why `path` is left out of every capture under `policy`, if it is: a
/// machine-local configuration file, or a credential store that is not
/// enrolled with encryption. The guard is deliberately conservative and
/// matches by name alone (`id_ed25519.pub` is protected like its private
/// half); a narrower rule is the user's to add.
pub(crate) fn capture_exclusion(path: &Path, policy: &Policy) -> Option<&'static str> {
    let name = path.file_name()?.to_str()?;
    if name.ends_with(".local.toml") {
        Some("machine-local configuration")
    } else if !policy.encrypt && is_builtin_credential(path, name) {
        Some(CREDENTIAL_REASON)
    } else {
        None
    }
}

/// Whether the builtin rules protect a file of this name at this path.
pub(crate) fn is_builtin_credential(path: &Path, name: &str) -> bool {
    static NAMES: std::sync::LazyLock<GlobSet> = std::sync::LazyLock::new(credential_names);
    static GLOBS: std::sync::LazyLock<GlobSet> =
        std::sync::LazyLock::new(|| glob_set(CREDENTIAL_GLOBS));
    GLOBS.is_match(name)
        || (path.starts_with(normalize(&global_config_dir())) && NAMES.is_match(name))
}

/// How many omissions a capture report lists one by one before it
/// summarizes them and points at `mise dot paths`.
pub(crate) const OMISSION_LINES: usize = 10;

/// Whether `patterns` (an entry's own `exclude` list, relative to
/// `entry_path`) drop `path`; the entry path itself never is.
pub(crate) fn excluded_by_entry(entry_path: &Path, patterns: &[String], path: &Path) -> bool {
    if patterns.is_empty() {
        return false;
    }
    let patterns: Vec<glob::Pattern> = patterns
        .iter()
        .filter_map(|pattern| glob::Pattern::new(pattern).ok())
        .collect();
    match path.strip_prefix(entry_path) {
        Ok(rel) if !rel.as_os_str().is_empty() => crate::system::files::is_excluded(rel, &patterns),
        _ => false,
    }
}

/// Whether the display path `path` is `root` itself or lies below it.
/// Display paths use the platform separator (`\` on Windows), so the
/// boundary is checked on either.
pub(crate) fn display_under(path: &str, root: &str) -> bool {
    path == root
        || path
            .strip_prefix(root)
            .is_some_and(|rest| rest.starts_with(['/', '\\']))
}

/// A tree this large is worth a second look before it is tracked: more
/// files than this, or more bytes, and `mise dot track` warns.
pub(crate) const LARGE_TREE_FILES: usize = 5_000;
pub(crate) const LARGE_TREE_BYTES: u64 = 256 * 1024 * 1024;

impl Walk {
    /// How many files the walk captures.
    pub(crate) fn file_count(&self) -> usize {
        self.roots.iter().map(|root| root.files.len()).sum()
    }

    /// The bytes of the captured files.
    pub(crate) fn bytes(&self) -> u64 {
        self.roots.iter().map(|root| root.bytes).sum()
    }

    /// `22,972 files, 1.2 GiB`.
    pub(crate) fn summary(&self) -> String {
        count_and_size(self.file_count(), self.bytes())
    }
}

/// What tracking one entry of a preview set captures: the files that entry
/// owns (a more specific entry owns its own subtree), and what a save
/// would leave out under it.
#[derive(Debug, Default)]
pub(crate) struct EntryPreview {
    pub files: usize,
    pub bytes: u64,
    pub omitted: Vec<PathReason>,
    pub nested: Vec<PathReason>,
    pub incomplete: Vec<PathReason>,
}

impl EntryPreview {
    pub(crate) fn summary(&self) -> String {
        count_and_size(self.files, self.bytes)
    }

    pub(crate) fn is_large(&self) -> bool {
        self.files > LARGE_TREE_FILES || self.bytes > LARGE_TREE_BYTES
    }
}

impl Walk {
    /// The preview of the entry at `index` of `set`, which this walk was
    /// taken from: nested targets in one command partition instead of the
    /// outer one counting the inner one's files too.
    pub(crate) fn preview_of(&self, set: &TrackedSet, index: usize) -> EntryPreview {
        let mut preview = EntryPreview::default();
        for (path, (owner, _)) in &self.files {
            if *owner != index {
                continue;
            }
            preview.files += 1;
            preview.bytes += std::fs::symlink_metadata(path)
                .ok()
                .filter(|m| m.is_file())
                .map_or(0, |m| m.len());
        }
        let owned = |reported: &PathReason| {
            set.entry_index_for(&file::replace_path(Path::new(&reported.path))) == Some(index)
        };
        preview.omitted = self.omitted.iter().filter(|r| owned(r)).cloned().collect();
        preview.nested = self.nested.iter().filter(|r| owned(r)).cloned().collect();
        let display = set.entries[index].display();
        preview.incomplete = self
            .incomplete
            .iter()
            .filter(|r| r.path == display)
            .cloned()
            .collect();
        preview
    }
}

/// `1 file, 12 B` or `22,972 files, 1.2 GiB`.
pub(crate) fn count_and_size(files: usize, bytes: u64) -> String {
    format!(
        "{} {}, {}",
        with_separators(files),
        if files == 1 { "file" } else { "files" },
        bytesize::ByteSize::b(bytes).display().iec()
    )
}

fn with_separators(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// The set tracking `path` alone would capture, under the `[history]`
/// exclusions: what `mise dot paths --preview` lists and what `mise dot
/// track` sizes up before it writes a declaration.
pub(crate) fn preview_set(path: &Path, policy: Policy) -> Result<TrackedSet> {
    Ok(preview_set_with(
        path,
        policy,
        super::config::exclude_globs()?,
    ))
}

/// [`preview_set`] with the `[history] exclude` globs already read, so a
/// command previewing several paths reads the configuration once.
pub(crate) fn preview_set_with(path: &Path, policy: Policy, exclude: Vec<String>) -> TrackedSet {
    let mut set = TrackedSet {
        exclude,
        ..Default::default()
    };
    set.push(TrackedEntry::new(normalize_target(path), "track", policy));
    set
}

/// The lines a capture reports about what it left out: every omission and
/// nested repository with its reason when there are few, otherwise one
/// summary.
pub(crate) fn omission_report(omitted: &[PathReason], nested: &[PathReason]) -> Vec<String> {
    if omitted.is_empty() && nested.is_empty() {
        vec![]
    } else if omitted.len() + nested.len() <= OMISSION_LINES {
        omitted
            .iter()
            .map(|omitted| format!("omitted: {} ({})", omitted.path, omitted.reason))
            .chain(
                nested
                    .iter()
                    .map(|nested| format!("nested: {} ({})", nested.path, nested.reason)),
            )
            .collect()
    } else {
        vec![omission_summary(omitted, nested)]
    }
}

/// One line naming how many files a capture leaves out and why.
pub(crate) fn omission_summary(omitted: &[PathReason], nested: &[PathReason]) -> String {
    let mut parts = vec![];
    if !omitted.is_empty() {
        let credentials = omitted
            .iter()
            .filter(|omitted| omitted.reason == CREDENTIAL_REASON)
            .count();
        let detail = match credentials {
            0 => String::new(),
            n if n == omitted.len() => " (credential store)".into(),
            n => format!(" ({n} credential store)"),
        };
        parts.push(format!(
            "{} files omitted from capture{detail}",
            omitted.len()
        ));
    }
    if !nested.is_empty() {
        parts.push(format!(
            "{} nested {} not saved",
            nested.len(),
            if nested.len() == 1 {
                "repository"
            } else {
                "repositories"
            }
        ));
    }
    format!("{}; `mise dot paths` lists them", parts.join("; "))
}

/// Credential stores mise itself knows by name; they mean something only
/// under the global configuration directory.
fn credential_names() -> GlobSet {
    glob_set(CREDENTIAL_NAMES)
}

fn glob_set(patterns: &[&str]) -> GlobSet {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        if let Ok(glob) = Glob::new(pattern) {
            builder.add(glob);
        }
    }
    builder.build().expect("static credential globs")
}

/// The `[history] exclude` globs, applied in order with the last match
/// deciding: a `!glob` after a broader glob re-includes what it matches.
#[derive(Debug, Default)]
pub(crate) struct ExcludeSet {
    list: PatternList,
}

impl ExcludeSet {
    pub(crate) fn new(globs: &[String]) -> Result<Self> {
        Ok(Self {
            list: PatternList::new(globs)?,
        })
    }

    /// The rules a checkpoint recorded, read back without parsing them.
    pub(crate) fn from_expanded(rules: &[super::store::ExpandedRule]) -> Result<Self> {
        Ok(Self {
            list: PatternList::from_expanded(rules)?,
        })
    }

    /// Whether `path`, tracked under `root`, is excluded.
    ///
    /// **A path is excluded when the last rule that matches it, or any of
    /// its ancestors down to the tracked root, is an exclusion.** So an
    /// excluded directory takes its contents with it — the reading a
    /// tracked entry's own `exclude` list already has — and a later
    /// `!glob` naming something inside it still re-includes that, because
    /// it is the later rule.
    ///
    /// Because a descendant is excluded by its own ancestor's match,
    /// pruning an excluded directory out of the walk can only ever be a
    /// speed-up: every file under it would have been excluded one by one
    /// anyway, and the walk and the callers that never walk — `mise dot
    /// save <path>`, the watcher — cannot disagree.
    ///
    /// Ancestors stop at the tracked root. A pattern is about what the
    /// user tracks, not about where their home directory happens to live.
    pub(crate) fn is_match(&self, path: &Path, root: &Path) -> bool {
        if self.list.rules.is_empty() {
            return false;
        }
        // Everything is compared as `/`-separated text. A pattern is
        // written that way whatever platform it is read on, a path is not,
        // and normalizing once here is the only place the two forms can
        // fail to line up.
        let relative_path = if path == root {
            path.file_name().map(PathBuf::from)
        } else {
            path.strip_prefix(root).map(Path::to_path_buf).ok()
        }
        .unwrap_or_else(|| path.to_path_buf());
        let mut candidates = vec![];
        for ancestor in path.ancestors() {
            candidates.push(separators(ancestor));
            if ancestor == root {
                break;
            }
        }
        let relative: Vec<String> = relative_path
            .ancestors()
            .filter(|ancestor| !ancestor.as_os_str().is_empty())
            .map(separators)
            .collect();
        let components: Vec<&str> = relative
            .first()
            .map(|relative| {
                relative
                    .split('/')
                    .filter(|component| !component.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        self.list
            .rules
            .iter()
            .rfind(|rule| rule.matches_any(&candidates, &relative, &components))
            .is_some_and(|rule| !rule.negated)
    }

    /// The patterns this list could not evaluate, as written.
    ///
    /// **An exclusion rule that cannot be evaluated never causes a live
    /// file to be deleted.** Warning and carrying on is right where the
    /// risk is capturing something unintended; it is wrong where the
    /// risk is destroying data, so a reader that is about to remove a
    /// file asks this first and leaves the file alone when it is not
    /// empty.
    pub(crate) fn unevaluable(&self) -> &[String] {
        &self.list.unevaluable
    }

    /// The patterns as they expanded here, for a checkpoint to record.
    ///
    /// **A replay reads the recorded expansion and never consults the
    /// environment.** A pattern is written once and read back in whatever
    /// environment a rollback runs in, where the same `$VAR` may be
    /// unset, empty, or simply different — and nothing at replay time can
    /// tell "still means what it meant" from "means something else now".
    /// Recording what was actually in effect removes the question.
    pub(crate) fn expanded(&self) -> &[super::store::ExpandedRule] {
        &self.list.expanded
    }

    /// Whether any `!pattern` could re-include something below `dir`.
    ///
    /// **A matching directory is pruned from the walk only when no
    /// negated pattern could re-include anything beneath it. When one
    /// could, the directory is walked and its files are filtered
    /// individually.** Pruning is an optimization; last-match-wins is the
    /// semantics, and the semantics win. A list with no negations — which
    /// is almost every list — keeps the whole saving.
    ///
    /// Deliberately conservative: a negated pattern that names no
    /// directory of its own, or one whose directory overlaps `dir` in
    /// either direction, disables pruning for that subtree.
    pub(crate) fn may_reinclude_below(&self, dir: &Path) -> bool {
        self.list
            .rules
            .iter()
            .filter(|rule| rule.negated)
            .any(|rule| {
                if rule.anchor != Anchor::Absolute || rule.reinclude_roots.is_empty() {
                    return true;
                }
                rule.reinclude_roots
                    .iter()
                    .any(|root| root.starts_with(dir) || dir.starts_with(root))
            })
    }
}

/// How a `[history]` pattern list is matched.
///
/// **A pattern with no path separator matches any single path
/// component, so `cache` matches a file named `cache` and everything
/// inside a directory named `cache`. A pattern that is absolute after `~` and environment expansion matches
/// the file's absolute path, both as written and normalized exactly as
/// tracked paths are, so it still matches through a symlinked ancestor.
/// Any other pattern is relative: it matches at any depth, as if written
/// with a leading `**/`, and is never resolved against the working
/// directory. A leading `./` is ignored. On Windows both `/` and `\`
/// count as separators.**
///
/// Within the list the last matching pattern decides and a leading `!`
/// negates, so `!glob` re-includes what an earlier glob excluded.
///
/// Each case answers a way the previous matcher failed. An absolute
/// pattern has to be normalized the way the walk normalizes what it
/// captures, or a pattern naming the live `~/…` location silently misses
/// whenever an ancestor is a symlink. `\` counts as a separator on
/// Windows because that is what `display_path` writes, so a path copied
/// out of mise's own output would otherwise be read back as a file-name
/// glob. A relative pattern is neither resolved nor left to fail:
/// resolving it would bind `**/*.log` to whichever directory the command
/// ran in, and matching it as written against an absolute path would
/// make `keys/**` match nothing at all while the config looked correct.
/// A leading `./` is ignored, so `./keys/**` means what `keys/**` means.
///
/// This is the matcher for the global `[history] exclude` list. A
/// tracked entry's own `exclude` list is a separate thing — patterns
/// there are relative to the entry and use
/// [`crate::system::files::is_excluded`] — but both read a
/// separator-free pattern as naming any path component, so the two lists
/// agree about what `cache` means.
///
/// The builtin credential heuristic deliberately does not read patterns
/// this way: [`is_builtin_credential`] tests the file name alone. The
/// two questions are different. Exclusion asks whether a *path* should
/// be saved, which a directory can answer for everything below it.
/// The guard asks whether a *file* is a credential store, which is a
/// property of its own name — a directory called `oauth` or
/// `token-cache` says nothing about the files inside it.
#[derive(Debug, Default)]
pub(crate) struct PatternList {
    rules: Vec<PatternRule>,
    /// Each evaluable rule as it expanded, negation carried as a field
    /// and the pattern opaque. A checkpoint records these so a replay
    /// never has to consult the environment it happens to be running in,
    /// and never has to parse a pattern back out of a string.
    expanded: Vec<super::store::ExpandedRule>,
    /// Patterns that could not be evaluated at all, as written. A rule
    /// naming an environment variable that is not set is left out of
    /// `rules` — expanding it to nothing would make it match far more
    /// than the user named — but it is remembered here, because a reader
    /// that is deciding whether to delete a live file has to know the
    /// list is incomplete rather than assume it is empty.
    unevaluable: Vec<String>,
}

/// What a pattern is matched against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Anchor {
    /// No path separator: any single component of the root-relative path.
    Name,
    /// Absolute after expansion: the path itself or an ancestor of it.
    Absolute,
    /// Anything else: the root-relative path, at any depth below the
    /// tracked root. Never the absolute path — a `sessions` directory
    /// somewhere above the tracked root is not what `sessions/**` means.
    Relative,
}

#[derive(Debug)]
struct PatternRule {
    matchers: Vec<globset::GlobMatcher>,
    /// For a negated path rule, the directories its matches must live
    /// under: each glob's literal leading prefix, cut at the last
    /// separator. Empty means the rule could match anywhere.
    reinclude_roots: Vec<PathBuf>,
    negated: bool,
    anchor: Anchor,
}

impl PatternRule {
    /// Compiles one already-expanded pattern. `negated` is given, never
    /// read out of the text: the pattern is opaque from here on.
    fn compile(body: &str, negated: bool) -> Result<Self> {
        let anchor = if !is_path_anchored(body) {
            Anchor::Name
        } else if file::replace_path(Path::new(body)).is_absolute() {
            Anchor::Absolute
        } else {
            Anchor::Relative
        };
        let globs = if anchor == Anchor::Name {
            vec![body.to_string()]
        } else {
            anchored_globs(body)
        };
        let mut matchers = vec![];
        for glob in &globs {
            // a path glob is gitignore-like: `*` stops at a separator,
            // `**` crosses them. A name glob matches one component, so
            // there is no separator in it to stop at.
            let compiled = if anchor != Anchor::Name {
                globset::GlobBuilder::new(glob)
                    .literal_separator(true)
                    .build()?
            } else {
                Glob::new(glob)?
            };
            matchers.push(compiled.compile_matcher());
        }
        let reinclude_roots = if negated && anchor == Anchor::Absolute {
            globs.iter().filter_map(|glob| literal_root(glob)).collect()
        } else {
            vec![]
        };
        Ok(Self {
            matchers,
            reinclude_roots,
            negated,
            anchor,
        })
    }

    /// Whether the rule matches any of `candidates` — the path and its
    /// ancestors down to the tracked root — or, for a name pattern, any
    /// component of the path relative to that root.
    /// `candidates` are the path and its ancestors down to the tracked
    /// root, `relative` the path relative to that root, and `components`
    /// its components — all already written with `/`, because that is
    /// what the patterns were compiled to.
    fn matches_any(&self, candidates: &[String], relative: &[String], components: &[&str]) -> bool {
        match self.anchor {
            Anchor::Absolute => candidates.iter().any(|c| self.is_glob_match(c)),
            Anchor::Relative => relative.iter().any(|r| self.is_glob_match(r)),
            Anchor::Name => components.iter().any(|c| self.is_glob_match(c)),
        }
    }

    fn is_glob_match(&self, candidate: &str) -> bool {
        self.matchers
            .iter()
            .any(|matcher| matcher.is_match(Path::new(candidate)))
    }
}

/// Expands `$VAR`, `${VAR}` and a leading `~` in a pattern body, before
/// anything is decided about it. Classification depends on the expanded
/// form — `$HOME/.config/app/**` has to be read as the absolute path it
/// names, not as a relative pattern that would match at any depth.
///
/// `None` means a variable in the pattern is not set. The rule is then
/// left out of the list entirely rather than expanded to nothing:
/// `$UNSET/keys/**` would otherwise become `/keys/**`, matching paths the
/// user never named.
fn expand_pattern(body: &str) -> Option<String> {
    let mut out = String::new();
    let mut rest = body;
    while let Some(index) = rest.find('$') {
        out.push_str(&rest[..index]);
        let after = &rest[index + 1..];
        // `$$` is a literal `$`, so a path like `report$$Q1.json` can be
        // written at all; without it an unset `Q1` would drop the rule and
        // a set one would silently retarget it
        if let Some(after) = after.strip_prefix('$') {
            out.push('$');
            rest = after;
            continue;
        }
        let (name, tail) = match after.strip_prefix('{') {
            Some(braced) => match braced.find('}') {
                Some(end) => (&braced[..end], &braced[end + 1..]),
                // an unclosed `${` is not a variable reference
                None => {
                    out.push('$');
                    rest = after;
                    continue;
                }
            },
            None => {
                let end = after
                    .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                    .unwrap_or(after.len());
                (&after[..end], &after[end..])
            }
        };
        if name.is_empty() {
            out.push('$');
            rest = after;
            continue;
        }
        // an empty value is not a value: expanding it in place would turn
        // `$EMPTY/keys/**` into `/keys/**`, which matches from the
        // filesystem root. Treated exactly like an unset variable.
        let value = std::env::var(name).ok().filter(|value| !value.is_empty())?;
        out.push_str(&value);
        rest = tail;
    }
    out.push_str(rest);
    Some(
        file::replace_path(Path::new(&out))
            .to_string_lossy()
            .into_owned(),
    )
}

/// Whether a pattern names a path rather than a file name.
fn is_path_anchored(body: &str) -> bool {
    body.contains('/') || (cfg!(windows) && body.contains('\\'))
}

/// The glob texts a path-anchored pattern is compiled from: the path as
/// written, and — when the two differ — its normalized form.
///
/// Both are needed. Normalizing is what lets a pattern naming the live
/// `~/…` location match a file the walk reached through a symlinked
/// ancestor. Keeping the form as written is what lets the same pattern
/// match a path that was never normalized, which is every path a caller
/// hands to [`TrackedSet::would_retain`] and every entry pushed with a
/// raw path — a `/tmp` that is really `/private/tmp`, a Windows 8.3
/// short name. Matching either is right for a list of globs where the
/// two spellings name the same file.
fn anchored_globs(body: &str) -> Vec<String> {
    let expanded = Path::new(body);
    if !expanded.is_absolute() {
        // a leading `./` says nothing a relative pattern does not already
        // say, and keeping it would produce `**/./keys/**`, which matches
        // nothing at all
        let text = separators(
            &expanded
                .components()
                .filter(|component| !matches!(component, std::path::Component::CurDir))
                .collect::<PathBuf>(),
        );
        return vec![if text.starts_with("**/") {
            text
        } else {
            format!("**/{text}")
        }];
    }
    let written = separators(expanded);
    let normalized = separators(&normalize_target(expanded));
    if normalized == written {
        vec![written]
    } else {
        vec![written, normalized]
    }
}

/// The directory a glob's matches must live under: its literal leading
/// part, cut at the last separator. `None` when the glob starts with a
/// metacharacter, and so could match anywhere.
fn literal_root(glob: &str) -> Option<PathBuf> {
    let literal = match glob.find(['*', '?', '[', '{']) {
        Some(index) => &glob[..index],
        None => glob,
    };
    let root = literal.rsplit_once('/').map(|(root, _)| root)?;
    (!root.is_empty()).then(|| PathBuf::from(root))
}

/// A path as the patterns are written: `/`-separated on every platform.
fn separators(path: &Path) -> String {
    let text = path.to_string_lossy().into_owned();
    if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        text
    }
}

impl PatternList {
    /// A list already expanded and recorded, read back without parsing:
    /// each rule's negation comes from its own field and its pattern is
    /// used as it stands.
    pub(crate) fn from_expanded(rules: &[super::store::ExpandedRule]) -> Result<Self> {
        let mut compiled = vec![];
        for rule in rules {
            compiled.push(PatternRule::compile(&rule.pattern, rule.negated)?);
        }
        Ok(Self {
            rules: compiled,
            expanded: rules.to_vec(),
            unevaluable: vec![],
        })
    }

    pub(crate) fn new(patterns: &[String]) -> Result<Self> {
        let mut rules = vec![];
        let mut unevaluable = vec![];
        let mut expanded = vec![];
        for pattern in patterns {
            let (body, negated) = match pattern.strip_prefix('!') {
                Some(rest) => (rest, true),
                None => (pattern.as_str(), false),
            };
            // expand first: everything else about the pattern — whether it
            // is anchored, whether it is absolute — is a fact about the
            // path it names, not about the text the user typed
            let Some(body) = expand_pattern(body) else {
                warn!(
                    "history: ignoring pattern {pattern:?}: it names an environment variable that is not set"
                );
                unevaluable.push(pattern.clone());
                continue;
            };
            expanded.push(super::store::ExpandedRule {
                negated,
                pattern: body.clone(),
            });
            rules.push(PatternRule::compile(&body, negated)?);
        }
        Ok(Self {
            rules,
            expanded,
            unevaluable,
        })
    }
}

/// Directories mise owns that are never captured.
pub(crate) fn hard_exclusions() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = [
        *dirs::STATE,
        *dirs::CACHE,
        *dirs::DATA,
        *dirs::INSTALLS,
        *dirs::DOWNLOADS,
        *dirs::PLUGINS,
    ]
    .into_iter()
    .map(normalize)
    .collect();
    dirs.push(normalize(&super::store::store_dir_in(&dirs::STATE)));
    dirs.push(normalize(&global_config_dir()).join(".mise-history"));
    dirs.extend(
        crate::agecrypt::identity_paths()
            .iter()
            .map(|path| normalize(path)),
    );
    dirs.sort();
    dirs.dedup();
    dirs
}

/// The global config directory (where `--adopt` checks out).
pub(crate) fn global_config_dir() -> PathBuf {
    crate::env::MISE_GLOBAL_CONFIG_FILE
        .as_deref()
        .map(|path| {
            path.parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf()
        })
        .unwrap_or_else(|| dirs::CONFIG.to_path_buf())
}

/// Canonical when the path exists, lexically normalized otherwise.
pub(crate) fn normalize(path: &Path) -> PathBuf {
    let expanded = file::replace_path(path);
    dunce::canonicalize(&expanded).unwrap_or_else(|_| lexical(&expanded))
}

/// Resolve existing ancestors consistently even when the leaf is missing.
/// A symlink leaf is tracked as a link, never as its destination.
pub(crate) fn normalize_target(path: &Path) -> PathBuf {
    let expanded = file::replace_path(path);
    if !file::is_symlink_or_junction(&expanded)
        && let Ok(resolved) = dunce::canonicalize(&expanded)
    {
        return lexical(&resolved);
    }
    let mut tail = Vec::new();
    let mut ancestor = expanded.as_path();
    if let (Some(parent), Some(name)) = (ancestor.parent(), ancestor.file_name()) {
        tail.push(name.to_os_string());
        ancestor = parent;
    }
    loop {
        let candidate = if ancestor.as_os_str().is_empty() {
            Path::new(".")
        } else {
            ancestor
        };
        if let Ok(mut resolved) = dunce::canonicalize(candidate) {
            for component in tail.iter().rev() {
                resolved.push(component);
            }
            return lexical(&resolved);
        }
        let (Some(parent), Some(name)) = (ancestor.parent(), ancestor.file_name()) else {
            return lexical(&expanded);
        };
        tail.push(name.to_os_string());
        ancestor = parent;
    }
}

/// The home directory or above: never walked, never watched.
pub(crate) fn is_refused_root(path: &Path, home: &Path) -> bool {
    path == home || home.starts_with(path) || path.parent().is_none()
}

fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// Turns a snapshot-tree path (`home/.zshrc`, `fs/etc/hosts`) into the
/// display form (`~/.zshrc`, `/etc/hosts`).
pub(crate) fn tree_path_to_display(tree_path: &str) -> String {
    let (stem, rest) = tree_path.split_once('/').unwrap_or((tree_path, ""));
    let root = stem.split('@').next().unwrap_or(stem);
    if root == "config" {
        display_path(global_config_dir().join(rest))
    } else if root == "home" {
        format!("~/{rest}")
    } else if let Some(rest) = tree_path.strip_prefix("fs/") {
        format!("/{rest}")
    } else {
        tree_path.to_string()
    }
}

/// Root aliases are portable through the home/config mapping. Aliases below
/// those roots are not: canonicalizing them would silently change the enrolled
/// destination on another machine. The leaf itself may still be a symlink.
/// The entry that owns `path`: the most specific one it lies under.
///
/// One rule, one place. Capture, `mise dot save <path>`, the watcher and
/// a replay all have to agree about which entry owns a file, because
/// each reads that entry's own lists and uses its path as the root the
/// global patterns are measured against. Ranking by byte length instead
/// of component count picks a different entry whenever a shallower path
/// simply has a longer name, and the two readings then disagree about
/// what a checkpoint covers.
pub(crate) fn owning_entry<'a>(
    entries: &'a [TrackedEntry],
    path: &Path,
) -> Option<&'a TrackedEntry> {
    entries
        .iter()
        .filter(|entry| path.starts_with(&entry.path))
        .max_by_key(|entry| entry.path.components().count())
}

/// The most specific of `items` that `path` lies under, for the display
/// paths a checkpoint or a manifest records. The same rule as
/// [`owning_entry`], counting components rather than bytes.
pub(crate) fn owning_display<'a, T: 'a>(
    items: impl IntoIterator<Item = &'a T>,
    path: &str,
    key: impl Fn(&T) -> &str,
) -> Option<&'a T> {
    items
        .into_iter()
        .filter(|item| display_under(path, key(item)))
        .max_by_key(|item| Path::new(key(item)).components().count())
}

/// The permission key under which the enrollment this machine selects
/// governs a path: the stream of the closest enrolled path at or above it,
/// else, for a directory with enrolled paths inside it, the variant-less
/// containing key; else none. Every reader of directory modes goes through
/// this, so what a pull applies, what a checkpoint records, and what the
/// next pull assumes are one definition.
pub(crate) fn governing_key(
    roots: &super::sync::layout::Roots,
    entries: &[TrackedEntry],
    path: &Path,
) -> Option<String> {
    let owner = owning_entry(entries, path);
    match owner {
        Some(entry) => roots.branch_path(path, entry.variant.as_deref()),
        None if entries
            .iter()
            .any(|entry| entry.path.starts_with(path) && entry.path != path) =>
        {
            roots.branch_path(path, None)
        }
        None => None,
    }
}

/// The mode a manifest wants for a directory: its record under the
/// governing key, absent meaning the default; nothing when no enrollment
/// this machine selects covers the directory.
pub(crate) fn mode_from(
    roots: &super::sync::layout::Roots,
    entries: &[TrackedEntry],
    permissions: &BTreeMap<String, u32>,
    path: &Path,
) -> Option<u32> {
    let key = governing_key(roots, entries, path)?;
    Some(permissions.get(&key).copied().unwrap_or(0o755))
}

pub(crate) fn ensure_portable_ancestors(path: &Path) -> Result<()> {
    eyre::ensure!(
        path.to_str().is_some(),
        "tracking does not support non-UTF-8 filenames"
    );
    let roots = super::sync::layout::Roots::current();
    let bases = [
        dirs::HOME.to_path_buf(),
        global_config_dir(),
        roots.home,
        roots.config_dir,
    ];
    let relative = |base: &Path| {
        let mut components = path.components();
        for expected in base.components() {
            let actual = components.next()?;
            if actual != expected
                && !(cfg!(windows)
                    && actual
                        .as_os_str()
                        .to_string_lossy()
                        .eq_ignore_ascii_case(&expected.as_os_str().to_string_lossy()))
            {
                return None;
            }
        }
        Some(components.collect::<PathBuf>())
    };
    let Some((base, rest)) = bases
        .iter()
        .filter_map(|base| relative(base).map(|rest| (base, rest)))
        .max_by_key(|(base, _)| base.components().count())
    else {
        eyre::bail!(
            "tracking requires a portable path under home or the mise configuration directory"
        );
    };
    let mut ancestor = base.clone();
    for component in rest
        .components()
        .take(rest.components().count().saturating_sub(1))
    {
        ancestor.push(component);
        if file::is_symlink_or_junction(&ancestor) {
            eyre::bail!(
                "cannot track {} through symlinked parent {}; explicitly track the link itself and its real target instead",
                display_path(path),
                display_path(&ancestor)
            );
        }
    }
    Ok(())
}

/// Turns a display or absolute path into its snapshot-tree path.
pub(crate) fn display_to_tree_path(path: &str) -> String {
    // the link itself, never its destination: a tracked symlink is captured
    // as a link and addressed as one
    let expanded = normalize_target(Path::new(path));
    let config = normalize(&global_config_dir());
    if let Ok(relative) = expanded.strip_prefix(config) {
        return format!("config/{}", relative.to_string_lossy().replace('\\', "/"))
            .trim_end_matches('/')
            .to_owned();
    }
    let home = normalize(&dirs::HOME);
    match expanded.strip_prefix(&home) {
        Ok(rel) if !rel.as_os_str().is_empty() => {
            format!("home/{}", rel.to_string_lossy().replace('\\', "/"))
        }
        Ok(_) => "home".to_string(),
        Err(_) => {
            let rel: Vec<String> = expanded
                .components()
                .filter_map(|component| match component {
                    Component::Normal(part) => Some(part.to_string_lossy().to_string()),
                    _ => None,
                })
                .collect();
            format!("fs/{}", rel.join("/"))
        }
    }
}

#[cfg(test)]
mod tests {
    /// Tracked root for the matcher tests: ancestors stop here.
    const ROOT: &str = "/nonexistent-mise-test";
    use super::*;

    #[cfg(unix)]
    #[test]
    fn enrollment_rejects_alias_parents_but_allows_links_and_real_targets() -> Result<()> {
        let roots = super::super::sync::layout::Roots::current();
        let temp = tempfile::tempdir_in(&roots.home)?;
        let real = temp.path().join("real");
        let alias = temp.path().join("alias");
        std::fs::create_dir(&real)?;
        std::fs::write(real.join("config"), "native")?;
        std::os::unix::fs::symlink("real", &alias)?;
        assert!(ensure_portable_ancestors(&alias).is_ok());
        assert!(ensure_portable_ancestors(&real.join("config")).is_ok());
        assert!(ensure_portable_ancestors(&alias.join("config")).is_err());
        assert!(ensure_portable_ancestors(&alias.join("missing/child")).is_err());
        let manifest = super::super::manifest::Manifest {
            enrollment: vec![super::super::manifest::Enrollment {
                path: roots.branch_path(&alias.join("config"), None).unwrap(),
                autosave: true,
                encrypt: false,
                variants: vec![],
                exclude: vec![],
            }],
            ..Default::default()
        };
        assert!(manifest.tracking().is_err());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn missing_descendants_keep_their_resolved_identity() {
        let temp = tempfile::tempdir().unwrap();
        let real = temp.path().join("real");
        std::fs::create_dir_all(real.join("nested")).unwrap();
        let alias = temp.path().join("alias");
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        let path = alias.join("nested/file");
        std::fs::write(&path, "contents").unwrap();
        let before = normalize_target(&path);
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(real.join("nested")).unwrap();
        assert_eq!(normalize_target(&path), before);
        assert_eq!(
            normalize_target(&alias),
            normalize(temp.path()).join("alias")
        );
    }

    fn entry(path: &Path) -> TrackedEntry {
        TrackedEntry::new(
            path.to_path_buf(),
            "track",
            Policy::for_mode(FileMode::Track),
        )
    }

    #[test]
    fn the_most_specific_entry_owns_a_file() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        let child = root.join("child");
        let mut set = TrackedSet::default();
        set.push(entry(&root));
        set.push(entry(&child));
        assert_eq!(
            set.entry_for(&child.join("file")).map(|e| &e.path),
            Some(&child)
        );
        assert_eq!(
            set.entry_for(&root.join("other")).map(|e| &e.path),
            Some(&root)
        );
        assert!(set.entry_for(&tmp.path().join("elsewhere")).is_none());
        // Repeating an enrollment does not create another owner.
        let mut set = TrackedSet::default();
        set.push(entry(&root));
        set.push(entry(&root));
        assert_eq!(set.entries.len(), 1);
        let mut encrypted = entry(&root);
        encrypted.policy.encrypt = true;
        set.push(encrypted);
        assert_eq!(set.invalid.len(), 1);
    }

    #[test]
    fn existing_leaf_uses_the_filesystem_canonical_spelling() {
        let temp = tempfile::tempdir().unwrap();
        let actual = temp.path().join("MixedCase");
        std::fs::write(&actual, "contents").unwrap();
        let alternative = temp.path().join("mixedcase");
        // This exercises case folding only on filesystems which provide it.
        if alternative.exists() {
            assert_eq!(normalize_target(&alternative), normalize_target(&actual));
        }
    }

    #[cfg(windows)]
    #[test]
    fn junction_leaf_keeps_its_enrolled_location() {
        let temp = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(temp.path()).unwrap();
        let target = root.join("target");
        let link = root.join("junction");
        std::fs::create_dir(&target).unwrap();
        junction::create(&target, &link).unwrap();
        assert_eq!(normalize_target(&link), link);
        assert_ne!(normalize_target(&link), target);
    }

    #[test]
    fn deployment_requests_do_not_enroll_files_or_sources() {
        use crate::system::files::FileRequest;
        use crate::system::resources::ResourceOrigin;

        let tmp = tempfile::tempdir().unwrap();
        let mut set = TrackedSet::default();
        for mode in [
            FileMode::Copy,
            FileMode::Template,
            FileMode::Content,
            FileMode::Symlink,
            FileMode::SymlinkEach,
        ] {
            set.add_requests([FileRequest {
                target_raw: tmp.path().join("output").display().to_string(),
                target: tmp.path().join("output"),
                source: tmp.path().join("source"),
                content: None,
                mode,
                exclude: vec![],
                manifest: None,
                base: tmp.path().to_path_buf(),
                origin: ResourceOrigin {
                    config: tmp.path().join("config.toml"),
                    config_root: tmp.path().to_path_buf(),
                    environment: vec![],
                    source: None,
                },
                policy: Policy::for_mode(mode),
                variants: vec![],
                enabled: true,
            }]);
        }
        assert!(set.entries.is_empty());
        assert_eq!(
            set.required_sources,
            vec![normalize_target(&tmp.path().join("source"))]
        );
        assert!(set.walk().unwrap().files.is_empty());
    }

    #[test]
    fn enrolled_directories_include_new_descendants_but_not_credentials() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("templates");
        std::fs::create_dir_all(&root).unwrap();
        let mut set = TrackedSet::default();
        set.push(entry(&root));
        assert!(set.walk().unwrap().files.is_empty());
        let child = root.join("nested");
        std::fs::create_dir_all(&child).unwrap();
        let included = child.join("gitconfig.tera");
        std::fs::write(&included, "template").unwrap();
        std::fs::write(child.join("config.local.toml"), "private").unwrap();
        std::fs::write(child.join("credentials.json"), "private").unwrap();
        let walk = set.walk().unwrap();
        assert_eq!(walk.files.len(), 1);
        assert!(walk.files.contains_key(&included));
        assert_eq!(walk.omitted.len(), 2);
        assert!(set.would_capture(&included).unwrap());
        assert!(!set.would_capture(&child.join("config.local.toml")).unwrap());
        assert!(!set.would_capture(&child.join("credentials.json")).unwrap());
    }

    #[test]
    fn the_credential_guard_matches_by_name_alone() {
        let policy = Policy::for_mode(FileMode::Track);
        let dir = Path::new("/nonexistent-mise-test/.ssh");
        // conservative by name: a public half or a recipient list is
        // protected like the private half; un-protecting is the user's call
        for name in [
            "id_ed25519",
            "id_ed25519.pub",
            "secrets.fish",
            "client_secret.pub",
            "oauth_token.pub",
            "credentials.pub",
        ] {
            assert_eq!(
                capture_exclusion(&dir.join(name), &policy),
                Some(CREDENTIAL_REASON),
                "{name}"
            );
        }
        assert_eq!(
            capture_exclusion(&dir.join("recipients.txt"), &policy),
            None
        );
        assert_eq!(
            capture_exclusion(&dir.join("config.local.toml"), &policy),
            Some("machine-local configuration")
        );
        let mut encrypted = policy;
        encrypted.encrypt = true;
        assert_eq!(capture_exclusion(&dir.join("id_ed25519"), &encrypted), None);
    }

    /// A pattern is matched against the path only when it holds a
    /// separator, and `\` is one only where the platform writes it.
    #[test]
    fn a_pattern_is_anchored_only_when_it_holds_a_separator() {
        assert!(is_path_anchored("~/.config/app/**"));
        assert!(is_path_anchored("keys/*.pem"));
        assert!(!is_path_anchored("*.pem"));
        assert!(!is_path_anchored("cache"));
        assert_eq!(is_path_anchored("~\\.config\\app"), cfg!(windows));
    }

    /// A path glob is gitignore-like: `*` stops at a separator and `**`
    /// crosses them, so `keys/*.pem` is one directory deep.
    #[test]
    fn a_star_in_a_path_pattern_stops_at_a_separator() {
        let root = Path::new(ROOT);
        let set = ExcludeSet::new(&["keys/*.pem".to_string()]).unwrap();
        assert!(set.is_match(&root.join("app/keys/a.pem"), root));
        assert!(!set.is_match(&root.join("app/keys/sub/a.pem"), root));
        let set = ExcludeSet::new(&["keys/**/*.pem".to_string()]).unwrap();
        assert!(set.is_match(&root.join("app/keys/a.pem"), root));
        assert!(set.is_match(&root.join("app/keys/sub/a.pem"), root));
    }

    /// A pattern with no separator matches any single path component, so
    /// it takes a matching directory's contents with it — the same
    /// reading a tracked entry's own `exclude` list already has.
    #[test]
    fn a_name_glob_excludes_any_path_component() {
        let set = ExcludeSet::new(&["cache".to_string()]).unwrap();
        assert!(set.is_match(
            Path::new("/nonexistent-mise-test/.codex/cache"),
            Path::new(ROOT)
        ));
        assert!(set.is_match(
            Path::new("/nonexistent-mise-test/.codex/cache/index"),
            Path::new(ROOT)
        ));
        assert!(set.is_match(
            Path::new("/nonexistent-mise-test/cache/deep/index"),
            Path::new(ROOT)
        ));
        assert!(!set.is_match(
            Path::new("/nonexistent-mise-test/.codex/config.toml"),
            Path::new(ROOT)
        ));
        assert!(!set.is_match(
            Path::new("/nonexistent-mise-test/.codex/cached"),
            Path::new(ROOT)
        ));
        let set = ExcludeSet::new(&["*.log".to_string()]).unwrap();
        assert!(set.is_match(
            Path::new("/nonexistent-mise-test/a/b/run.log"),
            Path::new(ROOT)
        ));
        // the last matching pattern decides, and `!` re-includes
        let set = ExcludeSet::new(&["cache".to_string(), "!cache".to_string()]).unwrap();
        assert!(!set.is_match(
            Path::new("/nonexistent-mise-test/.codex/cache"),
            Path::new(ROOT)
        ));
        // the guard asks a different question and keeps reading names only
        assert!(!is_builtin_credential(
            Path::new("/nonexistent-mise-test/oauth/notes.txt"),
            "notes.txt"
        ));
    }

    /// An excluded directory is not enumerated. `~/.codex/sessions` can
    /// hold tens of thousands of files and none of them can re-enter the
    /// capture, so reading the directory at all is wasted work. An
    /// unreadable directory makes the difference visible: descending into
    /// it records an `unreadable` omission, skipping it records nothing.
    #[cfg(unix)]
    #[test]
    fn an_excluded_directory_is_not_descended_into() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("codex");
        std::fs::create_dir_all(root.join("sessions")).unwrap();
        std::fs::write(root.join("config.toml"), "keep").unwrap();
        std::fs::write(root.join("sessions/one.jsonl"), "drop").unwrap();
        std::fs::set_permissions(
            root.join("sessions"),
            std::fs::Permissions::from_mode(0o000),
        )
        .unwrap();
        if std::fs::read_dir(root.join("sessions")).is_ok() {
            // running as root, where the permission says nothing
            return;
        }
        let mut set = TrackedSet {
            exclude: vec!["sessions".to_string()],
            ..Default::default()
        };
        set.push(entry(&root));
        let walk = set.walk().unwrap();
        assert_eq!(walk.files.len(), 1);
        assert!(walk.files.contains_key(&root.join("config.toml")));
        assert!(walk.omitted.is_empty(), "{:?}", walk.omitted);
        std::fs::set_permissions(
            root.join("sessions"),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
    }

    /// A relative pattern matches at any depth and means the same thing
    /// wherever the command runs: never bound to `$PWD`, and never left
    /// matching nothing because the path it is compared with is
    /// absolute.
    #[test]
    fn a_relative_pattern_matches_at_any_depth_and_ignores_the_working_directory() {
        let cwd = std::env::current_dir().unwrap();
        let log = ExcludeSet::new(&["**/*.log".to_string()]).unwrap();
        assert!(log.is_match(
            Path::new("/nonexistent-mise-test/a/b/c.log"),
            Path::new(ROOT)
        ));
        assert!(log.is_match(&cwd.join("a/b/c.log"), Path::new(ROOT)));
        assert!(!log.is_match(
            Path::new("/nonexistent-mise-test/a/b/c.txt"),
            Path::new(ROOT)
        ));

        // `sessions/**` used to match nothing at all, and briefly matched
        // only under whichever directory the command ran in; a leading
        // `./` says nothing extra and must not break the pattern
        for pattern in ["sessions/**", "./sessions/**"] {
            let sessions = ExcludeSet::new(&[pattern.to_string()]).unwrap();
            for root in [Path::new("/nonexistent-mise-test/.codex"), cwd.as_path()] {
                assert!(
                    sessions.is_match(&root.join("sessions/one.jsonl"), Path::new(ROOT)),
                    "{pattern} under {}",
                    root.display()
                );
            }
            assert!(!sessions.is_match(
                Path::new("/nonexistent-mise-test/.codex/config.toml"),
                Path::new(ROOT)
            ));
        }
        // a relative pattern is relative to the tracked root: a
        // `sessions` directory in the path above the root must not make
        // `sessions/**` swallow the whole tracked tree
        let above = ExcludeSet::new(&["sessions/**".to_string()]).unwrap();
        let nested_root = Path::new("/nonexistent-mise-test/sessions/.codex");
        assert!(!above.is_match(&nested_root.join("config.toml"), nested_root));
        assert!(above.is_match(&nested_root.join("sessions/one.jsonl"), nested_root));

        let keys = ExcludeSet::new(&["./keys/**".to_string()]).unwrap();
        assert!(keys.is_match(
            Path::new("/nonexistent-mise-test/.config/app/keys/id"),
            Path::new(ROOT)
        ));
    }

    /// An absolute pattern naming the live `~/…` location has to match a
    /// file the walk reached through a symlinked ancestor, which it only
    /// does when the pattern is normalized the way tracked paths are.
    #[cfg(unix)]
    #[test]
    fn an_absolute_pattern_follows_a_symlinked_ancestor() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("real");
        std::fs::create_dir_all(real.join("app")).unwrap();
        std::fs::write(real.join("app/store.kdb"), "vault").unwrap();
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        // what the walk captures: the normalized path under the real directory
        let walked = normalize_target(&link.join("app/store.kdb"));
        assert!(walked.starts_with(normalize_target(&real)));
        // what the user writes: the location they know, through the link
        let pattern = link.join("app/**").to_string_lossy().into_owned();
        let set = ExcludeSet::new(std::slice::from_ref(&pattern)).unwrap();
        assert!(set.is_match(&walked, Path::new(ROOT)));
    }

    /// A pattern must also match the path exactly as it was written,
    /// not only its normalized form. A tracked entry pushed with an
    /// un-normalized path — a temp directory reached through a symlink,
    /// a Windows 8.3 short name — is walked as written, so normalizing
    /// only the pattern would silently stop the exclusion matching.
    #[cfg(unix)]
    #[test]
    fn an_absolute_pattern_matches_the_path_as_written_too() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("real");
        std::fs::create_dir_all(real.join("codex")).unwrap();
        std::fs::write(real.join("codex/one.jsonl"), "drop").unwrap();
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let root = link.join("codex");
        let pattern = format!("{}/**", root.display());
        let set = ExcludeSet::new(std::slice::from_ref(&pattern)).unwrap();
        // as written
        assert!(set.is_match(&root.join("one.jsonl"), Path::new(ROOT)));
        // and normalized, the way a configured entry is walked
        assert!(set.is_match(&normalize_target(&root).join("one.jsonl"), Path::new(ROOT)));
    }

    /// A pattern is classified by the path it names, not by the text the
    /// user typed, so `$HOME/…` behaves exactly like `~/…` — including
    /// under a negation. A pattern naming an unset variable is left out
    /// rather than expanded to nothing, which would have turned
    /// `$UNSET/keys/**` into `/keys/**`.
    #[test]
    fn a_pattern_is_expanded_before_it_is_classified() {
        let home = crate::dirs::HOME.to_path_buf();
        let target = home.join(".mise-test-expand/app/keys/id");
        for pattern in [
            "~/.mise-test-expand/app/**",
            "$HOME/.mise-test-expand/app/**",
        ] {
            let set = ExcludeSet::new(&[pattern.to_string()]).unwrap();
            assert!(set.is_match(&target, Path::new(ROOT)), "{pattern}");
            assert!(
                !set.is_match(Path::new("/elsewhere/app/keys/id"), Path::new(ROOT)),
                "{pattern}"
            );
        }
        // `${VAR}` too, and a negation still negates
        let set = ExcludeSet::new(&[
            "${HOME}/.mise-test-expand/app/**".to_string(),
            "!$HOME/.mise-test-expand/app/keys/**".to_string(),
        ])
        .unwrap();
        assert!(!set.is_match(&target, Path::new(ROOT)));
        assert!(set.is_match(&home.join(".mise-test-expand/app/other"), Path::new(ROOT)));

        // an unset variable leaves the rule out; it must not become
        // `/keys/**`, and it must not panic
        let set = ExcludeSet::new(&["$MISE_TEST_UNSET_PATTERN_VAR/keys/**".to_string()]).unwrap();
        assert!(!set.is_match(Path::new("/keys/id"), Path::new(ROOT)));
        assert!(!set.is_match(&target, Path::new(ROOT)));
        // an expanded value that begins with `!` is data, not syntax: the
        // rule stays an exclusion, because negation is recorded as a
        // field and the pattern is never parsed back out of a string
        // SAFETY: single-threaded test setup, before any matcher is built
        unsafe { std::env::set_var("MISE_TEST_BANG_PATTERN_VAR", "!private") };
        let bang = ExcludeSet::new(&["$MISE_TEST_BANG_PATTERN_VAR/**".to_string()]).unwrap();
        assert_eq!(bang.expanded().len(), 1);
        assert!(!bang.expanded()[0].negated);
        assert_eq!(bang.expanded()[0].pattern, "!private/**");
        let recorded = ExcludeSet::from_expanded(bang.expanded()).unwrap();
        let root = Path::new("/nonexistent-mise-test");
        assert!(recorded.is_match(&root.join("!private/secret"), root));
        // a genuinely negated rule round-trips as negated
        let negated =
            ExcludeSet::new(&["cache".to_string(), "!cache/keep.conf".to_string()]).unwrap();
        assert!(negated.expanded()[1].negated);
        assert_eq!(negated.expanded()[1].pattern, "cache/keep.conf");
        let recorded = ExcludeSet::from_expanded(negated.expanded()).unwrap();
        assert!(recorded.is_match(&root.join("cache/index"), root));
        assert!(!recorded.is_match(&root.join("cache/keep.conf"), root));
        // a `!` anywhere but the first character is just a character
        let middle = ExcludeSet::new(&["oh!no.txt".to_string()]).unwrap();
        assert!(!middle.expanded()[0].negated);
        assert!(middle.is_match(&root.join("a/oh!no.txt"), root));
        unsafe { std::env::remove_var("MISE_TEST_BANG_PATTERN_VAR") };

        // an empty value is not a value: `$EMPTY/keys/**` must not
        // become `/keys/**`, matching from the filesystem root
        // SAFETY: single-threaded test setup, before any matcher is built
        unsafe { std::env::set_var("MISE_TEST_EMPTY_PATTERN_VAR", "") };
        let empty = ExcludeSet::new(&["$MISE_TEST_EMPTY_PATTERN_VAR/keys/**".to_string()]).unwrap();
        assert!(!empty.is_match(Path::new("/keys/id"), Path::new("/")));
        assert_eq!(empty.unevaluable().len(), 1);
        unsafe { std::env::remove_var("MISE_TEST_EMPTY_PATTERN_VAR") };

        // a lone `$` is literal, not a variable reference
        assert!(ExcludeSet::new(&["cost$".to_string()]).is_ok());

        // `$$` writes a literal `$`, so a real path holding one can be
        // named at all
        let literal = ExcludeSet::new(&["report$$Q1.json".to_string()]).unwrap();
        assert!(literal.is_match(
            Path::new("/nonexistent-mise-test/a/report$Q1.json"),
            Path::new(ROOT)
        ));
        assert!(!literal.is_match(
            Path::new("/nonexistent-mise-test/a/report.json"),
            Path::new(ROOT)
        ));
    }

    /// Pruning a matching directory must not swallow a later `!pattern`
    /// that re-includes something inside it: last-match-wins is the
    /// semantics, and the pruning is only an optimization.
    #[test]
    fn a_negation_below_an_excluded_directory_still_re_includes() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("codex");
        std::fs::create_dir_all(root.join("cache")).unwrap();
        std::fs::write(root.join("config.toml"), "keep").unwrap();
        std::fs::write(root.join("cache/index"), "drop").unwrap();
        std::fs::write(root.join("cache/keep.conf"), "keep").unwrap();
        for negation in [
            format!("!{}/cache/keep.conf", root.display()),
            "!cache/keep.conf".to_string(),
        ] {
            let mut set = TrackedSet {
                exclude: vec!["cache".to_string(), negation.clone()],
                ..Default::default()
            };
            set.push(entry(&root));
            let walk = set.walk().unwrap();
            let mut names: Vec<String> = walk
                .files
                .keys()
                .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
                .collect();
            names.sort();
            assert_eq!(names, ["config.toml", "keep.conf"], "{negation}");
        }
        // the walk and the callers that never walk must agree about every
        // file, which is what pruning could otherwise have broken
        let mut set = TrackedSet {
            exclude: vec![
                "cache".to_string(),
                format!("!{}/cache/keep.conf", root.display()),
            ],
            ..Default::default()
        };
        set.push(entry(&root));
        let walk = set.walk().unwrap();
        for file in ["config.toml", "cache/index", "cache/keep.conf"] {
            let path = root.join(file);
            assert_eq!(
                walk.files.contains_key(&path),
                set.would_retain(&path).unwrap(),
                "{file}"
            );
        }

        // with nothing to re-include, the directory is still pruned
        let exclude = ExcludeSet::new(&["cache".to_string()]).unwrap();
        assert!(!exclude.may_reinclude_below(&root.join("cache")));
        // a negation somewhere else entirely does not disable pruning
        let exclude = ExcludeSet::new(&[
            "cache".to_string(),
            "!/somewhere/else/keep.conf".to_string(),
        ])
        .unwrap();
        assert!(!exclude.may_reinclude_below(&root.join("cache")));
    }

    /// The invariant that keeps this family of bugs from recurring:
    /// the capture walk, `would_retain`, the watcher's own filter and a
    /// replay's reading of the recorded coverage must agree about every
    /// path. All four pick the owning entry the same way — most specific
    /// wins — and evaluate exclusion with the same ancestor-aware,
    /// last-match-wins matcher over one shared composition.
    #[test]
    fn capture_would_retain_watcher_and_replay_agree_about_every_path() {
        use crate::system::history::replay::{PathState, classify_coverage};
        let tmp = tempfile::tempdir().unwrap();
        let outer = tmp.path().join("a");
        let inner = outer.join("b");
        std::fs::create_dir_all(inner.join("cache")).unwrap();
        std::fs::write(outer.join("outer.toml"), "keep").unwrap();
        // owned by the inner entry, and the global `b` must not reach it
        // through the outer entry's root
        std::fs::write(inner.join("inner.toml"), "keep").unwrap();
        std::fs::write(inner.join("cache/index"), "drop").unwrap();
        std::fs::write(inner.join("cache/keep.conf"), "keep").unwrap();

        let mut set = TrackedSet {
            exclude: vec![
                "b".to_string(),
                "cache".to_string(),
                format!("!{}/cache/keep.conf", inner.display()),
            ],
            ..Default::default()
        };
        set.push(entry(&outer));
        set.push(entry(&inner));
        let walk = set.walk().unwrap();
        let coverage = set.coverage(&walk);

        // every path is built with `join`, so on Windows the live paths
        // carry `\` while the patterns carry `/`: one test pins the
        // separator handling on both platforms
        for (file, expected) in [
            (outer.join("outer.toml"), true),
            (inner.join("inner.toml"), true),
            (inner.join("cache/index"), false),
            (inner.join("cache/keep.conf"), true),
        ] {
            let display = display_path(&file);
            assert_eq!(
                walk.files.contains_key(&file),
                expected,
                "capture {display}"
            );
            assert_eq!(
                set.would_retain(&file).unwrap(),
                expected,
                "would_retain {display}"
            );
            assert_eq!(
                !set.excluded_by_lists(&set.exclude_set().unwrap(), &file),
                expected,
                "watcher {display}"
            );
            let covered = !matches!(classify_coverage(&coverage, &display), PathState::Uncovered);
            assert_eq!(covered, expected, "replay {display}");
        }

        // a replay reads the expansion the checkpoint recorded, never the
        // environment it is running in
        assert!(
            coverage
                .exclude_expanded
                .iter()
                .any(|rule| rule.pattern.contains("cache")),
            "{:?}",
            coverage.exclude_expanded
        );
        let display = display_path(outer.join("outer.toml"));
        let mut changed = coverage.clone();
        changed.exclude = vec!["$MISE_TEST_UNSET_PATTERN_VAR/**".to_string()];
        assert!(
            !matches!(
                classify_coverage(&changed, &display),
                PathState::Unevaluable(_)
            ),
            "the recorded expansion decides, so the as-written list cannot make it unknown"
        );

        // a checkpoint written before expansions were recorded is still
        // readable when nothing in its list could expand differently
        let mut plain = coverage.clone();
        plain.exclude_expanded.clear();
        assert!(!matches!(
            classify_coverage(&plain, &display),
            PathState::Unevaluable(_)
        ));
        // but one whose list could name a variable cannot be classified
        // at all, and must not be read as "absent"
        let mut legacy = plain.clone();
        legacy
            .exclude
            .push("$MISE_TEST_UNSET_PATTERN_VAR/**".to_string());
        assert!(matches!(
            classify_coverage(&legacy, &display),
            PathState::Unevaluable(_)
        ));

        // a rule that cannot be evaluated is unknown, never absent
        let mut unevaluable = plain.clone();
        unevaluable
            .exclude
            .push("$MISE_TEST_UNSET_PATTERN_VAR/cache/**".to_string());
        assert!(matches!(
            classify_coverage(&unevaluable, &display),
            PathState::Unevaluable(_)
        ));

        // a recorded rule keeps its negation as a field, so an expanded
        // value beginning with `!` cannot turn an exclusion into a
        // re-inclusion when the checkpoint is read back
        let recorded = crate::system::history::store::ExpandedRule {
            negated: false,
            pattern: format!("{}/**", display_path(&inner)),
        };
        let mut injected = coverage.clone();
        injected.exclude_expanded = vec![
            crate::system::history::store::ExpandedRule {
                negated: false,
                pattern: "!private/**".to_string(),
            },
            recorded,
        ];
        assert!(matches!(
            classify_coverage(&injected, &display_path(inner.join("inner.toml"))),
            PathState::Uncovered
        ));
    }

    #[test]
    fn display_under_accepts_either_separator() {
        assert!(display_under("~/.ssh", "~/.ssh"));
        assert!(display_under("~/.ssh/id_test", "~/.ssh"));
        assert!(display_under("~\\.ssh\\id_test", "~\\.ssh"));
        assert!(!display_under("~/.sshd/x", "~/.ssh"));
        assert!(!display_under("~/.ssh", "~/.ssh/id_test"));
    }

    #[test]
    fn omission_reports_list_few_and_summarize_many() {
        let omitted = |n: usize| -> Vec<PathReason> {
            (0..n)
                .map(|i| PathReason {
                    path: format!("~/.config/app/secret{i}"),
                    reason: CREDENTIAL_REASON.into(),
                })
                .collect()
        };
        assert!(omission_report(&[], &[]).is_empty());
        let few = omission_report(&omitted(2), &[]);
        assert_eq!(few.len(), 2);
        assert_eq!(
            few[0],
            format!("omitted: ~/.config/app/secret0 ({CREDENTIAL_REASON})")
        );
        let many = omission_report(&omitted(OMISSION_LINES + 1), &[]);
        assert_eq!(
            many,
            vec![format!(
                "{} files omitted from capture (credential store); `mise dot paths` lists them",
                OMISSION_LINES + 1
            )]
        );
        let mut mixed = omitted(1);
        mixed.push(PathReason {
            path: "~/.config/app/config.local.toml".into(),
            reason: "machine-local configuration".into(),
        });
        assert_eq!(
            omission_summary(&mixed, &[]),
            "2 files omitted from capture (1 credential store); `mise dot paths` lists them"
        );
        let nested = vec![PathReason {
            path: "~/.hammerspoon/Spoons/Sky.spoon".into(),
            reason: NESTED_REPOSITORY_REASON.into(),
        }];
        assert_eq!(
            omission_report(&[], &nested),
            vec![format!(
                "nested: ~/.hammerspoon/Spoons/Sky.spoon ({NESTED_REPOSITORY_REASON})"
            )]
        );
        assert_eq!(
            omission_summary(&omitted(1), &nested),
            "1 files omitted from capture (credential store); 1 nested repository not saved; `mise dot paths` lists them"
        );
    }

    #[test]
    fn a_nested_repository_is_a_pointer_and_is_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("plugins");
        let plugin = root.join("nested");
        std::fs::create_dir_all(plugin.join(".git")).unwrap();
        std::fs::write(plugin.join("init.lua"), "return {}").unwrap();
        std::fs::write(root.join("init.lua"), "top").unwrap();
        let mut set = TrackedSet::default();
        set.push(entry(&root));
        let walk = set.walk().unwrap();
        assert!(walk.files.contains_key(&root.join("init.lua")));
        assert!(walk.files.contains_key(&plugin));
        assert!(!walk.files.contains_key(&plugin.join("init.lua")));
        assert_eq!(walk.nested.len(), 1);
        assert_eq!(walk.nested[0].path, display_path(&plugin));
        assert_eq!(walk.nested[0].reason, NESTED_REPOSITORY_REASON);
        assert!(walk.omitted.is_empty());
        assert_eq!(set.coverage(&walk).nested, walk.nested);
        assert!(!set.would_capture(&plugin.join("init.lua")).unwrap());
        // the nested repository as its own entry saves its files
        set.push(entry(&plugin));
        let walk = set.walk().unwrap();
        assert!(walk.files.contains_key(&plugin.join("init.lua")));
        assert!(!walk.files.contains_key(&plugin));
        assert!(walk.nested.is_empty());
    }

    #[test]
    fn nested_targets_partition_a_preview() {
        let tmp = tempfile::tempdir().unwrap();
        let outer = tmp.path().join("codex");
        let inner = outer.join("sessions");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(outer.join("config.toml"), "outer").unwrap();
        std::fs::write(inner.join("one.jsonl"), "inner-1").unwrap();
        std::fs::write(inner.join("two.jsonl"), "inner-2").unwrap();
        std::fs::write(inner.join("auth-token"), "x").unwrap();
        let mut set = TrackedSet::default();
        set.push(entry(&outer));
        set.push(entry(&inner));
        let walk = set.walk().unwrap();
        let outer_preview = walk.preview_of(&set, 0);
        let inner_preview = walk.preview_of(&set, 1);
        assert_eq!(outer_preview.files, 1);
        assert_eq!(outer_preview.bytes, 5);
        assert!(outer_preview.omitted.is_empty());
        assert_eq!(inner_preview.files, 2);
        assert_eq!(inner_preview.bytes, 14);
        assert_eq!(inner_preview.omitted.len(), 1);
        assert_eq!(outer_preview.summary(), "1 file, 5 B");
    }

    #[test]
    fn walk_summaries_count_files_and_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("tree");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("a"), "12345").unwrap();
        std::fs::write(root.join("sub/b"), "123").unwrap();
        std::fs::write(root.join("sub/secret.key"), "x").unwrap();
        let set = preview_set(&root, Policy::for_mode(FileMode::Track)).unwrap();
        assert_eq!(set.entries.len(), 1);
        assert_eq!(set.entries[0].path, normalize_target(&root));
        #[cfg(unix)]
        std::os::unix::fs::symlink("a", root.join("link")).unwrap();
        let walk = set.walk().unwrap();
        // a symlink counts as a file but adds no bytes
        assert_eq!(walk.file_count(), if cfg!(unix) { 3 } else { 2 });
        assert_eq!(walk.bytes(), 8);
        assert_eq!(
            walk.summary(),
            if cfg!(unix) {
                "3 files, 8 B"
            } else {
                "2 files, 8 B"
            }
        );
        assert_eq!(walk.omitted.len(), 1);
        assert!(!walk.preview_of(&set, 0).is_large());
        assert_eq!(count_and_size(1, 0), "1 file, 0 B");
        assert_eq!(with_separators(0), "0");
        assert_eq!(with_separators(999), "999");
        assert_eq!(with_separators(1000), "1,000");
        assert_eq!(with_separators(22972), "22,972");
        assert_eq!(with_separators(1234567), "1,234,567");
    }

    #[test]
    fn an_entry_excludes_relative_to_its_own_path() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("codex");
        std::fs::create_dir_all(root.join("sessions/deep")).unwrap();
        std::fs::create_dir_all(root.join("config/sessions")).unwrap();
        std::fs::create_dir_all(root.join("cache")).unwrap();
        std::fs::write(root.join("config.toml"), "keep").unwrap();
        std::fs::write(root.join("notes.md"), "drop").unwrap();
        std::fs::write(root.join("sessions/one.jsonl"), "drop").unwrap();
        std::fs::write(root.join("sessions/deep/two.jsonl"), "drop").unwrap();
        std::fs::write(root.join("config/sessions/keep.toml"), "keep").unwrap();
        std::fs::write(root.join("cache/index"), "drop").unwrap();
        // a component pattern matches anywhere, an anchored one only at
        // the entry root, and a matching directory takes its subtree
        let mut entry = entry(&root);
        entry.exclude = vec!["*.md".into(), "sessions/**".into(), "cache".into()];
        let mut set = TrackedSet::default();
        set.push(entry);
        let walk = set.walk().unwrap();
        assert_eq!(walk.files.len(), 2);
        assert!(walk.files.contains_key(&root.join("config.toml")));
        assert!(
            walk.files
                .contains_key(&root.join("config/sessions/keep.toml"))
        );
        assert!(walk.omitted.is_empty());
        assert!(set.would_retain(&root.join("config.toml")).unwrap());
        assert!(!set.would_retain(&root.join("notes.md")).unwrap());
        assert!(
            !set.would_retain(&root.join("sessions/deep/two.jsonl"))
                .unwrap()
        );
        assert!(!set.would_retain(&root.join("cache/index")).unwrap());
        assert!(
            set.would_retain(&root.join("config/sessions/keep.toml"))
                .unwrap()
        );
        // the entry path itself is never dropped by its own list
        let mut entry =
            super::TrackedEntry::new(root.clone(), "track", Policy::for_mode(FileMode::Track));
        entry.exclude = vec!["codex".into()];
        assert!(!entry.is_excluded(&root));
        assert!(!entry.is_excluded(tmp.path()));
        // a global `!glob` re-include does not override an entry's list
        let mut entry =
            super::TrackedEntry::new(root.clone(), "track", Policy::for_mode(FileMode::Track));
        entry.exclude = vec!["cache".into()];
        let mut set = TrackedSet {
            exclude: vec![
                format!("{}/**", root.display()),
                format!("!{}/cache/**", root.display()),
            ],
            ..Default::default()
        };
        set.push(entry);
        let walk = set.walk().unwrap();
        assert!(walk.files.is_empty());
        assert!(!set.would_retain(&root.join("cache/index")).unwrap());
        assert_eq!(
            set.coverage(&walk).entries[0].exclude,
            vec!["cache".to_string()]
        );
    }

    #[test]
    fn entry_excludes_travel_through_the_enrollment_manifest() {
        use crate::system::files::FileRequest;
        use crate::system::resources::ResourceOrigin;
        let home = normalize(&dirs::HOME);
        let target = home.join(".mise-test-entry-exclude");
        let mut set = TrackedSet::default();
        set.add_requests([FileRequest {
            target_raw: "~/.mise-test-entry-exclude".into(),
            target: target.clone(),
            source: PathBuf::new(),
            content: None,
            mode: FileMode::Track,
            exclude: vec![glob::Pattern::new("sessions").unwrap()],
            manifest: None,
            base: home.clone(),
            origin: ResourceOrigin {
                config: home.join(".config/mise/config.toml"),
                config_root: home.join(".config/mise"),
                environment: vec![],
                source: None,
            },
            policy: Policy::for_mode(FileMode::Track),
            variants: vec![],
            enabled: true,
        }]);
        assert_eq!(set.manifest.enrollment.len(), 1);
        assert_eq!(
            set.manifest.enrollment[0].exclude,
            vec!["sessions".to_string()]
        );
        let rebuilt = set.manifest.tracking().unwrap();
        assert_eq!(rebuilt.entries[0].exclude, vec!["sessions".to_string()]);
        assert!(rebuilt.entries[0].is_excluded(&target.join("sessions/one")));
    }

    #[test]
    fn tree_paths_round_trip() {
        assert_eq!(tree_path_to_display("home/.zshrc"), "~/.zshrc");
        // a path that exists is canonicalized first (`/etc` is a link on
        // macOS), so the round trip uses one that does not
        assert_eq!(
            tree_path_to_display("fs/nonexistent-mise-test/hosts"),
            "/nonexistent-mise-test/hosts"
        );
        assert_eq!(
            display_to_tree_path("/nonexistent-mise-test/hosts"),
            "fs/nonexistent-mise-test/hosts"
        );
    }

    #[cfg(unix)]
    #[test]
    fn tracking_a_symlink_does_not_enroll_its_target() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("target");
        std::fs::write(&target, "not enrolled").unwrap();
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let mut set = TrackedSet::default();
        set.push(entry(&link));
        let walk = set.walk().unwrap();
        assert!(walk.files.contains_key(&link));
        assert!(!walk.files.contains_key(&target));
    }
}
