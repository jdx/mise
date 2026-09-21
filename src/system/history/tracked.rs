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
    ///
    /// `None` means the declaration said nothing, so whatever the saved
    /// manifest carries stands. `Some` means it spoke — including
    /// `Some([])`, which clears the list another machine published.
    /// Flattening the two into one empty vec left no way to say "capture
    /// all of it after all".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude: Option<Vec<String>>,
}

impl TrackedEntry {
    /// The compiled `exclude` patterns; an invalid one was already
    /// reported when the declaration was read.
    pub(crate) fn exclude_patterns(&self) -> Vec<glob::Pattern> {
        self.exclude
            .iter()
            .flatten()
            .filter_map(|pattern| glob::Pattern::new(pattern).ok())
            .collect()
    }

    /// Whether the entry's own `exclude` list drops `path`: a path below
    /// the entry whose entry-relative form matches, as for a deployment
    /// entry. The entry path itself is never excluded by its own list.
    pub(crate) fn is_excluded(&self, path: &Path) -> bool {
        excluded_by_entry(&self.path, self.exclude.as_deref().unwrap_or(&[]), path)
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
            exclude: None,
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
                exclude: declared_exclude(&request),
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
                    entry.exclude = declared_exclude(&request);
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

    pub(crate) fn entry_index_for(&self, path: &Path) -> Option<usize> {
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

    /// Whether the exclusion lists drop `path`: the global
    /// `[history] exclude` globs, then the owning entry's own list,
    /// which is applied after the global one and is not re-included by
    /// a global `!glob`. A path no entry covers is dropped.
    ///
    /// **The one composition, because narrowing selection stops
    /// management and does not delete.** A capture drops a newly
    /// excluded file from the next snapshot while leaving it on disk —
    /// which is the whole point of excluding it — so every other
    /// consumer has to read that absence the same way. Asking ownership
    /// alone made synchronization read it as a deletion to replay, and
    /// `exclude = ["leave"]` on one machine became `rm` on every other
    /// one.
    pub(crate) fn excluded_by_lists(&self, exclude: &ExcludeSet, path: &Path) -> bool {
        match self.entry_for(path) {
            Some(owner) => exclude.is_match(path) || owner.is_excluded(path),
            None => true,
        }
    }

    pub(crate) fn exclude_set(&self) -> Result<ExcludeSet> {
        ExcludeSet::new(&self.exclude)
    }

    /// Walks every entry and decides, file by file, what the capture holds.
    pub(crate) fn walk(&self) -> Result<Walk> {
        self.walk_entries(None)
    }

    /// Walks only the entries at `selected`, while the set keeps all of
    /// them.
    ///
    /// **Which entry owns a path is a question about the whole set;
    /// walking is what costs.** A preview needs every declaration
    /// present, so a target nested under an existing entry — or an
    /// existing entry nested under the target — is attributed the way a
    /// capture would attribute it. It does not need the other entries
    /// walked: `mise dot track --dry-run` on one directory would
    /// otherwise re-walk and re-stat every directory already tracked on
    /// the machine, which is the opposite of cheap.
    pub(crate) fn walk_selected(&self, selected: &[usize]) -> Result<Walk> {
        self.walk_entries(Some(selected))
    }

    fn walk_entries(&self, selected: Option<&[usize]>) -> Result<Walk> {
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
            if selected.is_some_and(|selected| !selected.contains(&index)) {
                continue;
            }
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
        // A more specific entry owns this subtree and walks it itself.
        // **Skipped whole, not file by file**: ownership is by the most
        // specific entry, so nothing below a directory another entry
        // owns can belong to this one, and stat'ing all of it to decide
        // that again is the cost this avoids — a tracked directory
        // inside another tracked directory would otherwise be walked
        // twice on every save.
        if set
            .entry_index_for(path)
            .is_some_and(|owner| owner != index)
        {
            if candidate.file_type().is_dir() {
                walker.skip_current_dir();
            }
            continue;
        }
        if exclude.is_match(path) {
            continue;
        }
        let file_type = candidate.file_type();
        // the entry's own exclusions: a matching directory is not entered
        if !entry_exclude.is_empty()
            && let Ok(rel) = path.strip_prefix(&entry.path)
            && crate::system::files::is_excluded(&pattern_relative(rel), &entry_exclude)
        {
            if file_type.is_dir() {
                walker.skip_current_dir();
            }
            continue;
        }
        if file_type.is_dir() {
            if path.join(".git").exists() {
                // A repository found inside a tracked directory is skipped
                // whole, and nothing is written for it — not its files, not
                // a commit pointer. A pointer would name objects this
                // history does not have, and there is a supported way to
                // get the files: track the repository itself.
                walk.nested.push(PathReason {
                    path: display_path(path),
                    reason: NESTED_REPOSITORY_REASON.into(),
                });
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

/// What a capture says about a directory with its own `.git` found
/// inside a tracked one.
///
/// **A repository encountered inside a tracked directory is skipped, and
/// nothing is recorded for it. Tracking a repository's own directory
/// captures its working files, always without `.git`.** The explanation
/// names the remedy because there is one: the reach-in is unsupported,
/// not the goal.
pub(crate) const NESTED_REPOSITORY_REASON: &str =
    "a separate Git repository; track it directly to capture its working files";

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

/// An entry-relative path as its patterns see it.
///
/// **A pattern is written with `/`, and on Windows the path it is matched
/// against arrives with `\`.** `cache/**` would never match
/// `cache\index`, so a `~\.codex` entry's `exclude` list would quietly
/// do nothing there. The separator is settled here, in the one helper the
/// capture walk, a dry run and a replay all match through, so the three
/// cannot disagree about what a list drops.
///
/// On unix a backslash is an ordinary character in a filename and is left
/// alone: a file actually named `cache\index` is one component, not two.
fn pattern_relative(rel: &Path) -> std::borrow::Cow<'_, Path> {
    #[cfg(windows)]
    {
        std::borrow::Cow::Owned(PathBuf::from(rel.to_string_lossy().replace('\\', "/")))
    }
    #[cfg(not(windows))]
    {
        std::borrow::Cow::Borrowed(rel)
    }
}

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
        Ok(rel) if !rel.as_os_str().is_empty() => {
            crate::system::files::is_excluded(&pattern_relative(rel), &patterns)
        }
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
            "{} nested {} skipped",
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

/// The `exclude` list a declaration wrote, or `None` when it wrote none.
///
/// **Saying nothing and saying nothing-is-excluded are different
/// answers.** The composed request flattens both to an empty pattern
/// list, so the explicitness the configuration layer already records is
/// what tells them apart. Without it a machine could never clear a list
/// another machine published: writing `exclude = []` would read as "I
/// have no opinion" and the saved list would stand for ever.
fn declared_exclude(request: &crate::system::files::FileRequest) -> Option<Vec<String>> {
    request.policy.explicit.exclude.then(|| {
        request
            .exclude
            .iter()
            .map(|pattern| pattern.as_str().to_owned())
            .collect()
    })
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
    let owner = entries
        .iter()
        .filter(|entry| path.starts_with(&entry.path))
        .max_by_key(|entry| entry.path.components().count());
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
                exclude: None,
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

    /// A preview walks the target and nothing else, while the set still
    /// holds every declaration so ownership is decided the way a capture
    /// decides it.
    #[test]
    fn a_selected_walk_visits_only_what_was_asked_for() {
        let tmp = tempfile::tempdir().unwrap();
        let other = tmp.path().join("other");
        let target = tmp.path().join("target");
        std::fs::create_dir_all(other.join("deep")).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(other.join("deep/one.toml"), "other").unwrap();
        std::fs::write(target.join("two.toml"), "target").unwrap();

        let mut set = TrackedSet::default();
        set.push(entry(&other));
        set.push(entry(&target));
        let index = set.entry_index_for(&target).unwrap();

        let all = set.walk().unwrap();
        assert!(all.files.contains_key(&other.join("deep/one.toml")));
        assert!(all.files.contains_key(&target.join("two.toml")));

        let selected = set.walk_selected(&[index]).unwrap();
        assert!(
            !selected.files.contains_key(&other.join("deep/one.toml")),
            "an entry nobody asked about was walked"
        );
        assert!(selected.files.contains_key(&target.join("two.toml")));
        // and the answer about the target is the same one a full walk
        // gives, because ownership was decided from the whole set
        assert_eq!(
            selected.preview_of(&set, index).files,
            all.preview_of(&set, index).files
        );
    }

    /// A tracked directory inside another tracked directory is walked
    /// by its own entry, once — not descended into twice and then
    /// discarded file by file.
    #[test]
    fn an_entry_inside_another_is_not_walked_twice() {
        let tmp = tempfile::tempdir().unwrap();
        let outer = tmp.path().join("config");
        let inner = outer.join("nvim");
        std::fs::create_dir_all(inner.join("lua")).unwrap();
        std::fs::write(outer.join("outer.toml"), "outer").unwrap();
        for i in 0..20 {
            std::fs::write(inner.join(format!("lua/{i}.lua")), "inner").unwrap();
        }

        let mut set = TrackedSet::default();
        set.push(entry(&outer));
        set.push(entry(&inner));
        let outer_index = set.entry_index_for(&outer).unwrap();
        let inner_index = set.entry_index_for(&inner).unwrap();

        // the files land under the entry that owns them, exactly as
        // before: what changed is only how the other entry got there
        let walk = set.walk().unwrap();
        assert_eq!(walk.files[&outer.join("outer.toml")].0, outer_index);
        assert_eq!(walk.files[&inner.join("lua/3.lua")].0, inner_index);
        assert_eq!(walk.files.len(), 21);

        // and walking the outer entry alone reaches only its own file
        let selected = set.walk_selected(&[outer_index]).unwrap();
        assert_eq!(
            selected.files.keys().collect::<Vec<_>>(),
            vec![&outer.join("outer.toml")]
        );
    }

    /// A pattern is written with `/` on every platform; the path it is
    /// matched against is not. Capture, dry run and replay all match
    /// through `excluded_by_entry`, so this is where the two meet.
    #[test]
    fn an_entry_list_matches_a_path_with_the_host_separator() {
        let root = PathBuf::from(if cfg!(windows) {
            "C:\\Users\\me\\.codex"
        } else {
            "/home/me/.codex"
        });
        let patterns = ["cache/**".to_string()];
        assert!(excluded_by_entry(
            &root,
            &patterns,
            &root.join("cache").join("index"),
        ));
        assert!(!excluded_by_entry(
            &root,
            &patterns,
            &root.join("config.toml"),
        ));
        // on unix a backslash is a character in a filename, not a
        // separator, so one file named `cache\index` is not a file
        // `index` inside `cache`
        #[cfg(unix)]
        assert!(!excluded_by_entry(
            &root,
            &patterns,
            &root.join("cache\\index"),
        ));
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
            "1 files omitted from capture (credential store); 1 nested repository skipped; `mise dot paths` lists them"
        );
    }

    #[test]
    fn a_nested_repository_is_skipped_and_tracking_it_captures_its_files() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("plugins");
        let plugin = root.join("nested");
        std::fs::create_dir_all(plugin.join(".git")).unwrap();
        std::fs::write(plugin.join(".git/HEAD"), "ref: refs/heads/main").unwrap();
        std::fs::write(plugin.join("init.lua"), "return {}").unwrap();
        std::fs::write(root.join("init.lua"), "top").unwrap();
        let mut set = TrackedSet::default();
        set.push(entry(&root));
        let walk = set.walk().unwrap();
        assert!(walk.files.contains_key(&root.join("init.lua")));
        // nothing is written for it: not its files, and not a pointer
        assert!(!walk.files.contains_key(&plugin));
        assert!(!walk.files.contains_key(&plugin.join("init.lua")));
        assert_eq!(walk.nested.len(), 1);
        assert_eq!(walk.nested[0].path, display_path(&plugin));
        assert_eq!(walk.nested[0].reason, NESTED_REPOSITORY_REASON);
        // the explanation names the remedy, because there is one
        assert!(walk.nested[0].reason.contains("track it directly"));
        assert!(walk.omitted.is_empty());
        assert_eq!(set.coverage(&walk).nested, walk.nested);
        assert!(!set.would_capture(&plugin.join("init.lua")).unwrap());
        // and that remedy works: tracking the repository itself captures
        // its working files, always without `.git`
        set.push(entry(&plugin));
        let walk = set.walk().unwrap();
        assert!(walk.files.contains_key(&plugin.join("init.lua")));
        assert!(!walk.files.contains_key(&plugin));
        assert!(!walk.files.contains_key(&plugin.join(".git/HEAD")));
        assert!(
            !walk
                .files
                .keys()
                .any(|path| path.components().any(|c| c.as_os_str() == ".git")),
            "`.git` is never captured"
        );
        assert!(walk.nested.is_empty());
        assert!(set.would_capture(&plugin.join("init.lua")).unwrap());
        assert!(!set.would_capture(&plugin.join(".git/HEAD")).unwrap());
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
        entry.exclude = Some(vec!["*.md".into(), "sessions/**".into(), "cache".into()]);
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
        entry.exclude = Some(vec!["codex".into()]);
        assert!(!entry.is_excluded(&root));
        assert!(!entry.is_excluded(tmp.path()));
        // a global `!glob` re-include does not override an entry's list
        let mut entry =
            super::TrackedEntry::new(root.clone(), "track", Policy::for_mode(FileMode::Track));
        entry.exclude = Some(vec!["cache".into()]);
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
            Some(vec!["cache".to_string()])
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
            // the declaration wrote `exclude`, which is what makes the
            // list its own answer rather than silence
            policy: Policy {
                explicit: crate::system::files::ExplicitFields {
                    exclude: true,
                    ..Default::default()
                },
                ..Policy::for_mode(FileMode::Track)
            },
            variants: vec![],
            enabled: true,
        }]);
        assert_eq!(set.manifest.enrollment.len(), 1);
        assert_eq!(
            set.manifest.enrollment[0].exclude,
            Some(vec!["sessions".to_string()])
        );
        let rebuilt = set.manifest.tracking().unwrap();
        assert_eq!(
            rebuilt.entries[0].exclude,
            Some(vec!["sessions".to_string()])
        );
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
