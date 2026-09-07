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
use super::store::{Coverage, CoverageEntry, DerivedRecord, PathReason};
use crate::config::Config;
use crate::dirs;
use crate::file::{self, display_path};
use crate::system::files::{FileMode, FilePolicy};

/// Names under the global config directory that hold credentials: private
/// by default, in every outgoing representation.
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
    /// For outputs: the source that generates them.
    pub source: Option<PathBuf>,
    /// Why the entry is not shared although sharing was not switched off.
    pub note: Option<String>,
    pub declared_in: Option<PathBuf>,
}

impl TrackedEntry {
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
            source: None,
            note: None,
            declared_in: None,
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

/// One private file found during a walk.
#[derive(Clone, Debug)]
pub(crate) struct PrivateFile {
    pub path: PathBuf,
    pub reason: String,
    pub policy: Policy,
}

/// What a walk of the tracked set found.
#[derive(Debug, Default)]
pub(crate) struct Walk {
    pub manifest: super::manifest::Manifest,
    /// The entries as walked: the set's entries plus derived ones.
    pub entries: Vec<TrackedEntry>,
    pub roots: Vec<CaptureRoot>,
    /// Every captured file with the entry that owns it and its policy.
    pub files: BTreeMap<PathBuf, (usize, Policy)>,
    pub private: Vec<PrivateFile>,
    pub derived: Vec<DerivedRecord>,
    pub omitted: Vec<PathReason>,
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
        self.entries
            .iter()
            .filter(|entry| path.starts_with(&entry.path))
            .max_by_key(|entry| entry.path.components().count())
    }

    fn entry_index_for(&self, path: &Path) -> Option<usize> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| path.starts_with(&entry.path))
            .max_by_key(|(_, entry)| entry.path.components().count())
            .map(|(index, _)| index)
    }

    /// Whether a capture of this set would include `path`: under an entry,
    /// not excluded, not inside mise's own directories or a `.git`.
    pub(crate) fn would_capture(&self, path: &Path) -> Result<bool> {
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
        // what the walker omits: special files, files over the size limit
        if let Ok(meta) = std::fs::symlink_metadata(path)
            && !meta.is_dir()
            && classify_file(&meta).is_err()
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
        Ok(!self.exclude_set()?.is_match(path))
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
            root.bytes += std::fs::symlink_metadata(path)
                .map(|m| m.len())
                .unwrap_or(0);
        }
        walk.roots = roots.into_values().collect();
        Ok(walk)
    }

    /// The rules this set captures under, for the checkpoint record.
    pub(crate) fn coverage(&self, walk: &Walk) -> Coverage {
        let mut entries: Vec<CoverageEntry> = walk
            .entries
            .iter()
            .map(|entry| CoverageEntry {
                path: entry.display(),
                mode: entry.mode.clone(),
                variant: entry.variant.clone(),
                source: entry.source.as_deref().map(display_path),
                autosave: entry.policy.autosave,
                encrypt: entry.policy.encrypt,
                state: "live".into(),
                promotion: None,
                private: entry.note.clone(),
                declared_in: entry.declared_in.as_deref().map(display_path),
            })
            .collect();
        for private in &walk.private {
            entries.push(CoverageEntry {
                path: display_path(&private.path),
                mode: "private".into(),
                variant: None,
                source: None,
                autosave: private.policy.autosave,
                encrypt: private.policy.encrypt,
                state: "live".into(),
                promotion: None,
                private: Some(private.reason.clone()),
                declared_in: None,
            });
        }
        let mut omitted = walk.omitted.clone();
        omitted.extend(self.invalid.iter().cloned());
        Coverage {
            entries,
            exclude: self.exclude.clone(),
            derived: walk.derived.clone(),
            incomplete: walk.incomplete.clone(),
            omitted,
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
        if exclude.is_match(&entry.path) {
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
        if exclude.is_match(path) {
            continue;
        }
        let file_type = candidate.file_type();
        if file_type.is_dir() {
            if path.join(".git").exists() {
                // captured as a gitlink; never descended into
                walk.files.insert(path.to_path_buf(), (index, entry.policy));
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

fn capture_exclusion(path: &Path, policy: &Policy) -> Option<&'static str> {
    static NAMES: std::sync::LazyLock<GlobSet> = std::sync::LazyLock::new(credential_names);
    static GLOBS: std::sync::LazyLock<GlobSet> = std::sync::LazyLock::new(credential_globs);
    let name = path.file_name()?.to_str()?;
    if name.ends_with(".local.toml") {
        Some("machine-local configuration")
    } else if !policy.encrypt
        && (GLOBS.is_match(name)
            || (path.starts_with(normalize(&global_config_dir())) && NAMES.is_match(name)))
    {
        Some("credential store; encrypt the file before tracking it")
    } else {
        None
    }
}

/// Credential stores mise itself knows by name; they mean something only
/// under the global configuration directory.
fn credential_names() -> GlobSet {
    glob_set(CREDENTIAL_NAMES)
}

/// Key material by name pattern, private wherever it is captured.
fn credential_globs() -> GlobSet {
    glob_set(CREDENTIAL_GLOBS)
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
    patterns: Vec<(globset::GlobMatcher, bool)>,
}

impl ExcludeSet {
    pub(crate) fn new(globs: &[String]) -> Result<Self> {
        let mut patterns = vec![];
        for glob in globs {
            let (pattern, negated) = match glob.strip_prefix('!') {
                Some(rest) => (rest, true),
                None => (glob.as_str(), false),
            };
            let expanded = file::replace_path(Path::new(pattern));
            patterns.push((
                Glob::new(&expanded.to_string_lossy())?.compile_matcher(),
                negated,
            ));
        }
        Ok(Self { patterns })
    }

    /// Whether `path` is excluded: the last matching pattern decides.
    pub(crate) fn is_match(&self, path: &Path) -> bool {
        let mut excluded = false;
        for (matcher, negated) in &self.patterns {
            if matcher.is_match(path) {
                excluded = !negated;
            }
        }
        excluded
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
    // the setup branch's reserved directories, should a checkout of it sit
    // in the configuration directory: never captured, never republished
    let config_dir = normalize(&global_config_dir());
    for reserved in ["tracked", "sources", ".mise-history"] {
        dirs.push(config_dir.join(reserved));
    }
    dirs.extend(
        crate::agecrypt::identity_paths()
            .iter()
            .map(|path| normalize(path)),
    );
    dirs.sort();
    dirs.dedup();
    dirs
}

/// The global config directory (where `--from-git` checks out).
pub(crate) fn global_config_dir() -> PathBuf {
    crate::env::MISE_GLOBAL_CONFIG_FILE
        .as_deref()
        .and_then(Path::parent)
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
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
    use super::*;

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
        assert!(walk.private.is_empty());
        assert!(set.would_capture(&included).unwrap());
        assert!(!set.would_capture(&child.join("config.local.toml")).unwrap());
        assert!(!set.would_capture(&child.join("credentials.json")).unwrap());
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
        assert!(walk.derived.is_empty());
    }
}
