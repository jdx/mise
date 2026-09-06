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
//! Entries come from the implicit roots (the global config directory and
//! `dotfiles.root`), from `[dotfiles]` declarations of the system and global
//! layers (tracked files, the destinations of source-managed entries, and
//! the sources those entries reference), and from the targets of tracked
//! symlinks inside `$HOME`.

use std::collections::{BTreeMap, BTreeSet};
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

/// How far a chain of symlinks is followed for derived targets.
const MAX_LINK_DEPTH: usize = 8;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EntryKind {
    /// The global config directory and `dotfiles.root`: always tracked.
    Implicit,
    /// A `[dotfiles]` entry with `mode = "track"`.
    Track,
    /// The destination of a source-managed `[dotfiles]` entry.
    Output,
    /// The source a source-managed entry references.
    Source,
    /// The target of a tracked symlink inside `$HOME`.
    Derived,
}

#[derive(Clone, Debug)]
pub(crate) struct TrackedEntry {
    /// Absolute, `~` expanded, lexically normalized.
    pub path: PathBuf,
    pub kind: EntryKind,
    /// The declaring mode (`track`, `template`, `copy`, …) or the kind.
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
    pub(crate) fn display(&self) -> String {
        display_path(&self.path)
    }

    pub(crate) fn new(path: PathBuf, kind: EntryKind, mode: &str, policy: Policy) -> Self {
        Self {
            path,
            kind,
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
    /// The always-tracked entries: the global config directory (where
    /// `--from-git` checks out) and `dotfiles.root`.
    pub(crate) fn implicit() -> Self {
        let mut set = Self::default();
        for dir in [global_config_dir(), crate::system::files::dotfiles_root()] {
            set.push(TrackedEntry::new(
                normalize(&dir),
                EntryKind::Implicit,
                "implicit",
                Policy::for_mode(FileMode::Track),
            ));
        }
        set
    }

    /// The effective tracked set for the loaded configuration.
    pub(crate) async fn effective() -> Result<Self> {
        let config = Config::get().await?;
        Self::from_config(&config)
    }

    /// The implicit entries plus everything the system and global
    /// `[dotfiles]` declarations enroll.
    pub(crate) fn from_config(config: &Config) -> Result<Self> {
        let mut set = Self::implicit();
        set.exclude = super::config::exclude_globs()?;
        let environments = select::active_environments();
        for request in crate::system::files::files_from_config(config)? {
            if !crate::system::files::declaration_is_global(config, &request) {
                continue;
            }
            let declared_in = Some(request.origin.config.clone());
            match request.mode {
                FileMode::Track => {
                    let mut entry = TrackedEntry::new(
                        normalize_target(&request.target),
                        EntryKind::Track,
                        "track",
                        request.policy,
                    );
                    entry.declared_in = declared_in;
                    match select::select(&request.variants, &environments) {
                        Selection::Single => {}
                        Selection::Variant(variant) => {
                            if let Some(share) = variant.share {
                                entry.policy.share = share;
                            }
                            entry.variant = Some(variant.name());
                        }
                        Selection::NoMatch => {
                            entry.policy.share = false;
                            entry.note = Some(
                                "no variant matches this machine; add `--os …` or a default variant to share it"
                                    .into(),
                            );
                        }
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
                FileMode::Symlink | FileMode::SymlinkEach => {
                    // the destination is a link (or links) to the source; the
                    // source is the file worth capturing
                    let mut source = TrackedEntry::new(
                        normalize(&request.source),
                        EntryKind::Source,
                        "source",
                        Policy::for_mode(FileMode::Track),
                    );
                    source.declared_in = declared_in;
                    set.push(source);
                }
                FileMode::Content => {
                    let mut output = TrackedEntry::new(
                        normalize(&request.target),
                        EntryKind::Output,
                        request.mode.name(),
                        Policy {
                            share: false,
                            ..request.policy
                        },
                    );
                    output.declared_in = declared_in;
                    set.push(output);
                }
                FileMode::Copy | FileMode::Template => {
                    let mut output = TrackedEntry::new(
                        normalize(&request.target),
                        EntryKind::Output,
                        request.mode.name(),
                        Policy {
                            share: false,
                            ..request.policy
                        },
                    );
                    output.source = Some(normalize(&request.source));
                    output.declared_in = declared_in.clone();
                    set.push(output);
                    let mut source = TrackedEntry::new(
                        normalize(&request.source),
                        EntryKind::Source,
                        "source",
                        Policy::for_mode(FileMode::Track),
                    );
                    source.declared_in = declared_in;
                    set.push(source);
                }
            }
        }
        for invalid in crate::system::files::invalid_declarations() {
            set.invalid.push(PathReason {
                path: invalid.target,
                reason: format!("{} ({})", invalid.reason, display_path(&invalid.config)),
            });
        }
        Ok(set)
    }

    /// Adds an entry; for the same path an explicit declaration outranks an
    /// implicit, derived, or source one.
    pub(crate) fn push(&mut self, entry: TrackedEntry) {
        if let Some(existing) = self
            .entries
            .iter_mut()
            .find(|existing| existing.path == entry.path)
        {
            if rank(entry.kind) > rank(existing.kind) {
                *existing = entry;
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
            .max_by_key(|entry| (entry.path.components().count(), rank(entry.kind)))
    }

    fn entry_index_for(&self, path: &Path) -> Option<usize> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| path.starts_with(&entry.path))
            .max_by_key(|(_, entry)| (entry.path.components().count(), rank(entry.kind)))
            .map(|(index, _)| index)
    }

    /// Whether a capture of this set would include `path`: under an entry,
    /// not excluded, not inside mise's own directories or a `.git`.
    pub(crate) fn would_capture(&self, path: &Path) -> Result<bool> {
        let Some(owner) = self.entry_for(path) else {
            return Ok(false);
        };
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
        let mut set = self.clone();
        let exclude = set.exclude_set()?;
        let hard = hard_exclusions();
        let home = normalize(&dirs::HOME);
        let config_dir = normalize(&global_config_dir());
        let credential_names = credential_names();
        let credential_globs = credential_globs();
        let mut walk = Walk::default();
        let mut links: Vec<(PathBuf, usize)> = vec![];
        let mut done = 0;
        // derived entries are appended while walking, so loop until none are
        while done < set.entries.len() {
            let index = done;
            done += 1;
            let entry = set.entries[index].clone();
            walk_entry(&set, index, &entry, &exclude, &hard, &mut walk, &mut links);
        }
        // targets of tracked symlinks inside $HOME become derived entries
        let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
        let mut round = 0;
        while !links.is_empty() && round < MAX_LINK_DEPTH {
            round += 1;
            let pending = std::mem::take(&mut links);
            for (link, owner) in pending {
                if !seen.insert(link.clone()) {
                    continue;
                }
                let Some(target) = resolve_link(&link) else {
                    continue;
                };
                if set
                    .entries
                    .iter()
                    .any(|entry| target.starts_with(&entry.path))
                {
                    continue; // already covered
                }
                if !target.starts_with(&home) || target == home {
                    continue;
                }
                if set
                    .entries
                    .iter()
                    .any(|entry| entry.path.starts_with(&target))
                {
                    continue; // an ancestor of an entry
                }
                if hard.iter().any(|dir| target.starts_with(dir)) || exclude.is_match(&target) {
                    continue;
                }
                let policy = set.entries[owner].policy;
                let mut derived =
                    TrackedEntry::new(target.clone(), EntryKind::Derived, "derived", policy);
                derived.source = Some(link.clone());
                walk.derived.push(DerivedRecord {
                    path: display_path(&target),
                    from: display_path(&link),
                });
                set.entries.push(derived);
                let index = set.entries.len() - 1;
                let entry = set.entries[index].clone();
                walk_entry(&set, index, &entry, &exclude, &hard, &mut walk, &mut links);
            }
        }
        // privacy: `.local.toml` anywhere, credential stores under the config
        // directory, unless a per-file declaration says otherwise
        for (path, (owner, policy)) in walk.files.iter_mut() {
            let entry = &set.entries[*owner];
            // only a per-file declaration that sets `share` or `backup`
            // itself overrides the defaults; naming a key file is not enough
            let explicit =
                entry.kind == EntryKind::Track && entry.path == *path && entry.policy.overridden;
            if explicit {
                continue;
            }
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if name.ends_with(".local.toml") {
                policy.share = false;
                walk.private.push(PrivateFile {
                    path: path.clone(),
                    reason: "machine-local configuration".into(),
                    policy: *policy,
                });
            } else if credential_globs.is_match(&name)
                || (path.starts_with(&config_dir) && credential_names.is_match(&name))
            {
                policy.share = false;
                policy.backup = false;
                walk.private.push(PrivateFile {
                    path: path.clone(),
                    reason: "credential store".into(),
                    policy: *policy,
                });
            }
        }
        walk.entries = set.entries.clone();
        // the snapshot roots
        let mut home_files = vec![];
        let mut fs_files = vec![];
        let mut home_bytes = 0;
        let mut fs_bytes = 0;
        for path in walk.files.keys() {
            if path.to_str().is_none() {
                eyre::bail!(
                    "history cannot represent a non-UTF-8 filename; refusing to change its bytes"
                );
            }
            let size = std::fs::symlink_metadata(path)
                .map(|m| m.len())
                .unwrap_or(0);
            if let Ok(rel) = path.strip_prefix(&home) {
                home_files.push(rel.to_path_buf());
                home_bytes += size;
            } else {
                let rel: PathBuf = path
                    .components()
                    .filter(|component| matches!(component, Component::Normal(_)))
                    .collect();
                fs_files.push(rel);
                fs_bytes += size;
            }
        }
        walk.roots.push(CaptureRoot {
            label: "home".into(),
            path: home,
            files: home_files,
            bytes: home_bytes,
        });
        if !fs_files.is_empty() {
            walk.roots.push(CaptureRoot {
                label: "fs".into(),
                path: PathBuf::from(std::path::MAIN_SEPARATOR.to_string()),
                files: fs_files,
                bytes: fs_bytes,
            });
        }
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
                share: entry.policy.share,
                backup: entry.policy.backup,
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
                share: private.policy.share,
                backup: private.policy.backup,
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
    links: &mut Vec<(PathBuf, usize)>,
) {
    let home = normalize(&dirs::HOME);
    let display = entry.display();
    if entry.path == home || home.starts_with(&entry.path) || entry.path.parent().is_none() {
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
        // A source a `[dotfiles]` entry still references is said so, not
        // silently dropped (`untrack` refuses to write such an exclusion)
        if exclude.is_match(&entry.path) {
            if entry.kind == EntryKind::Source {
                walk.omitted.push(PathReason {
                    path: display,
                    reason:
                        "excluded by [history] exclude although a [dotfiles] entry references it"
                            .to_string(),
                });
            }
            return;
        }
        match classify_file(&meta) {
            Ok(_) => {
                if meta.file_type().is_symlink() {
                    links.push((entry.path.clone(), index));
                }
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
            // under a source a `[dotfiles]` entry references: said once
            // per excluded subtree, never silently dropped
            if entry.kind == EntryKind::Source
                && !path.parent().is_some_and(|parent| exclude.is_match(parent))
            {
                walk.omitted.push(PathReason {
                    path: display_path(path),
                    reason:
                        "excluded by [history] exclude although a [dotfiles] entry references it"
                            .to_string(),
                });
            }
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
                if file_type.is_symlink() {
                    links.push((path.to_path_buf(), index));
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

fn rank(kind: EntryKind) -> u8 {
    match kind {
        EntryKind::Implicit => 0,
        EntryKind::Derived => 1,
        EntryKind::Source => 2,
        EntryKind::Output => 3,
        EntryKind::Track => 4,
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

/// The final target of a symlink chain, or `None` for a dangling link, a
/// cycle, or a chain longer than [`MAX_LINK_DEPTH`].
fn resolve_link(link: &Path) -> Option<PathBuf> {
    let mut current = link.to_path_buf();
    let mut seen = BTreeSet::new();
    for _ in 0..MAX_LINK_DEPTH {
        if !seen.insert(current.clone()) {
            return None;
        }
        let meta = std::fs::symlink_metadata(&current).ok()?;
        if !meta.file_type().is_symlink() {
            return Some(normalize(&current));
        }
        let dest = std::fs::read_link(&current).ok()?;
        current = if dest.is_absolute() {
            dest
        } else {
            current.parent()?.join(dest)
        };
        current = lexical(&current);
    }
    None
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

/// [`normalize`] for a tracked target: a symlink is tracked as the link
/// itself (its destination becomes a derived entry), so only the parent is
/// resolved.
pub(crate) fn normalize_target(path: &Path) -> PathBuf {
    let expanded = file::replace_path(path);
    let is_link =
        std::fs::symlink_metadata(&expanded).is_ok_and(|meta| meta.file_type().is_symlink());
    if is_link && let (Some(parent), Some(name)) = (expanded.parent(), expanded.file_name()) {
        let parent = if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent
        };
        return dunce::canonicalize(parent)
            .unwrap_or_else(|_| lexical(parent))
            .join(name);
    }
    dunce::canonicalize(&expanded).unwrap_or_else(|_| lexical(&expanded))
}

/// Where a chain of symlinks ends, lexically: the last link's target,
/// which need not exist (a target between two versions, say).
pub(crate) fn link_target(link: &Path) -> Option<PathBuf> {
    let mut current = link.to_path_buf();
    let mut seen = BTreeSet::new();
    for _ in 0..MAX_LINK_DEPTH {
        if !seen.insert(current.clone()) {
            return None;
        }
        let dest = std::fs::read_link(&current).ok()?;
        let joined = if dest.is_absolute() {
            dest
        } else {
            current.parent()?.join(dest)
        };
        current = lexical(&joined);
        if !current.is_symlink() {
            return Some(current);
        }
    }
    None
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
    if let Some(rest) = tree_path.strip_prefix("home/") {
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

    fn entry(path: &Path, kind: EntryKind, mode: &str) -> TrackedEntry {
        TrackedEntry::new(
            path.to_path_buf(),
            kind,
            mode,
            Policy::for_mode(FileMode::Track),
        )
    }

    #[test]
    fn the_most_specific_entry_owns_a_file() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        let child = root.join("child");
        let mut set = TrackedSet::default();
        set.push(entry(&root, EntryKind::Implicit, "implicit"));
        set.push(entry(&child, EntryKind::Track, "track"));
        assert_eq!(
            set.entry_for(&child.join("file")).map(|e| e.kind),
            Some(EntryKind::Track)
        );
        assert_eq!(
            set.entry_for(&root.join("other")).map(|e| e.kind),
            Some(EntryKind::Implicit)
        );
        assert!(set.entry_for(&tmp.path().join("elsewhere")).is_none());
        // an explicit declaration replaces an implicit one for the same path
        let mut set = TrackedSet::default();
        set.push(entry(&root, EntryKind::Implicit, "implicit"));
        set.push(entry(&root, EntryKind::Track, "track"));
        assert_eq!(set.entries.len(), 1);
        assert_eq!(set.entries[0].kind, EntryKind::Track);
        set.push(entry(&root, EntryKind::Source, "source"));
        assert_eq!(set.entries[0].kind, EntryKind::Track);
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

    #[test]
    fn symlink_chains_resolve_within_limits() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("target");
        std::fs::write(&target, "x").unwrap();
        #[cfg(unix)]
        {
            let link = tmp.path().join("link");
            std::os::unix::fs::symlink(&target, &link).unwrap();
            let link2 = tmp.path().join("link2");
            std::os::unix::fs::symlink("link", &link2).unwrap();
            assert_eq!(resolve_link(&link2), Some(normalize(&target)));
            let a = tmp.path().join("a");
            let b = tmp.path().join("b");
            std::os::unix::fs::symlink(&b, &a).unwrap();
            std::os::unix::fs::symlink(&a, &b).unwrap();
            assert_eq!(resolve_link(&a), None);
            assert_eq!(resolve_link(&tmp.path().join("missing")), None);
        }
    }
}
