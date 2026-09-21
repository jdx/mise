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

use std::collections::{BTreeMap, BTreeSet};
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
    /// The entry's own `include` globs, relative to its path and matched
    /// like `exclude`.
    ///
    /// `None` means no list was declared and the whole tree is captured.
    /// `Some` means one was, and only what it names is — including
    /// `Some([])`, which selects nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
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

    /// The compiled `include` patterns.
    ///
    /// A pattern that cannot be read is dropped, which makes the list
    /// select less: on a capture that means a file is not saved, and on
    /// a replay it means the file reads as unselected, which never
    /// deletes. Both directions are the safe one, and such a pattern is
    /// refused where it is written, so this is the last line rather than
    /// the only one.
    pub(crate) fn include_patterns(&self) -> Option<Vec<glob::Pattern>> {
        Some(
            self.include
                .as_ref()?
                .iter()
                .filter_map(|pattern| glob::Pattern::new(pattern).ok())
                .collect(),
        )
    }

    /// An `include` pattern that names something inside `directory`, if
    /// one does. Used to explain why a pattern reaching into a nested
    /// repository selects nothing, rather than leaving the user to
    /// wonder whether they wrote it wrong.
    /// Whether nothing in the entry's `include` list could select
    /// anything inside `directory`, so the walk can skip it whole.
    ///
    /// **Every way of not knowing answers "walk it".** With the limits
    /// counting what is captured, a directory walked for nothing costs
    /// time; one skipped by mistake costs a file the user asked to keep.
    /// So an entry with no list never prunes, and a directory this entry
    /// cannot even relate to itself is walked rather than skipped.
    pub(crate) fn include_prunes(&self, directory: &Path) -> bool {
        let Some(patterns) = self.include.as_ref() else {
            return false;
        };
        let Ok(rel) = directory.strip_prefix(&self.path) else {
            return false;
        };
        let components = path_components(rel);
        !patterns
            .iter()
            .any(|pattern| reaches_into(pattern, &components))
    }

    pub(crate) fn include_reaching_into(&self, directory: &Path) -> Option<&str> {
        let rel = directory.strip_prefix(&self.path).ok()?;
        let components = path_components(rel);
        self.include
            .iter()
            .flatten()
            .find(|pattern| reaches_into(pattern, &components))
            .map(String::as_str)
    }

    /// Whether the entry's `include` list selects `path`.
    ///
    /// **Rule 1: without an `include` list the whole tracked tree is
    /// considered. Rule 2: with one, only matching paths are. Rule 3: an
    /// explicit `exclude` still wins over `include`.** Rules 1 and 2 live
    /// here; rule 3 is the order the two lists are applied in, at every
    /// call site.
    ///
    /// **A list selects paths *inside* the entry, and the entry itself is
    /// not one of them**: its patterns are read relative to the entry, so
    /// none of them can name it. An entry that is itself a file is
    /// therefore left out by any list it declares — the same answer a
    /// declared-but-empty list gives, which is what makes "declared and
    /// empty" a choice rather than a special case. A directory entry's
    /// own path is not a file and is never captured either way.
    pub(crate) fn is_included(&self, path: &Path) -> bool {
        let Some(patterns) = self.include_patterns() else {
            return true;
        };
        match path.strip_prefix(&self.path) {
            Ok(rel) if !rel.as_os_str().is_empty() => {
                crate::system::files::is_excluded(&pattern_relative(rel), &patterns)
            }
            _ => false,
        }
    }

    /// Whether an `include` pattern names `path`, as opposed to `path`
    /// being the entry itself — which no pattern selected.
    ///
    /// Rule 4 asks this rather than `is_included`, so that declaring a
    /// list on an entry that *is* a credential-named file does not lift
    /// the guard for it. Overriding the guard means a pattern that names
    /// the file, which is something only a directory entry can have.
    fn selected_by_pattern(&self, path: &Path) -> bool {
        // the same matcher, not a second copy of it: the two questions
        // differ only in what they answer when no list was declared
        self.include.is_some() && self.is_included(path)
    }

    /// Why a capture leaves `path` out under this entry, if it does,
    /// with rule 4 applied.
    ///
    /// The one place rule 4 is decided. The walk, `mise dot save <path>`,
    /// `mise dot track`'s preflight and its dry run all ask here, so none
    /// of them can promise something the others will not do.
    pub(crate) fn capture_exclusion(&self, path: &Path) -> Option<&'static str> {
        let reason = capture_exclusion(path, &self.policy)?;
        // **An `include` list is a selection, and selection decides what
        // is captured.** A list the user wrote is the user choosing these
        // paths, so the builtin credential filtering that applies when no
        // list is given does not overrule it — a literal and a glob carry
        // the same authority, because "how specific was the pattern" is
        // not a question the user was answering.
        //
        // What the file is encrypted with is a separate question, decided
        // by `encrypt`. So a credential-like file selected for plaintext
        // capture is captured, and said out loud everywhere selection is
        // shown.
        if reason == CREDENTIAL_REASON && self.selected_by_pattern(path) {
            return None;
        }
        Some(reason)
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
            include: None,
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
    /// Credential-named files an `include` list selected for plaintext
    /// capture, so every report can say so.
    pub plaintext: Vec<PathReason>,
    /// For each entry with an `include` list, how many of its files the
    /// walk looked at, so a report can say how much the list selects.
    ///
    /// **This counts what was searched, which is not the whole tree.** A
    /// directory no pattern could reach into is skipped unopened — the
    /// point of the list — so what is inside it is never counted. An
    /// entry whose walk skipped anything is in [`Self::skipped`], and a
    /// report says "2 files" there rather than claiming "2 of 4".
    pub considered: BTreeMap<usize, u64>,
    /// Entries whose walk skipped a directory the `include` list could
    /// not reach into, so their `considered` count is a floor and not a
    /// total.
    pub skipped: BTreeSet<usize>,
    /// Repositories found inside a tracked directory, skipped whole.
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
                include: request.include.as_ref().map(|patterns| {
                    patterns
                        .iter()
                        .map(|pattern| pattern.as_str().to_owned())
                        .collect()
                }),
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
                    entry.include = request.include.as_ref().map(|patterns| {
                        patterns
                            .iter()
                            .map(|pattern| pattern.as_str().to_owned())
                            .collect()
                    });
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
        owning_entry_index(&self.entries, path)
    }

    /// Refuse to write a checkpoint while an `[history] exclude` rule
    /// cannot be used.
    ///
    /// **Walking always works; only storing refuses.** A capture that
    /// cannot apply an exclusion does not go ahead without it: dropping
    /// the rule broadens the snapshot to precisely the paths it was
    /// written to leave out, and that snapshot can be published to a
    /// connected origin. So the refusal sits where a checkpoint is about
    /// to be written, and says which rule, in which file, is the problem.
    /// Everything that only reads — `mise dot paths`, a dry-run preview,
    /// `mise dot status`, the watch set the watcher builds at startup —
    /// keeps working and reports the same rule, because the command the
    /// diagnostic sends the user to must not fail for the reason it is
    /// diagnosing. The watcher takes this refusal on its own save: it
    /// stays running and declines to capture.
    pub(crate) fn refuse_unusable_exclusions(&self) -> Result<()> {
        match unusable_exclusions(&self.exclude_set()?) {
            Some(report) => eyre::bail!(
                "{report}, so nothing is captured; fix or remove the pattern, then try again"
            ),
            None => Ok(()),
        }
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
        if owner.capture_exclusion(path).is_some() {
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
        Ok(!self.dropped(
            &self.exclude_set()?,
            owner,
            path,
            Asked::Exactly,
            kind_of(path),
        ))
    }

    /// Whether the selection lists drop `path`: the global globs read
    /// against its owning entry's path, then that entry's own `exclude`,
    /// which a global `!glob` does not re-include, and last its
    /// `include` list — so **rule 3 holds, an explicit `exclude` wins
    /// over `include`**.
    ///
    /// The one composition. `would_retain` adds the filesystem checks a
    /// capture also makes; the watcher asks this alone, because it is
    /// deciding what to watch rather than what a walk found; and
    /// synchronization asks it to tell a path selection stopped
    /// covering from one that was deleted. All of them pick the owner
    /// with [`owning_entry`], so they cannot disagree.
    ///
    /// **Every caller says which question it is asking.** There is no
    /// default, deliberately: this predicate answered the watcher's
    /// permissive question for everyone, and synchronization — which is
    /// asking retention's strict question — inherited it and deleted a
    /// file on one machine because another machine stopped selecting it.
    /// A default is what let that happen, so the next reader has to
    /// choose rather than be handed the watcher's answer.
    pub(crate) fn excluded_by_lists(
        &self,
        exclude: &ExcludeSet,
        path: &Path,
        asked: Asked,
    ) -> bool {
        match self.entry_for(path) {
            Some(owner) => self.dropped(exclude, owner, path, asked, kind_of(path)),
            None => true,
        }
    }

    /// **One predicate, two axes: which question is being asked, and
    /// what is being asked about.** Stated here and implemented here,
    /// because four separate bugs came from these questions being
    /// answered by parallel copies.
    ///
    /// The *kind* axis:
    ///
    /// - For a **file**, the question is "is this file selected".
    /// - For a **directory**, it is "does this tree contain any
    ///   selected path". A directory is covered when something beneath
    ///   it is selected, although no pattern ever selects the directory
    ///   itself: an anchored list like `rules/**` does not name `rules`.
    ///   Asking `is_included` of a directory reported a tracked tree as
    ///   uncaptured, so `mise dot track` warned about a symlink whose
    ///   source it was capturing, and a rollback skipped the empty
    ///   subdirectories a capture still walks.
    /// - A path with **no kind** — removed or renamed — is where the
    ///   two disagree, and the reader decides.
    ///
    /// The *reader* axis, which only a path with no kind is left to:
    ///
    /// - A **capture** asks "should I store this path now", and
    ///   **retention** asks "is this still selected", to decide whether
    ///   a previously saved version is carried forward when the file
    ///   cannot be read now. Both answer from the patterns alone:
    ///   [`Asked::Exactly`]. Answering permissively would keep files
    ///   the user has taken out of their `include` list in history
    ///   indefinitely, for no better reason than that they happened to
    ///   be unreadable at save time.
    /// - **Synchronization** asks "is this path still managed here, so
    ///   that its absence from the incoming snapshot is a deletion to
    ///   replay". That is retention's question in other words, so it is
    ///   [`Asked::Exactly`] too. Asked permissively, a local copy whose
    ///   kind cannot be read — it sits under a directory this machine
    ///   cannot search — looks managed although no pattern selects it,
    ///   and a narrowing made on another machine deletes it here.
    /// - The **watcher** asks "might this event matter", and is
    ///   deliberately permissive: a path that has just vanished counts
    ///   as whatever it could have been, so a deleted tree still wakes
    ///   it. [`Asked::Possibly`].
    fn dropped(
        &self,
        exclude: &ExcludeSet,
        owner: &TrackedEntry,
        path: &Path,
        asked: Asked,
        kind: Kind,
    ) -> bool {
        // **A tracked directory's own root is not something the global
        // list judges.** The walk never tests it: it steps past the root
        // and filters what is inside. A bare pattern that happens to
        // equal the directory's name would otherwise say "excluded"
        // about an entry whose contents a capture takes in full — and a
        // rollback would then stop recording that the directory existed.
        // A file entry is tested, exactly as the walk tests it.
        // A root that cannot be read — it has just been removed — is
        // not judged either. Nothing in the declaration says which kind
        // an entry is (`mode = "track"` covers both), and of the two
        // answers only this one is safe: a removed tracked tree is a
        // change history has to notice, and a pattern equal to its name
        // swallowing that event would leave its files in history after
        // they are gone.
        if inside_nested_repository(owner, path) {
            return true;
        }
        let judged = path != owner.path || kind == Kind::File;
        if judged && exclude.is_match(path, &owner.path) {
            return true;
        }
        if owner.is_excluded(path) {
            return true;
        }
        match kind {
            Kind::File => !owner.is_included(path),
            Kind::Directory => owner.include_prunes(path),
            Kind::Unknown => match asked {
                Asked::Exactly => !owner.is_included(path),
                Asked::Possibly => !owner.is_included(path) && owner.include_prunes(path),
            },
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
        // the rule cannot narrow this walk, so say what the listing now
        // holds that it was written to leave out. Refusing here would
        // break `mise dot paths`, the watch set, and the previews — the
        // very commands that let someone see and fix the rule.
        if let Some(report) = unusable_exclusions(&exclude) {
            walk.warnings.push(format!(
                "{report}; it is ignored here, so this lists paths it would leave out, and nothing is captured until it is fixed"
            ));
        }
        for (index, entry) in set.entries.iter().enumerate() {
            if selected.is_some_and(|selected| !selected.contains(&index)) {
                continue;
            }
            walk_entry(set, index, entry, &exclude, &hard, &mut walk);
        }
        // Protected files are excluded from capture itself, never kept in
        // a hidden local-only history. Explicit encryption permits key files
        // to be tracked without storing their plaintext.
        walk.files.retain(|path, (index, policy)| {
            let Some(owner) = set.entries.get(*index) else {
                return capture_exclusion(path, policy).is_none();
            };
            // rule 4 lives on the entry, so every reader gets the same
            // answer; a file it lets through is announced, because it
            // goes into history in plaintext and to any connected origin
            let Some(reason) = owner.capture_exclusion(path) else {
                if capture_exclusion(path, policy).is_some() {
                    walk.plaintext.push(PathReason {
                        path: display_path(path),
                        reason: "selected by an include list; saved in plaintext".into(),
                    });
                }
                return true;
            };
            walk.omitted.push(PathReason {
                path: display_path(path),
                reason: reason.into(),
            });
            false
        });
        // a credential-named file saved because an entry named it exactly
        // is worth saying out loud on every capture, not only in
        // `mise dot paths`: it goes to any connected origin as plaintext
        for plaintext in &walk.plaintext {
            walk.warnings.push(format!(
                "{}: an include list selects it, so it is saved in plaintext although it looks like a credential store; `encrypt = true` saves it encrypted instead",
                plaintext.path
            ));
        }
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
                include: entry.include.clone(),
            })
            .collect();
        let mut omitted = walk.omitted.clone();
        omitted.extend(self.invalid.iter().cloned());
        Coverage {
            entries,
            exclude: self.exclude.clone(),
            // a marker rather than the rules themselves: a checkpoint
            // written before this matcher cannot be read with it
            matcher: Some(MATCHER_VERSION),
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
        // an `include` list selects paths inside a tracked directory, so
        // a file entry that declares one selects nothing at all. Said out
        // loud rather than dropped quietly: a file that stops being saved
        // is exactly the kind of thing a user needs told.
        if !entry.is_included(&entry.path) {
            walk.omitted.push(PathReason {
                path: display,
                reason: "its include list selects nothing: a list selects paths inside a tracked directory, and this entry is a file".into(),
            });
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
    let entry_include = entry.include_patterns();
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
        let file_type = candidate.file_type();
        // **A repository inside a tracked directory is structure, not
        // selection.** Asked of every directory before any exclude,
        // include or re-include rule, because no rule in a selection
        // list may send the walk into another working tree. A directory
        // an exclusion drops is not always pruned — a later `!` rule can
        // re-include something below it — so the walk descends, and with
        // this check further down it descended into repositories, and a
        // re-include then captured their files. `would_retain` refuses
        // those same paths, so capture and retention disagreed about
        // files that a connected origin would have received.
        //
        // A repository found under an excluded directory is reported
        // too: it costs one `.git` probe per directory the walk was
        // about to skip anyway, and saying "this is a repository" about
        // a path the user will look for is better than saying nothing.
        if file_type.is_dir() && path.join(".git").exists() {
            // A repository found inside a tracked directory is skipped
            // whole, and nothing is written for it — not its files, not
            // a commit pointer. A pointer would name objects this
            // history does not have, and there is a supported way to
            // get the files: track the repository itself. An `include`
            // pattern that names paths inside one is called out, so the
            // skip does not read as the list being wrong.
            let reason = match entry.include_reaching_into(path) {
                Some(pattern) => format!(
                    "{NESTED_REPOSITORY_REASON}; the include pattern {pattern:?} selects nothing inside it"
                ),
                None => NESTED_REPOSITORY_REASON.to_string(),
            };
            walk.nested.push(PathReason {
                path: display_path(path),
                reason,
            });
            walker.skip_current_dir();
            continue;
        }
        // an excluded directory is not entered at all: `~/.codex/sessions`
        // can hold tens of thousands of files, and none of them can come
        // back into the capture
        if file_type.is_dir() && exclude.prunes_directory(path, &entry.path) {
            walker.skip_current_dir();
            continue;
        }
        if exclude.is_match(path, &entry.path) {
            continue;
        }
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
            // **What the list cannot select is not walked.** The mirror
            // of exclude pruning, conservative in the same direction:
            // `include_reaching_into` says a pattern could name something
            // here whenever it cannot rule it out, so a missed skip costs
            // a walk while a wrong one would cost a file. A repository
            // was already decided above, before any of this.
            if entry.include_prunes(path) {
                walk.skipped.insert(index);
                walker.skip_current_dir();
            }
            continue;
        }
        // rule 2: with an `include` list, only matching paths are
        // considered. Applied after the exclude lists, so rule 3 holds: an
        // explicit exclusion wins.
        if let Some(entry_include) = &entry_include {
            *walk.considered.entry(index).or_default() += 1;
            match path.strip_prefix(&entry.path) {
                Ok(rel)
                    if !crate::system::files::is_excluded(
                        &pattern_relative(rel),
                        entry_include,
                    ) =>
                {
                    continue;
                }
                Err(_) => continue,
                Ok(_) => {}
            }
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
                // **The limits bound what is captured, so they count what
                // is captured.** A path an `include` list rejected is not
                // in the snapshot and must not spend the budget: a
                // growing `sessions/` directory would otherwise exhaust
                // it and the one file the list names would never be
                // saved — the feature defeated by exactly the files it
                // exists to leave out.
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

/// Whether `patterns` (an entry's own `include` list, relative to
/// `entry_path`) select `path`; the entry path itself never is, exactly
/// as [`TrackedEntry::is_included`] answers it. The mirror of
/// [`excluded_by_entry`], for a replay reading the list a checkpoint
/// recorded.
pub(crate) fn included_by_entry(entry_path: &Path, patterns: &[String], path: &Path) -> bool {
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

/// Whether two display paths name the same path.
///
/// **The same path reaches this comparison in two spellings.** A walk
/// writes display paths with [`crate::file::display_path`], which uses
/// the platform separator, and a record read back out of a commit
/// rebuilds them with [`tree_path_to_display`], which always writes `/`.
/// On Windows one path is then `~\.codex\plugin` and the other
/// `~/.codex/plugin`, and comparing the strings says they are two
/// different paths.
pub(crate) fn display_paths_equal(left: &str, right: &str) -> bool {
    left.split(['/', '\\']).eq(right.split(['/', '\\']))
}

/// Whether the display path `path` is `root` itself or lies below it.
///
/// **Two display paths are compared through one normalized form, never
/// byte for byte.** Within a host both spellings occur: one path may
/// have been written by `display_path`, with the host's separator, while
/// the other was rebuilt from a tree path with `/`. They name the same
/// file, and every caller that compares them — coverage, replay, status
/// — goes through here so there is one place this can be wrong.
///
/// **The convention is the reading host's, and that is the only one
/// these strings are ever in.** A checkpoint records portable `home/…`
/// tree paths; every display string is rebuilt from those locally, so
/// none of them carries another host's separator. That is what makes it
/// safe — and necessary — to read a backslash as an ordinary character
/// in a file name on unix, where it is one: treating `~/x\y` as a file
/// inside `~/x` would let one entry appear to own another's paths, and a
/// replay would then judge a live file by the wrong root and the wrong
/// exclusions.
pub(crate) fn display_under(path: &str, root: &str) -> bool {
    let path = display_separators(path);
    let root = display_separators(root);
    path == root
        || path
            .strip_prefix(&root)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// A display path in the one form comparisons use: `/`-separated.
fn display_separators(path: &str) -> String {
    // on Windows both characters separate; on unix a backslash is part of
    // a file's name, and rewriting it would invent a directory boundary
    // that does not exist
    if cfg!(windows) {
        path.replace('\\', "/")
    } else {
        path.to_string()
    }
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

    /// Say what this walk had to report: a truncated tree, an exclusion
    /// it could not apply.
    ///
    /// **Every reader says it, not only the one that captures.** A walk
    /// that only lists still walked under the same rules, and a rule it
    /// could not use changes what the listing holds — so a command that
    /// showed the listing silently would be the one place the problem is
    /// invisible.
    pub(crate) fn report_warnings(&self) {
        for warning in &self.warnings {
            warn!("history: {warning}");
        }
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
    /// Credential-named files this entry captures in the clear, so a dry
    /// run promises what the first save will really do.
    pub plaintext: Vec<PathReason>,
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
        preview.plaintext = self
            .plaintext
            .iter()
            .filter(|r| owned(r))
            .cloned()
            .collect();
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

pub(crate) fn with_separators(n: usize) -> String {
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
    list: PatternList,
}

impl ExcludeSet {
    pub(crate) fn new(globs: &[String]) -> Result<Self> {
        Ok(Self {
            list: PatternList::new(globs)?,
        })
    }

    /// The `[history] exclude` rules this matcher cannot use.
    ///
    /// **An exclusion that does not work must not be read as no
    /// exclusion.** A capture asks here and refuses rather than store
    /// the files the broken rule named, because those are the files the
    /// user wrote it to keep out, and a capture can be published to a
    /// connected origin.
    pub(crate) fn unusable(&self) -> &[(String, String)] {
        &self.list.unusable
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
        self.decide(path, root, false)
    }

    /// Whether the walk can skip `dir` whole: the rules exclude the
    /// directory itself, or a rule of the form `<P>/**` names everything
    /// under it. The second case is the spelling the docs show, and
    /// without it `~/.codex/sessions` was still enumerated file by file.
    ///
    /// This belongs here and not in [`Self::is_match`] because pruning is
    /// a pure optimization: the matcher is ancestor-aware, so every file
    /// under an excluded directory is excluded whether or not the walk
    /// visits it. A missed prune is only slower, and a prune that fires
    /// cannot change which files are captured.
    pub(crate) fn prunes_directory(&self, dir: &Path, root: &Path) -> bool {
        self.decide(dir, root, true) && !self.may_reinclude_below(dir)
    }

    fn decide(&self, path: &Path, root: &Path, as_directory: bool) -> bool {
        if self.list.rules.is_empty() {
            return false;
        }
        // Everything is compared as `/`-separated text. A pattern is
        // written that way whatever platform it is read on, a path is not,
        // and normalizing once here is the only place the two forms can
        // fail to line up.
        // **A path outside the tracked root has no path relative to
        // it.** Falling back to the absolute path here let a relative
        // rule — compiled as `**/sessions/**` — match
        // `/somewhere/else/sessions/file`, so a capture would have
        // considered paths no entry covers, the watcher would have
        // followed them, and a replay would have skipped restoring files
        // the checkpoint never held. An absolute rule still applies:
        // those are matched against the path itself, which is a question
        // that does not need a root.
        let relative_path = if path == root {
            path.file_name().map(PathBuf::from)
        } else {
            path.strip_prefix(root).map(Path::to_path_buf).ok()
        };
        let mut candidates = vec![];
        for ancestor in path.ancestors() {
            candidates.push(separators(ancestor));
            if ancestor == root {
                break;
            }
        }
        let relative: Vec<String> = relative_path
            .iter()
            .flat_map(|relative| relative.ancestors().collect::<Vec<_>>())
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
            .rfind(|rule| {
                if as_directory {
                    rule.covers_directory(&candidates, &relative, &components)
                } else {
                    rule.matches_any(&candidates, &relative, &components)
                }
            })
            .is_some_and(|rule| !rule.negated)
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
    /// Deliberately conservative, and more so than it may look: only an
    /// absolute negation has a directory of its own to compare against.
    /// A name pattern (`!important.log`) or a relative one
    /// (`!cache/keep.conf`) matches at any depth by design, so there is
    /// no subtree it is confined to and no directory that can be proved
    /// safe to skip — one such rule anywhere in `[history] exclude`
    /// turns pruning off for every tracked entry. That costs a walk of
    /// directories whose files are then filtered one by one; the
    /// alternative costs files the user asked to keep.
    pub(crate) fn may_reinclude_below(&self, dir: &Path) -> bool {
        // the comparison is between a pattern prefix and a path, so it
        // gets the same normalization the matcher gives them: `/` on
        // every platform, compared component by component
        let dir = separators(dir);
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
                    .any(|root| under_or_above(root, &dir))
            })
    }
}

/// Which question a reader of the selection lists is asking.
///
/// Every caller of [`TrackedSet::excluded_by_lists`] names one. There is
/// no default: inheriting the watcher's permissive answer by omission is
/// how synchronization came to delete a file another machine had merely
/// stopped selecting.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Asked {
    /// Is this path itself selected? What a capture stores and what
    /// retention keeps.
    Exactly,
    /// Could this path matter? What the watcher wakes for.
    Possibly,
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
    /// Rules this matcher cannot use, as `(pattern, reason)`, kept
    /// rather than warned about and forgotten. See
    /// [`ExcludeSet::unusable`].
    unusable: Vec<(String, String)>,
}

/// What a pattern is matched against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Anchor {
    /// No path separator: any single component of the root-relative path.
    Name,
    /// Absolute after `~` expansion: the path itself or an ancestor.
    Absolute,
    /// Anything else: the root-relative path, at any depth below the
    /// tracked root. Never the absolute path — a `sessions` directory
    /// somewhere above the tracked root is not what `sessions/**` means.
    Relative,
}

#[derive(Debug)]
struct PatternRule {
    matchers: Vec<globset::GlobMatcher>,
    /// For a glob of the form `<P>/**`, a matcher for `<P>` itself. The
    /// glob matches what is under the directory, never the directory
    /// path, so this is what lets the walk recognise the directory and
    /// skip it whole.
    directory_matchers: Vec<globset::GlobMatcher>,
    /// For a negated path rule, the directories its matches must live
    /// under: each glob's literal leading prefix, cut at the last
    /// separator, `/`-separated like everything else the matcher
    /// compares. Empty means the rule could match anywhere.
    reinclude_roots: Vec<String>,
    negated: bool,
    anchor: Anchor,
}

impl PatternRule {
    /// Compiles one already-expanded pattern. `negated` is given, never
    /// read out of the text: the pattern is opaque from here on.
    fn compile(body: &str, negated: bool) -> Result<Self> {
        let anchor = anchor_of(body);
        let globs = if anchor == Anchor::Name {
            vec![body.to_string()]
        } else {
            anchored_globs(body)
        };
        let compile = |glob: &str| -> Result<globset::GlobMatcher> {
            Ok(build_glob(glob, anchor)?.compile_matcher())
        };
        let mut matchers = vec![];
        let mut directory_matchers = vec![];
        for glob in &globs {
            matchers.push(compile(glob)?);
            if let Some(directory) = glob.strip_suffix("/**")
                && !directory.is_empty()
            {
                directory_matchers.push(compile(directory)?);
            }
        }
        let reinclude_roots = if negated && anchor == Anchor::Absolute {
            globs.iter().filter_map(|glob| literal_root(glob)).collect()
        } else {
            vec![]
        };
        Ok(Self {
            matchers,
            directory_matchers,
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

    /// Whether the rule covers `dir` as a whole: it matches the directory
    /// itself, or it is a `<P>/**` naming everything under it.
    fn covers_directory(
        &self,
        candidates: &[String],
        relative: &[String],
        components: &[&str],
    ) -> bool {
        if self.matches_any(candidates, relative, components) {
            return true;
        }
        let against: &[String] = match self.anchor {
            Anchor::Absolute => candidates,
            Anchor::Relative => relative,
            // a name glob has no `/`, so it has no `/**` form either
            Anchor::Name => return false,
        };
        against.iter().any(|candidate| {
            self.directory_matchers
                .iter()
                .any(|matcher| matcher.is_match(Path::new(candidate)))
        })
    }
}

/// A relative path as the components a pattern is matched against.
fn path_components(rel: &Path) -> Vec<String> {
    rel.components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect()
}

/// Whether an `include` pattern could select something inside the
/// directory at `components` (relative to the tracked entry).
///
/// **Asked with the same glob semantics selection uses, not with a
/// literal prefix.** `Spoons/*` reaches into `Spoons`, and so do
/// `**/Sky.spoon` and a bare `Sky.spoon`, because a pattern without a
/// separator matches a name at any depth. A prefix comparison sees only
/// the first of those, and the user is then told their pattern selects
/// nothing without being told which pattern.
fn reaches_into(pattern: &str, components: &[String]) -> bool {
    // **Pruning is an optimization and must never change what is
    // selected, so it reads a pattern exactly as the matcher reads it.**
    // On Windows a backslash separates; on unix it is a character in a
    // name or a glob escape, and rewriting it here would invent a
    // directory boundary the matcher does not see — and then skip a
    // directory whose files the list still selects. Same rule as
    // `pattern_relative` and `display_separators`, not a third one.
    let pattern = display_separators(pattern);
    // a name matches a component at any depth, so it can name a file
    // inside any directory
    if !pattern.contains('/') {
        return true;
    }
    let parts: Vec<&str> = pattern.split('/').filter(|part| !part.is_empty()).collect();
    let mut parts = parts.as_slice();
    let mut rest = components;
    loop {
        match (parts.first(), rest.first()) {
            // `**` descends as far as it likes
            (Some(&"**"), _) => return true,
            // One side ran out with everything so far matching, and all
            // three ways that happens reach in: the pattern names
            // something inside the directory, or the directory itself,
            // or an ancestor of it — and a pattern matching a directory
            // takes everything under it.
            (Some(_), None) | (None, _) => return true,
            (Some(part), Some(component)) => {
                // a component pattern that cannot be read is not a
                // mismatch: this answer decides whether a directory is
                // walked at all, so "cannot tell" descends
                let matches = glob::Pattern::new(part)
                    .map(|glob| glob.matches(component))
                    .unwrap_or(true);
                if !matches {
                    return false;
                }
                parts = &parts[1..];
                rest = &rest[1..];
            }
        }
    }
}

/// The one way a pattern body becomes a glob.
///
/// **What validation asks and what compilation does are the same
/// question, so they go through the same builder.** A path glob is
/// gitignore-like: `*` stops at a separator, `**` crosses them. A name
/// glob matches one component, so there is no separator in it to stop
/// at. Building a candidate any other way would let `mise dot exclude`
/// accept a pattern that [`PatternRule::compile`] then drops with a
/// warning — the exclusion the user asked for silently doing nothing.
fn build_glob(glob: &str, anchor: Anchor) -> std::result::Result<Glob, globset::Error> {
    if anchor == Anchor::Name {
        Glob::new(glob)
    } else {
        globset::GlobBuilder::new(glob)
            .literal_separator(true)
            .build()
    }
}

/// The anchor a pattern body compiles under, which decides how its globs
/// are built.
fn anchor_of(body: &str) -> Anchor {
    if !is_path_anchored(body) {
        Anchor::Name
    } else if file::replace_path(Path::new(body)).is_absolute() {
        Anchor::Absolute
    } else {
        Anchor::Relative
    }
}

/// Why this matcher cannot use a pattern, if it cannot.
///
/// Shared with `mise dot exclude`, which refuses such a pattern rather
/// than writing it, so the message a user sees when they type one is the
/// message the loader would have warned about later.
pub(crate) fn unusable_pattern(body: &str) -> Option<String> {
    if body.contains('$') {
        return Some(
            "environment variables are not supported in exclusion patterns; write `~/…` or an absolute path".into(),
        );
    }
    // Every form the pattern is compiled from is checked, not just the
    // last one. An anchored pattern becomes two globs — as written and
    // normalized — and they are not equally valid: a directory literally
    // named `link[x` makes the written form an unclosed character class
    // while its normalized form, through a symlink, has no bracket at
    // all. Checking one of them would accept a pattern that
    // `PatternRule::compile` then drops with a warning, which is the
    // opposite of what this exists to prevent.
    let anchor = anchor_of(body);
    let probes = if anchor == Anchor::Name {
        vec![body.to_string()]
    } else {
        anchored_globs(body)
    };
    probes
        .iter()
        .find_map(|probe| build_glob(probe, anchor).err().map(|err| err.to_string()))
}

/// Whether a pattern names a path rather than a file name. `~` alone is
/// a path, so it is expanded before the question is asked.
fn is_path_anchored(body: &str) -> bool {
    let body = &file::replace_path(Path::new(body))
        .to_string_lossy()
        .into_owned();
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
    // `~` is the one expansion a pattern gets, and it decides whether the
    // pattern is absolute, so it happens before anything else is asked
    // about it
    let expanded = file::replace_path(Path::new(body));
    let expanded = expanded.as_path();
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
fn literal_root(glob: &str) -> Option<String> {
    let literal = match glob.find(['*', '?', '[', '{']) {
        Some(index) => &glob[..index],
        None => glob,
    };
    let root = literal.rsplit_once('/').map(|(root, _)| root)?;
    (!root.is_empty()).then(|| root.to_string())
}

/// Whether two `/`-separated paths are the same or one contains the
/// other, compared by whole components so `/a/bc` is not below `/a/b`.
fn under_or_above(one: &str, other: &str) -> bool {
    let (shorter, longer) = if one.len() <= other.len() {
        (one, other)
    } else {
        (other, one)
    };
    longer == shorter
        || longer
            .strip_prefix(shorter)
            .is_some_and(|rest| rest.starts_with('/'))
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
    pub(crate) fn new(patterns: &[String]) -> Result<Self> {
        let mut rules = vec![];
        let mut unusable = vec![];
        for pattern in patterns {
            let (body, negated) = match pattern.strip_prefix('!') {
                Some(rest) => (rest, true),
                None => (pattern.as_str(), false),
            };
            // **A rule this matcher cannot use is recorded, not dropped
            // quietly.** Building the matcher still never fails: a list
            // lives in configuration that was written against an older
            // mise, and a matcher that refuses to build takes the
            // watcher down with it. What must not happen is the capture
            // going ahead without the rule, which is the one case where
            // dropping it broadens the snapshot to exactly the files
            // the rule existed to leave out. So the rule is kept here
            // and [`ExcludeSet::unusable`] hands it to the capture,
            // which refuses. `mise dot exclude` refuses such a pattern
            // at the point the user writes it, so nothing new gets in.
            if let Some(reason) = unusable_pattern(body) {
                unusable.push((pattern.clone(), reason));
                continue;
            }
            match PatternRule::compile(body, negated) {
                Ok(rule) => rules.push(rule),
                Err(err) => unusable.push((pattern.clone(), err.to_string())),
            }
        }
        Ok(Self { rules, unusable })
    }
}

/// What a path is, as far as the caller of the selection predicate can
/// tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A file, a symlink, or anything else that is not a directory.
    File,
    Directory,
    /// Nothing left to ask: the path has been removed or renamed.
    Unknown,
}

/// The kind to judge `path` as.
///
/// **Nothing in a declaration says which kind an entry is**, and no
/// field is a safe proxy for one: `mode = "track"` covers a file and a
/// directory alike, and an `include` list on an entry that turns out to
/// be a file is reported and kept, not removed from the set, so reading
/// the list as "therefore a directory" gives that entry the wrong
/// answer. The filesystem is asked instead, and a path that is not
/// there has no kind — which is the case [`TrackedSet::dropped`] hands
/// to the reader, because that is the one case where a file's answer
/// and a directory's answer differ and neither is available.
fn kind_of(path: &Path) -> Kind {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => Kind::Directory,
        Ok(_) => Kind::File,
        Err(_) => Kind::Unknown,
    }
}

/// Whether `path` lies inside a repository nested below `owner`'s root.
///
/// **Structure, not selection.** A working tree inside a tracked
/// directory is never captured — its files belong to that repository,
/// and history has none of its objects — so no pattern, in any list, can
/// bring one back. Every reader asks this before it asks what the lists
/// say: the walk when it decides whether to descend, `would_retain`
/// when it decides whether a saved version is still covered, and the
/// watcher, which would otherwise wake for every write inside a
/// checked-out repository it can never save.
fn inside_nested_repository(owner: &TrackedEntry, path: &Path) -> bool {
    path.ancestors()
        .skip(1)
        .take_while(|ancestor| ancestor.starts_with(&owner.path) && *ancestor != owner.path)
        .any(|ancestor| ancestor.join(".git").exists())
}

/// Which `[history] exclude` rules cannot be used, each named with the
/// file that declares it, or `None` when every rule compiles.
///
/// One wording for both readers: the walk reports it and carries on, and
/// [`TrackedSet::refuse_unusable_exclusions`] turns the same sentence into
/// the refusal a checkpoint gets, so the diagnostic a user is asked to act
/// on says the same thing wherever they meet it.
fn unusable_exclusions(exclude: &ExcludeSet) -> Option<String> {
    let unusable = exclude.unusable();
    if unusable.is_empty() {
        return None;
    }
    let sources: Vec<String> = unusable
        .iter()
        .map(|(pattern, reason)| {
            let files = super::config::exclusion_sources(pattern);
            match files.is_empty() {
                true => format!("{pattern:?}: {reason}"),
                false => format!(
                    "{pattern:?} in {}: {reason}",
                    files
                        .iter()
                        .map(display_path)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            }
        })
        .collect();
    Some(format!(
        "[history] exclude cannot be applied: {}",
        sources.join("; ")
    ))
}

/// The matcher a checkpoint's `exclude` list was read with. Bumped only
/// when a change could make a pattern match *less* than it used to, so a
/// replay of an older checkpoint does not conclude a path was absent
/// when the older matcher would have called it excluded.
pub(crate) const MATCHER_VERSION: u32 = 1;

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
    owning_entry_index(entries, path).map(|index| &entries[index])
}

/// Where [`owning_entry`]'s answer sits in `entries`.
///
/// **The search hands back the index; nothing looks the entry up again.**
/// A second search — by identity or by value — can come back empty, and
/// an empty answer here does not read as "something went wrong", it reads
/// as "no entry covers this path". That turns a covered path into an
/// uncovered one, and every selection and coverage decision downstream
/// then goes the other way.
pub(crate) fn owning_entry_index(entries: &[TrackedEntry], path: &Path) -> Option<usize> {
    entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| path.starts_with(&entry.path))
        .max_by_key(|(_, entry)| entry.path.components().count())
        .map(|(index, _)| index)
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
        // **Normalized once, then used for everything after.** A
        // recorded display path keeps the separator of the host that
        // wrote it, so `~\.config\mise` counted as components on a unix
        // reader is one component, not three — and the most specific
        // entry would lose to a shallower one. The comparison above
        // already reads both spellings as the same path; the ranking has
        // to read them the same way too.
        .max_by_key(|item| display_depth(key(item)))
}

/// How deep a display path is, whichever host's separator it carries.
fn display_depth(path: &str) -> usize {
    display_separators(path)
        .split('/')
        .filter(|component| !component.is_empty() && *component != ".")
        .count()
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
                include: None,
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

    /// What the CLI accepts is what the matcher compiles. The two asked
    /// the same question through different builders once, which is how a
    /// pattern gets accepted at the prompt and then dropped with a
    /// warning at load — the exclusion silently doing nothing.
    #[test]
    fn every_pattern_the_cli_accepts_compiles() {
        for body in [
            "cache",
            "*.log",
            "!*.log",
            "sessions/**",
            "~/.codex/sessions/**",
            "./rules/*.md",
            "a[bc]d",
            "**/node_modules",
            "{a,b}/**",
        ] {
            let negated = body.starts_with('!');
            let rule = body.strip_prefix('!').unwrap_or(body);
            assert_eq!(
                unusable_pattern(rule).is_none(),
                PatternRule::compile(rule, negated).is_ok(),
                "the CLI and the matcher disagree about {body:?}"
            );
        }
        // and one the matcher cannot compile is refused rather than
        // accepted and dropped
        assert!(unusable_pattern("rules/[unclosed/**").is_some());
        assert!(PatternRule::compile("rules/[unclosed/**", false).is_err());
    }

    /// A pattern is compiled from more than one glob when it is
    /// anchored, and the refusal has to cover all of them: a form that
    /// only `PatternRule::compile` rejects is dropped with a warning long
    /// after `mise dot exclude` accepted the pattern.
    #[cfg(unix)]
    #[test]
    fn a_pattern_is_refused_when_any_form_it_compiles_to_is_unusable() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("real")).unwrap();
        // a directory whose real name opens a character class that the
        // path it resolves to does not
        let link = tmp.path().join("link[x");
        std::os::unix::fs::symlink(tmp.path().join("real"), &link).unwrap();
        let body = format!("{}/**", link.display());

        let forms = anchored_globs(&body);
        assert_eq!(forms.len(), 2, "expected two forms, got {forms:?}");
        assert!(
            Glob::new(&forms[1]).is_ok(),
            "the normalized form is the usable one: {forms:?}"
        );
        assert!(
            unusable_pattern(&body).is_some(),
            "the written form is unusable, so the pattern is refused: {forms:?}"
        );
    }

    /// A recorded display path carries the separator of the host that
    /// wrote it, and the most specific entry has to win on either host —
    /// the comparison and the ranking must read the same path the same
    /// way.
    #[test]
    fn the_owner_of_a_recorded_path_is_the_same_on_either_host() {
        struct Recorded(&'static str);
        for (entries, path, expected) in [
            (
                vec![Recorded("~/.config"), Recorded("~/.config/mise")],
                "~/.config/mise/config.toml",
                "~/.config/mise",
            ),
            // the same set spelled the way `display_path` writes it on
            // Windows, where both characters separate and the two
            // spellings mix within one host
            #[cfg(windows)]
            (
                vec![Recorded("~\\.config"), Recorded("~\\.config\\mise")],
                "~\\.config\\mise\\config.toml",
                "~\\.config\\mise",
            ),
            #[cfg(windows)]
            (
                vec![Recorded("~/.config"), Recorded("~\\.config\\mise")],
                "~/.config/mise/config.toml",
                "~\\.config\\mise",
            ),
            // on unix a backslash is part of a name, so this entry is one
            // directory called `.config\mise` and owns nothing under
            // `~/.config`
            #[cfg(unix)]
            (
                vec![Recorded("~/.config"), Recorded("~/.config\\mise")],
                "~/.config/mise/config.toml",
                "~/.config",
            ),
        ] {
            let owner = owning_display(&entries, path, |entry| entry.0);
            assert_eq!(
                owner.map(|entry| entry.0),
                Some(expected),
                "{path} under {:?}",
                entries.iter().map(|entry| entry.0).collect::<Vec<_>>()
            );
        }
    }

    /// The walk steps past a tracked directory's own root and filters
    /// what is inside, so a bare global pattern equal to that
    /// directory's name must not make retention or the watcher call the
    /// entry excluded — and removing the entry must still be noticed,
    /// when there is no longer anything to say what kind it was.
    #[test]
    fn a_global_pattern_matching_an_entrys_own_name_does_not_exclude_it() {
        let tmp = tempfile::tempdir().unwrap();
        let directory = tmp.path().join("cache");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("kept.toml"), "keep").unwrap();
        let file = tmp.path().join("notes.md");
        std::fs::write(&file, "keep").unwrap();

        let mut set = TrackedSet {
            exclude: vec!["cache".to_string(), "notes.md".to_string()],
            ..Default::default()
        };
        set.push(entry(&directory));
        set.push(entry(&file));
        let exclude = set.exclude_set().unwrap();

        // the directory entry: walked, so its root is not judged
        assert!(!set.excluded_by_lists(&exclude, &directory, Asked::Possibly));
        assert!(set.would_retain(&directory).unwrap());
        assert!(
            set.walk()
                .unwrap()
                .files
                .contains_key(&directory.join("kept.toml"))
        );
        // and a file entry is judged, exactly as the walk judges it
        assert!(set.excluded_by_lists(&exclude, &file, Asked::Possibly));
        assert!(!set.would_retain(&file).unwrap());
        assert!(!set.walk().unwrap().files.contains_key(&file));

        // removing a tracked entry is a change history has to notice,
        // and once it is gone there is nothing to ask what kind it was
        std::fs::remove_dir_all(&directory).unwrap();
        std::fs::remove_file(&file).unwrap();
        assert!(
            !set.excluded_by_lists(&exclude, &directory, Asked::Possibly),
            "the removal of a tracked directory was ignored"
        );
        assert!(
            !set.excluded_by_lists(&exclude, &file, Asked::Possibly),
            "the removal of a tracked file was ignored"
        );
    }

    /// A list selects paths inside a tracked directory. An entry that is
    /// itself a file has nothing inside it, so any list it declares
    /// leaves it out — one answer for the empty list and the non-empty
    /// one alike, and the same answer from the capture walk, the
    /// watcher's filter and a replay reading the recorded list.
    #[test]
    fn an_include_list_on_a_file_entry_selects_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("config.toml");
        std::fs::write(&file, "keep").unwrap();

        for include in [None, Some(vec![]), Some(vec!["config.toml".to_string()])] {
            let declared = include.is_some();
            let mut tracked = entry(&file);
            tracked.include = include.clone();
            let mut set = TrackedSet::default();
            set.push(tracked);

            let walk = set.walk().unwrap();
            assert_eq!(
                walk.files.contains_key(&file),
                !declared,
                "capture with include = {include:?}"
            );
            assert_eq!(
                set.would_retain(&file).unwrap(),
                !declared,
                "would_retain with include = {include:?}"
            );
            // the entry is a file, so the watcher asks the same question
            // the capture does and gets the same answer
            assert_eq!(
                !set.excluded_by_lists(&set.exclude_set().unwrap(), &file, Asked::Possibly),
                !declared,
                "watcher with include = {include:?}"
            );
            assert!(
                !included_by_entry(&file, include.as_deref().unwrap_or_default(), &file),
                "replay with include = {include:?}"
            );
            // and the drop is reported rather than silent
            assert_eq!(
                walk.omitted
                    .iter()
                    .any(|omitted| omitted.reason.contains("selects nothing")),
                declared,
                "reported with include = {include:?}"
            );
        }
    }

    /// **A directory is covered when something beneath it is, although
    /// no pattern ever selects a directory.** Asking a directory "are
    /// you selected" made every reader call a tracked tree uncaptured:
    /// `mise dot track` warned that a symlink's source was not in
    /// history while the capture was saving its files, and a rollback
    /// called the subdirectories a capture still walks uncovered.
    #[test]
    fn a_directory_is_covered_by_what_its_include_list_selects_below_it() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("codex");
        std::fs::create_dir_all(root.join("rules/deep")).unwrap();
        std::fs::create_dir_all(root.join("sessions")).unwrap();
        std::fs::write(root.join("rules/one.md"), "keep").unwrap();
        std::fs::write(root.join("sessions/a.jsonl"), "drop").unwrap();
        let mut tracked = entry(&root);
        tracked.include = Some(vec!["rules/**".to_string()]);
        let mut set = TrackedSet::default();
        set.push(tracked);

        // the tree the list selects into, and every directory on the way
        for directory in [root.clone(), root.join("rules"), root.join("rules/deep")] {
            assert!(
                set.would_capture(&directory).unwrap(),
                "would_capture {}",
                directory.display()
            );
            assert!(
                set.would_retain(&directory).unwrap(),
                "would_retain {}",
                directory.display()
            );
        }
        // and one no pattern can reach into is not
        assert!(!set.would_capture(&root.join("sessions")).unwrap());

        // files still answer as files: strict, from the patterns, with
        // no stat of their own
        assert!(set.would_retain(&root.join("rules/one.md")).unwrap());
        assert!(!set.would_retain(&root.join("sessions/a.jsonl")).unwrap());

        // an unreadable file the list does not select is still not
        // retained: unreadable is not unknown
        let unselected = root.join("sessions/a.jsonl");
        assert!(!set.would_retain(&unselected).unwrap());

        // a selected directory that has just vanished still wakes the
        // watcher, because a deleted tree is a change history must see
        let exclude = set.exclude_set().unwrap();
        std::fs::remove_dir_all(root.join("rules")).unwrap();
        assert!(!set.excluded_by_lists(&exclude, &root.join("rules"), Asked::Possibly));
    }

    /// The credential guard is lifted by a pattern that selected the
    /// file, never by the presence of a list. An entry that is itself a
    /// credential-named file has nothing inside it for a pattern to
    /// name, so no list it declares can lift its guard.
    #[test]
    fn a_list_on_a_credential_file_entry_does_not_lift_its_guard() {
        let tmp = tempfile::tempdir().unwrap();
        let key = tmp.path().join("id_rsa");
        std::fs::write(&key, "key").unwrap();
        for include in [
            None,
            Some(vec![]),
            Some(vec!["id_rsa".to_string()]),
            Some(vec!["**".to_string()]),
        ] {
            let mut tracked = entry(&key);
            tracked.include = include.clone();
            assert_eq!(
                tracked.capture_exclusion(&key),
                Some(CREDENTIAL_REASON),
                "include = {include:?} lifted the guard on the entry itself"
            );
        }
        // inside a directory entry a pattern can name it, and then the
        // selection decides
        let dir = tmp.path().join("ssh");
        std::fs::create_dir(&dir).unwrap();
        let inside = dir.join("id_rsa");
        std::fs::write(&inside, "key").unwrap();
        let mut tracked = entry(&dir);
        assert_eq!(tracked.capture_exclusion(&inside), Some(CREDENTIAL_REASON));
        tracked.include = Some(vec!["id_rsa".to_string()]);
        assert_eq!(tracked.capture_exclusion(&inside), None);
    }

    /// A repository inside a tracked directory is skipped whatever the
    /// include list says, and the explanation names the pattern that
    /// reached into it — found with the same glob semantics selection
    /// uses, not a literal prefix.
    #[test]
    fn a_nested_repository_is_skipped_and_the_pattern_that_reached_in_is_named() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("hammerspoon");
        let plugin = root.join("Spoons/Sky.spoon");
        std::fs::create_dir_all(plugin.join(".git")).unwrap();
        std::fs::write(root.join("init.lua"), "keep").unwrap();
        std::fs::write(plugin.join("init.lua"), "theirs").unwrap();

        for (patterns, named) in [
            (vec!["Spoons/*"], true),
            (vec!["**/init.lua"], true),
            (vec!["init.lua"], true),
            (vec!["Spoons/Sky.spoon/**"], true),
        ] {
            let mut tracked = entry(&root);
            tracked.include = Some(patterns.iter().map(|p| (*p).to_string()).collect());
            let mut set = TrackedSet::default();
            set.push(tracked);
            let walk = set.walk().unwrap();
            assert!(
                !walk.files.contains_key(&plugin.join("init.lua")),
                "{patterns:?} captured a file inside a nested repository"
            );
            let reported = walk
                .nested
                .iter()
                .find(|nested| nested.path.ends_with("Sky.spoon"))
                .unwrap_or_else(|| panic!("{patterns:?}: the repository was not reported"));
            assert_eq!(
                reported.reason.contains("selects nothing inside it"),
                named,
                "{patterns:?}: {}",
                reported.reason
            );
        }
    }

    /// The promise of the feature, end to end: one named file beside a
    /// directory of noise is captured, the noise is not walked, and the
    /// scan limits — which count what is captured — are nowhere near.
    #[test]
    fn a_narrow_include_list_does_not_walk_what_it_leaves_out() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("codex");
        std::fs::create_dir_all(root.join("rules")).unwrap();
        std::fs::create_dir_all(root.join("sessions/deep")).unwrap();
        std::fs::write(root.join("rules/one.md"), "keep").unwrap();
        for i in 0..40 {
            std::fs::write(root.join(format!("sessions/{i}.jsonl")), "noise").unwrap();
            std::fs::write(root.join(format!("sessions/deep/{i}.jsonl")), "noise").unwrap();
        }

        let mut tracked = entry(&root);
        tracked.include = Some(vec!["rules/**".to_string()]);
        let mut set = TrackedSet::default();
        set.push(tracked);
        let index = set.entry_index_for(&root).unwrap();
        let walk = set.walk().unwrap();

        assert_eq!(
            walk.files.keys().collect::<Vec<_>>(),
            vec![&root.join("rules/one.md")]
        );
        // the 80 files under `sessions` were never examined, let alone
        // counted: `considered` only counts what the walk reached
        assert!(
            walk.considered.get(&index).copied().unwrap_or(0) <= 2,
            "the walk descended into what the list leaves out: {:?}",
            walk.considered
        );
        assert!(walk.incomplete.is_empty(), "{:?}", walk.incomplete);
    }

    /// The watcher must wake on the tracked directory itself when the
    /// list selects something inside it, while a capture still declines
    /// to store the directory as a file — and retention, which asks the
    /// patterns alone, answers about the files.
    #[test]
    fn a_directory_whose_children_are_selected_is_watched() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("codex");
        std::fs::create_dir_all(root.join("rules")).unwrap();
        std::fs::write(root.join("rules/one.md"), "keep").unwrap();
        std::fs::write(root.join("notes.md"), "noise").unwrap();

        let mut tracked = entry(&root);
        tracked.include = Some(vec!["rules/**".to_string()]);
        let mut set = TrackedSet::default();
        set.push(tracked);
        let exclude = set.exclude_set().unwrap();
        let exclude_anchored = set.exclude_set().unwrap();

        // the entry directory and the directory the list reaches into:
        // an event on either has to be looked at
        for directory in [root.clone(), root.join("rules")] {
            assert!(
                !set.excluded_by_lists(&exclude, &directory, Asked::Possibly),
                "the watcher ignored {}",
                directory.display()
            );
        }
        // a directory nothing could select is still dropped
        std::fs::create_dir(root.join("sessions")).unwrap();
        assert!(set.excluded_by_lists(&exclude, &root.join("sessions"), Asked::Possibly));

        // **and the files inside one the patterns can reach are still
        // judged one by one.** A list of names reaches into every
        // directory, which must not turn every file under the entry into
        // something the watcher wakes for — that is the noise the list
        // was written to stop.
        let mut by_name = entry(&root);
        by_name.include = Some(vec!["config.toml".to_string()]);
        let mut named = TrackedSet::default();
        named.push(by_name);
        let exclude = named.exclude_set().unwrap();
        std::fs::create_dir_all(root.join("sessions")).unwrap();
        std::fs::write(root.join("sessions/one.jsonl"), "noise").unwrap();
        std::fs::write(root.join("config.toml"), "keep").unwrap();
        assert!(
            named.excluded_by_lists(&exclude, &root.join("sessions/one.jsonl"), Asked::Possibly),
            "the watcher woke for a transcript the list does not select"
        );
        assert!(!named.excluded_by_lists(&exclude, &root.join("config.toml"), Asked::Possibly));
        // a deletion leaves nothing to stat, and a selected file that
        // was deleted is still a change worth capturing
        std::fs::remove_file(root.join("config.toml")).unwrap();
        assert!(!named.excluded_by_lists(&exclude, &root.join("config.toml"), Asked::Possibly));
        // the directory a name pattern could match in is still watched
        assert!(!named.excluded_by_lists(&exclude, &root.join("sessions"), Asked::Possibly));

        // **a directory that is gone counts as what it could have
        // been.** An anchored list never selects the directory itself,
        // so reading a vanished path as a file would let a removed tree
        // pass unnoticed and leave history claiming files that no longer
        // exist.
        std::fs::remove_dir_all(root.join("rules")).unwrap();
        assert!(
            !set.excluded_by_lists(&exclude_anchored, &root.join("rules"), Asked::Possibly),
            "a removed directory of selected files was ignored"
        );
        std::fs::remove_dir_all(root.join("sessions")).unwrap();
        assert!(
            set.excluded_by_lists(&exclude_anchored, &root.join("sessions"), Asked::Possibly),
            "a removed directory nothing could select woke the watcher"
        );

        // and the files are decided exactly, by both
        assert!(set.would_retain(&root.join("rules/one.md")).unwrap());
        assert!(!set.would_retain(&root.join("notes.md")).unwrap());
        assert!(!set.excluded_by_lists(&exclude, &root.join("rules/one.md"), Asked::Possibly));
        assert!(set.excluded_by_lists(&exclude, &root.join("notes.md"), Asked::Possibly));
    }

    /// What a pattern selects decides what may be skipped, so the two
    /// have to say the same thing about a directory.
    #[test]
    fn a_pattern_that_selects_below_a_directory_reaches_into_it() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("codex");
        std::fs::create_dir_all(root.join("rules/deep")).unwrap();
        std::fs::write(root.join("rules/one.md"), "keep").unwrap();
        std::fs::write(root.join("rules/deep/two.md"), "keep").unwrap();

        // every one of these matches the directory `rules/deep` or an
        // ancestor of it, and **a pattern matching a directory takes
        // everything under it** — so all three select the file, and none
        // of them may prune the directory
        for (pattern, selects_deep) in [
            ("rules", true),
            ("rules/**", true),
            ("rules/*", true),
            ("sessions/**", false),
        ] {
            let mut tracked = entry(&root);
            tracked.include = Some(vec![pattern.to_string()]);
            let deep = root.join("rules/deep/two.md");
            assert_eq!(
                tracked.is_included(&deep),
                selects_deep,
                "{pattern} selecting {}",
                deep.display()
            );
            // and the walk must not skip a directory whose contents the
            // same pattern selects
            assert_eq!(
                tracked.include_prunes(&root.join("rules/deep")),
                !selects_deep,
                "{pattern} pruning rules/deep"
            );
        }
    }

    /// **Retention asks the patterns, not the filesystem.** A file the
    /// include list no longer selects is not carried forward because it
    /// happened to be unreadable when the save ran; one the list still
    /// selects is.
    #[test]
    fn retention_keeps_what_the_list_still_selects() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("codex");
        std::fs::create_dir_all(root.join("rules")).unwrap();

        let mut tracked = entry(&root);
        tracked.include = Some(vec!["rules/**".to_string()]);
        let mut set = TrackedSet::default();
        set.push(tracked);

        // neither file exists — this is the save that cannot read them
        let selected = root.join("rules/one.md");
        let dropped = root.join("sessions/one.jsonl");
        assert!(
            set.would_retain(&selected).unwrap(),
            "a selected file was not carried forward"
        );
        assert!(
            !set.would_retain(&dropped).unwrap(),
            "a file the list no longer selects was kept because it could not be read"
        );
        // and the watcher, which asks a different question, wakes for
        // both because either might have just been deleted
        let exclude = set.exclude_set().unwrap();
        assert!(!set.excluded_by_lists(&exclude, &selected, Asked::Possibly));
    }

    /// **Unselected is not absent.** A file that appears at a tracked
    /// entry's own path is not something a checkpoint with an include
    /// list ever held, so a rollback must not remove it.
    #[test]
    fn a_file_at_the_entry_path_is_not_read_as_absent() {
        use crate::system::history::replay::{PathState, classify_coverage};
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("codex");
        std::fs::create_dir_all(root.join("rules")).unwrap();
        std::fs::write(root.join("rules/one.md"), "keep").unwrap();

        let mut tracked = entry(&root);
        tracked.include = Some(vec!["rules/**".to_string()]);
        let mut set = TrackedSet::default();
        set.push(tracked);
        let coverage = set.coverage(&set.walk().unwrap());

        for (path, expected_absent) in [
            // the entry's own path: no pattern selects it, so a file
            // that appears there was never covered
            (root.clone(), false),
            // unselected, so never covered either
            (root.join("notes.md"), false),
            // selected and not in the tree: this one the checkpoint
            // positively says was absent
            (root.join("rules/gone.md"), true),
        ] {
            let state = classify_coverage(&coverage, &display_path(&path));
            assert_eq!(
                matches!(state, PathState::Absent),
                expected_absent,
                "{}",
                path.display()
            );
        }
    }

    /// **Pruning must never change what is selected.** On unix a
    /// backslash is a character in a name, so a pattern carrying one
    /// selects a directory whose name carries one — and the walk has to
    /// go in there rather than reading the backslash as a separator and
    /// skipping it.
    #[cfg(unix)]
    #[test]
    fn a_pattern_with_a_backslash_selects_what_it_names_on_unix() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("codex");
        let odd = root.join("we\\ird");
        std::fs::create_dir_all(&odd).unwrap();
        std::fs::write(odd.join("kept.md"), "keep").unwrap();

        let mut tracked = entry(&root);
        tracked.include = Some(vec!["we\\ird/**".to_string()]);
        let mut set = TrackedSet::default();
        set.push(tracked);

        // the matcher selects it, so the walk must reach it
        assert!(set.entries[0].is_included(&odd.join("kept.md")));
        assert!(
            !set.entries[0].include_prunes(&odd),
            "a directory the list selects was skipped unopened"
        );
        assert!(set.walk().unwrap().files.contains_key(&odd.join("kept.md")));
    }

    /// A path whose kind cannot be read is where the strict and the
    /// permissive questions disagree, and synchronization asks the
    /// strict one: a file another machine stopped selecting is not a
    /// deletion to replay here, however unreadable the local copy is.
    #[test]
    fn a_path_with_no_kind_answers_the_question_it_was_asked() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("sample");
        std::fs::create_dir_all(root.join("nested")).unwrap();
        let mut tracked = entry(&root);
        // a name-only list: it can match at any depth, so no directory
        // can be pruned and the permissive answer is always "might
        // matter"
        tracked.include = Some(vec!["keep".into()]);
        let mut set = TrackedSet::default();
        set.push(tracked);
        let exclude = set.exclude_set().unwrap();

        // nothing is there to stat, so the reader decides
        let narrowed = root.join("nested/leave");
        assert!(
            set.excluded_by_lists(&exclude, &narrowed, Asked::Exactly),
            "a path the include list does not select counted as managed, so a narrowing elsewhere would delete it here"
        );
        assert!(
            !set.excluded_by_lists(&exclude, &narrowed, Asked::Possibly),
            "the watcher stopped waking for a path that has just vanished"
        );

        // what the list does name is selected under either question
        let selected = root.join("nested/keep");
        assert!(!set.excluded_by_lists(&exclude, &selected, Asked::Exactly));
        assert!(!set.excluded_by_lists(&exclude, &selected, Asked::Possibly));
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
        // the index and the entry are one answer, so a covered path is
        // never read as uncovered because the entry could not be located
        // a second time
        for path in [child.join("file"), root.join("other")] {
            let index = set
                .entry_index_for(&path)
                .expect("a covered path has an index");
            assert_eq!(
                Some(&set.entries[index].path),
                set.entry_for(&path).map(|entry| &entry.path)
            );
        }
        assert!(set.entry_index_for(&tmp.path().join("elsewhere")).is_none());
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
                include: None,
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
        let restore = || {
            std::fs::set_permissions(
                root.join("sessions"),
                std::fs::Permissions::from_mode(0o700),
            )
            .unwrap()
        };
        if std::fs::read_dir(root.join("sessions")).is_ok() {
            // running as root, where the permission says nothing
            restore();
            return;
        }
        // every spelling of "exclude this directory" prunes it: a bare
        // name, the gitignore form the docs show, and an absolute path
        for pattern in [
            "sessions".to_string(),
            "sessions/**".to_string(),
            format!("{}/sessions/**", root.display()),
        ] {
            let mut set = TrackedSet {
                exclude: vec![pattern.clone()],
                ..Default::default()
            };
            set.push(entry(&root));
            let walk = set.walk().unwrap();
            assert_eq!(walk.files.len(), 1, "{pattern}");
            assert!(
                walk.files.contains_key(&root.join("config.toml")),
                "{pattern}"
            );
            assert!(walk.omitted.is_empty(), "{pattern}: {:?}", walk.omitted);
        }
        // but a negation that could re-include something inside makes the
        // walk descend after all, and the unreadable directory shows it
        let mut set = TrackedSet {
            exclude: vec![
                "sessions/**".to_string(),
                "!sessions/keep.jsonl".to_string(),
            ],
            ..Default::default()
        };
        set.push(entry(&root));
        let walk = set.walk().unwrap();
        assert!(
            walk.omitted
                .iter()
                .any(|omitted| omitted.reason.starts_with("unreadable")),
            "{:?}",
            walk.omitted
        );
        restore();
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
        // the same rule under a different tracked root: a relative
        // pattern matches at any depth *inside the entry*, which is the
        // only place it is ever asked about
        assert!(log.is_match(&cwd.join("a/b/c.log"), &cwd));
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
                    sessions.is_match(&root.join("sessions/one.jsonl"), root),
                    "{pattern} under {}",
                    root.display()
                );
            }
            // and a path outside the tracked entry is not this entry's
            // business, whatever the pattern would say about its name
            assert!(!sessions.is_match(
                Path::new("/somewhere/else/sessions/one.jsonl"),
                Path::new("/nonexistent-mise-test/.codex")
            ));
            assert!(!sessions.is_match(
                Path::new("/nonexistent-mise-test/.codex/config.toml"),
                Path::new("/nonexistent-mise-test/.codex")
            ));
        }
        // `~` is expanded, so a pattern written with it is absolute and
        // anchored, not a relative one matching at any depth
        let home_pattern = ExcludeSet::new(&["~/.mise-test-tilde/**".to_string()]).unwrap();
        let home = crate::dirs::HOME.to_path_buf();
        assert!(home_pattern.is_match(&home.join(".mise-test-tilde/x"), &home));
        assert!(!home_pattern.is_match(
            Path::new("/nonexistent-mise-test/a/.mise-test-tilde/x"),
            Path::new("/nonexistent-mise-test")
        ));

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

    /// Pruning a matching directory must not swallow a later `!pattern`
    /// that re-includes something inside it: last-match-wins is the
    /// semantics, and the pruning is only an optimization.
    #[test]
    fn a_negation_below_an_excluded_directory_still_re_includes() {
        // the matcher alone, with no walk: if this part ever fails the
        // cause is the matching, and if only the walk below fails the
        // cause is pruning skipping the directory before the negation is
        // consulted
        let root = Path::new(ROOT);
        let rules = ["cache".to_string(), "!cache/keep.conf".to_string()];
        let set = ExcludeSet::new(&rules).unwrap();
        for (file, excluded) in [("cache/index", true), ("cache/keep.conf", false)] {
            let path = root.join(file);
            assert_eq!(
                set.is_match(&path, root),
                excluded,
                "matcher: {file} under {} should be excluded={excluded}",
                root.display()
            );
        }
        assert!(
            set.may_reinclude_below(&root.join("cache")),
            "a negation naming something inside the directory must keep it walked"
        );

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("codex");
        let cache = root.join("cache");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(root.join("config.toml"), "keep").unwrap();
        std::fs::write(cache.join("index"), "drop").unwrap();
        std::fs::write(cache.join("keep.conf"), "keep").unwrap();
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
        assert!(
            !exclude.may_reinclude_below(&cache),
            "no negation at all, so {} must be prunable",
            cache.display()
        );
        // a negation somewhere else entirely does not disable pruning.
        // The path is built from the temp directory rather than written
        // as `/somewhere/else`, which is not absolute on Windows and
        // would be read as a relative pattern matching at any depth.
        let elsewhere = tmp.path().join("elsewhere/keep.conf");
        let exclude =
            ExcludeSet::new(&["cache".to_string(), format!("!{}", elsewhere.display())]).unwrap();
        assert!(
            !exclude.may_reinclude_below(&cache),
            "a negation at {} cannot re-include anything below {}",
            elsewhere.display(),
            cache.display()
        );
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
        // a working tree of its own, under a directory the global list
        // excludes and a later `!` rule reaches back into. The `!` rule
        // keeps `cache` from being pruned, so the walk descends — and
        // nothing in a selection list may carry it into another
        // repository.
        let repository = inner.join("cache/repo");
        std::fs::create_dir_all(repository.join(".git")).unwrap();
        std::fs::write(repository.join("secret"), "not ours").unwrap();

        let mut set = TrackedSet {
            exclude: vec![
                "b".to_string(),
                "cache".to_string(),
                format!("!{}/cache/keep.conf", inner.display()),
                format!("!{}/cache/repo/secret", inner.display()),
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
                !set.excluded_by_lists(&set.exclude_set().unwrap(), &file, Asked::Possibly),
                expected,
                "watcher {display}"
            );
            let covered = !matches!(classify_coverage(&coverage, &display), PathState::Uncovered);
            assert_eq!(covered, expected, "replay {display}");
        }

        // The repository is reported as one, not silently skipped, and
        // every reader refuses what is inside it — the walk because it
        // never descended, `would_retain` and the watcher because no
        // list may reach into another working tree, and a replay with
        // the reason rather than by calling it uncovered.
        assert_eq!(
            walk.nested
                .iter()
                .map(|nested| nested.path.as_str())
                .collect::<Vec<_>>(),
            vec![display_path(&repository).as_str()],
        );
        let describe = |state: &PathState| match state {
            PathState::Absent => "absent".to_string(),
            PathState::Uncovered => "uncovered".to_string(),
            PathState::Omitted(reason) => format!("omitted: {reason}"),
            PathState::Unevaluable(reason) => format!("unevaluable: {reason}"),
        };
        let secret = repository.join("secret");
        assert!(!walk.files.contains_key(&secret), "capture");
        assert!(!set.would_retain(&secret).unwrap(), "would_retain");
        assert!(
            set.excluded_by_lists(&set.exclude_set().unwrap(), &secret, Asked::Possibly),
            "watcher"
        );
        assert!(
            matches!(
                classify_coverage(&coverage, &display_path(&secret)),
                PathState::Omitted(reason) if reason.contains("repository")
            ),
            "replay: {}",
            describe(&classify_coverage(&coverage, &display_path(&secret)))
        );

        // `Absent` only when the record positively says so. Each other
        // input gets the answer that fits it — a repository the record
        // says was skipped reads as `Omitted` carrying the record's own
        // explanation, a record this mise cannot interpret reads as
        // `Unevaluable` — and none of them deletes.
        let display = display_path(outer.join("outer.toml"));
        for (name, broken, omitted_as) in [
            (
                "written before this matcher",
                {
                    let mut c = coverage.clone();
                    c.matcher = None;
                    c
                },
                None,
            ),
            (
                "written by a newer matcher",
                {
                    let mut c = coverage.clone();
                    c.matcher = Some(super::MATCHER_VERSION + 1);
                    c
                },
                None,
            ),
            (
                "a repository recorded as skipped",
                {
                    let mut c = coverage.clone();
                    c.nested.push(crate::system::history::store::PathReason {
                        path: display_path(&outer),
                        reason: NESTED_REPOSITORY_REASON.into(),
                    });
                    c
                },
                Some(NESTED_REPOSITORY_REASON),
            ),
            // On Windows the record may spell the path with either
            // separator and they mean the same directory. On unix a
            // backslash is part of a name, so a path spelled that way is
            // a different path — and one the record says nothing about,
            // which is why this case is Windows-only rather than
            // expecting the same answer everywhere.
            #[cfg(windows)]
            (
                "a repository recorded with the host's separators",
                {
                    let mut c = coverage.clone();
                    c.nested.push(crate::system::history::store::PathReason {
                        path: display_path(&outer).replace('/', "\\"),
                        reason: NESTED_REPOSITORY_REASON.into(),
                    });
                    c
                },
                Some(NESTED_REPOSITORY_REASON),
            ),
        ] {
            let state = classify_coverage(&broken, &display);
            let got = describe(&state);
            match omitted_as {
                // the record explains itself, so a rollback tells the
                // user what was skipped rather than that the checkpoint
                // cannot be interpreted
                Some(reason) => assert!(
                    matches!(&state, PathState::Omitted(found) if found == reason),
                    "{name}: expected the record's own explanation, got {got}"
                ),
                None => assert!(
                    matches!(&state, PathState::Unevaluable(_)),
                    "{name}: expected an uninterpretable record, got {got}"
                ),
            }
            // asserted, not assumed: `skip_reason` is the single place a
            // rollback decides to delete, and every state here refuses
            assert!(
                state.skip_reason("0123456789").is_some(),
                "{name}: {got} must never delete a live file"
            );
        }

        // the third non-deleting answer, from a path no entry covers
        let outside = display_path(tmp.path().join("outside.toml"));
        let state = classify_coverage(&coverage, &outside);
        assert!(
            matches!(state, PathState::Uncovered),
            "a path under no entry is uncovered, got {}",
            describe(&state)
        );
        assert!(
            state.skip_reason("0123456789").is_some(),
            "an uncovered path must never delete a live file"
        );
        // a rule neither side can use is dropped by both, so the record
        // stays readable — that agreement is what makes dropping safe
        let mut unusable = coverage.clone();
        unusable
            .exclude
            .push("$MISE_TEST_UNSUPPORTED/**".to_string());
        assert_eq!(
            matches!(classify_coverage(&unusable, &display), PathState::Absent),
            matches!(classify_coverage(&coverage, &display), PathState::Absent),
            "an unusable rule changes nothing, because capture ignored it too"
        );

        // a record this mise can read answers normally
        let mut plain = coverage.clone();
        plain.exclude.clear();
        assert!(!matches!(
            classify_coverage(&plain, &display),
            PathState::Unevaluable(_)
        ));
    }

    /// The four rules of a tracked entry's `include` list, each pinned
    /// on the same tree.
    #[test]
    fn an_include_list_selects_what_a_tracked_directory_saves() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("codex");
        std::fs::create_dir_all(root.join("rules/deep")).unwrap();
        std::fs::create_dir_all(root.join("sessions")).unwrap();
        std::fs::write(root.join("config.toml"), "keep").unwrap();
        std::fs::write(root.join("notes.md"), "noise").unwrap();
        std::fs::write(root.join("rules/one.md"), "keep").unwrap();
        std::fs::write(root.join("rules/deep/two.md"), "keep").unwrap();
        std::fs::write(root.join("sessions/one.jsonl"), "noise").unwrap();

        let captured = |include: Option<&[&str]>, exclude: &[&str]| -> Vec<String> {
            let mut entry = entry(&root);
            entry.include = include.map(|p| p.iter().map(|p| (*p).to_string()).collect());
            entry.exclude = exclude.iter().map(|p| (*p).to_string()).collect();
            let mut set = TrackedSet::default();
            set.push(entry);
            let walk = set.walk().unwrap();
            let mut names: Vec<String> = walk
                .files
                .keys()
                .map(|path| {
                    path.strip_prefix(&root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/")
                })
                .collect();
            names.sort();
            // the walk and every other reader have to agree
            for path in walk.files.keys() {
                assert!(set.would_retain(path).unwrap(), "{}", path.display());
            }
            names
        };

        // rule 1: no list means the whole tree
        assert_eq!(
            captured(None, &[]),
            [
                "config.toml",
                "notes.md",
                "rules/deep/two.md",
                "rules/one.md",
                "sessions/one.jsonl"
            ]
        );
        // rule 2: with a list, only what it names — and a directory
        // pattern takes everything under it
        assert_eq!(
            captured(Some(&["config.toml", "rules/**"]), &[]),
            ["config.toml", "rules/deep/two.md", "rules/one.md"]
        );
        // a new sibling appears without being named, and stays out
        std::fs::write(root.join("telemetry.json"), "noise").unwrap();
        assert_eq!(
            captured(Some(&["config.toml", "rules/**"]), &[]),
            ["config.toml", "rules/deep/two.md", "rules/one.md"]
        );
        // a declared but empty list selects nothing, and is never read as
        // no list at all — that would capture the whole tree
        assert!(captured(Some(&[]), &[]).is_empty());
        // and a list that names nothing present is the same answer: the
        // list decides, so what it does not select is not captured
        assert!(captured(Some(&["nothing-here"]), &[]).is_empty());
        // rule 3: an explicit exclude wins over an include
        assert_eq!(
            captured(Some(&["config.toml", "rules/**"]), &["rules/deep"]),
            ["config.toml", "rules/one.md"]
        );
    }

    /// An `include` list is a selection, and selection decides what is
    /// captured: a literal and a glob carry the same authority, and
    /// either one selecting a credential-named file captures it — in
    /// plaintext, said out loud. Without a list, the builtin filtering
    /// applies as it always has, so an existing declaration is
    /// unaffected by this feature.
    #[test]
    fn an_include_list_decides_what_is_captured_and_says_when_it_is_plaintext() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("fish");
        std::fs::create_dir_all(root.join("functions")).unwrap();
        std::fs::write(root.join("functions/hello.fish"), "function hello; end").unwrap();
        std::fs::write(root.join("functions/secrets.fish"), "set -x TOKEN x").unwrap();

        let walk_with = |include: Option<&[&str]>, encrypt: bool| {
            let mut entry = entry(&root);
            entry.include = include.map(|p| p.iter().map(|p| (*p).to_string()).collect());
            entry.policy.encrypt = encrypt;
            let mut set = TrackedSet::default();
            set.push(entry);
            set.walk().unwrap()
        };
        let holds = |walk: &Walk, name: &str| walk.files.keys().any(|p| p.ends_with(name));

        // no list: the builtin filtering applies, exactly as before
        let walk = walk_with(None, false);
        assert!(holds(&walk, "hello.fish"));
        assert!(!holds(&walk, "secrets.fish"));
        assert!(walk.plaintext.is_empty());
        assert!(
            walk.omitted
                .iter()
                .any(|o| o.path.ends_with("secrets.fish"))
        );

        // a list selects, and a glob is as authoritative as a literal
        for include in [
            &["functions/secrets.fish"][..],
            &["functions/*.fish"][..],
            &["**"][..],
        ] {
            let walk = walk_with(Some(include), false);
            assert!(holds(&walk, "secrets.fish"), "{include:?}");
            assert!(walk.omitted.is_empty(), "{include:?}");
            assert_eq!(walk.plaintext.len(), 1, "{include:?}");
            assert!(
                walk.plaintext[0].path.ends_with("secrets.fish"),
                "{include:?}"
            );
            // and every reader agrees with the walk
            let mut set = TrackedSet::default();
            let mut owner = entry(&root);
            owner.include = Some(include.iter().map(|p| (*p).to_string()).collect());
            set.push(owner);
            assert!(
                set.would_retain(&root.join("functions/secrets.fish"))
                    .unwrap(),
                "{include:?}"
            );
        }

        // encryption is a separate question: the file is captured either
        // way, and nothing is stored in the clear
        let walk = walk_with(Some(&["functions/secrets.fish"]), true);
        assert!(holds(&walk, "secrets.fish"));
        assert!(walk.plaintext.is_empty());
    }

    /// A declared-but-empty list selects nothing, an entry that is
    /// itself a file included — and declaring a list on such an entry
    /// never lifts the credential guard for it, because no pattern named
    /// the file. Overriding the guard is something only a pattern does.
    #[test]
    fn an_empty_include_selects_nothing_even_for_a_single_file_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("app");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("credentials"), "token").unwrap();
        let file = dir.join("credentials");

        let walk_with = |include: Option<&[&str]>| {
            let mut e = entry(&file);
            e.include = include.map(|p| p.iter().map(|p| (*p).to_string()).collect());
            let mut set = TrackedSet::default();
            set.push(e);
            (set.walk().unwrap(), set)
        };
        // no list: the guard applies, as it always has
        let (walk, set) = walk_with(None);
        assert!(walk.files.is_empty());
        assert!(!set.would_retain(&file).unwrap());
        // an empty list selects nothing at all
        let (walk, set) = walk_with(Some(&[]));
        assert!(walk.files.is_empty());
        assert!(walk.plaintext.is_empty());
        assert!(!set.would_retain(&file).unwrap());
        // and a list on a file entry does not name the file, so the
        // guard still stands: this is not the direct-entry override
        let (walk, set) = walk_with(Some(&["credentials"]));
        assert!(walk.files.is_empty(), "{:?}", walk.files);
        assert!(walk.plaintext.is_empty());
        assert!(!set.would_retain(&file).unwrap());
        // the supported spelling is a pattern on the directory entry
        let mut owner = entry(&dir);
        owner.include = Some(vec!["credentials".to_string()]);
        let mut set = TrackedSet::default();
        set.push(owner);
        let walk = set.walk().unwrap();
        assert!(walk.files.contains_key(&file));
        assert_eq!(walk.plaintext.len(), 1);
        assert!(set.would_retain(&file).unwrap());
    }

    /// A repository inside a tracked directory is skipped whatever the
    /// entry's `include` list says, and a pattern reaching into it is
    /// told that it selects nothing — there is a supported way to
    /// capture those files, and it is not this.
    #[test]
    fn an_include_list_cannot_reach_into_a_nested_repository() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("hammerspoon");
        let plugin = root.join("Spoons/Sky.spoon");
        std::fs::create_dir_all(plugin.join(".git")).unwrap();
        std::fs::write(plugin.join("init.lua"), "return {}").unwrap();
        std::fs::write(root.join("init.lua"), "top").unwrap();

        let walk_with = |include: &[&str]| {
            let mut entry = entry(&root);
            entry.include = Some(include.iter().map(|p| (*p).to_string()).collect());
            let mut set = TrackedSet::default();
            set.push(entry);
            set.walk().unwrap()
        };
        // a pattern that names paths inside it selects nothing, and the
        // skip says which pattern that was
        let walk = walk_with(&["Spoons/Sky.spoon/**"]);
        assert!(walk.files.is_empty(), "{:?}", walk.files);
        assert_eq!(walk.nested.len(), 1);
        assert!(walk.nested[0].reason.contains("selects nothing inside it"));
        assert!(walk.nested[0].reason.contains("Spoons/Sky.spoon/**"));
        // a bare name matches a component at any depth, so `init.lua`
        // does reach into the repository, and is named as such while the
        // entry's own `init.lua` is still captured
        let walk = walk_with(&["init.lua"]);
        assert!(walk.files.contains_key(&root.join("init.lua")));
        assert!(!walk.files.contains_key(&plugin.join("init.lua")));
        assert_eq!(walk.nested.len(), 1);
        assert!(walk.nested[0].reason.contains("\"init.lua\""));
        // a list that cannot select anything under `Spoons` does not
        // walk it at all, so there is no repository to report and
        // nothing to explain: what the list leaves out stays out, rather
        // than being visited and discarded
        let walk = walk_with(&["other/**"]);
        assert!(walk.nested.is_empty(), "{:?}", walk.nested);
        assert!(walk.files.is_empty(), "{:?}", walk.files);
        // an entry with no list reports it as it always did
        let mut plain = entry(&root);
        plain.include = None;
        let mut set = TrackedSet::default();
        set.push(plain);
        let walk = set.walk().unwrap();
        assert_eq!(walk.nested.len(), 1);
        assert_eq!(walk.nested[0].reason, NESTED_REPOSITORY_REASON);
    }

    #[test]
    fn display_under_accepts_either_separator() {
        assert!(display_under("~/.ssh", "~/.ssh"));
        assert!(display_under("~/.ssh/id_test", "~/.ssh"));
        assert!(!display_under("~/.sshd/x", "~/.ssh"));
        assert!(!display_under("~/.ssh", "~/.ssh/id_test"));

        // **On Windows both spellings mix, and both separate.** A path
        // recorded by `display_path` carries `\` while one rebuilt from a
        // tree path carries `/`, and they name the same file.
        #[cfg(windows)]
        {
            assert!(display_under("~\\.ssh\\id_test", "~\\.ssh"));
            assert!(display_under("~\\.ssh\\id_test", "~/.ssh"));
            assert!(display_under("~/.ssh/id_test", "~\\.ssh"));
            assert!(display_under("~\\.ssh", "~/.ssh"));
            assert!(!display_under("~\\.sshd\\x", "~/.ssh"));
        }

        // **On unix a backslash is part of a name.** `~/.ssh\id_test` is
        // one file called `.ssh\id_test`, not a file inside `~/.ssh` —
        // reading it as a separator would let one entry appear to own
        // another's paths, and a replay would judge a live file by the
        // wrong root and the wrong exclusions.
        #[cfg(unix)]
        {
            assert!(!display_under("~/.ssh\\id_test", "~/.ssh"));
            assert!(display_under("~/.ssh\\id_test", "~/.ssh\\id_test"));
            assert!(!display_under("~\\.ssh\\id_test", "~/.ssh"));
        }
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
            include: None,
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
