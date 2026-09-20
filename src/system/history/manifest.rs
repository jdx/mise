//! Portable enrollment metadata stored in the ordinary tracked-file tree.
//! This is configuration, not a second history or a machine recovery record.

use eyre::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::select::Variant;
use super::shadow::{HistoryRepo, Overlay};

pub(crate) const PATH: &str = ".mise-history/manifest.json";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Enrollment {
    /// A portable repository path, rooted at `home/` or `config/`.
    pub path: String,
    pub autosave: bool,
    pub encrypt: bool,
    pub variants: Vec<Variant>,
    /// The entry's own `exclude` globs, relative to its path. Written only
    /// when set, so a setup without them stays readable by older clients.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
    /// The entry's own `include` globs, relative to its path. Written
    /// only when the entry declares a list, so a setup without one stays
    /// readable by older clients — and a declared but empty list, which
    /// selects nothing, is not mistaken for no list at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
    pub format: u32,
    pub enrollment: Vec<Enrollment>,
    pub exclude: Vec<String>,
    pub recipients: Vec<String>,
    /// Non-default Unix permission bits, keyed by portable variant path.
    pub permissions: BTreeMap<String, u32>,
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            format: 1,
            enrollment: vec![],
            exclude: vec![],
            recipients: vec![],
            permissions: BTreeMap::new(),
        }
    }
}

impl Manifest {
    pub(crate) fn file_permissions(
        &self,
        path: &str,
        object: Option<&super::sync::reconcile::Object>,
    ) -> Option<u32> {
        if !cfg!(unix) {
            return None;
        }
        let (mode, _) = object?;
        let default = match mode.as_str() {
            "100644" => 0o644,
            "100755" => 0o755,
            _ => return None,
        };
        Some(self.permissions.get(path).copied().unwrap_or(default))
    }
    /// Whether a permission path belongs to an enrolled stream: the enrolled
    /// path or one below it, or (without a variant) a directory strictly
    /// between the root and an enrolled path of any stream. A containing
    /// directory is one filesystem object, so it is recorded once, whatever
    /// streams the paths inside it belong to. The root itself is never owned.
    pub(crate) fn owns_stream(&self, path: &str) -> bool {
        let (stem, relative) = path.split_once('/').unwrap_or((path, ""));
        let (_, variant) = stem
            .split_once('@')
            .map_or((stem, None), |(root, variant)| (root, Some(variant)));
        self.enrolls_stream(path)
            || (variant.is_none()
                && !relative.is_empty()
                && self
                    .enrollment
                    .iter()
                    .any(|entry| strictly_below(&entry.path, &plain(path))))
    }

    /// The enrolled paths this machine selects from this manifest, each with
    /// its selected variant: an enrollment with no matching variant here is
    /// left out. Like [`Self::tracking`], but a pure view for a manifest
    /// that is not being enrolled (a saved one, read to decide baselines).
    pub(crate) fn selected_entries(&self) -> Vec<super::tracked::TrackedEntry> {
        let roots = super::sync::layout::Roots::current();
        let environments = super::select::active_environments();
        self.enrollment
            .iter()
            .filter_map(|enrollment| {
                let variant = match super::select::select(&enrollment.variants, &environments) {
                    super::select::Selection::Single => None,
                    super::select::Selection::Variant(variant) => Some(variant.name()),
                    super::select::Selection::NoMatch | super::select::Selection::Ambiguous(_) => {
                        return None;
                    }
                };
                let local = roots.locate(&enrollment.path).path()?.to_path_buf();
                let mut policy = crate::system::files::FilePolicy::for_mode(
                    crate::system::files::FileMode::Track,
                );
                policy.autosave = enrollment.autosave;
                policy.encrypt = enrollment.encrypt;
                let mut entry = super::tracked::TrackedEntry::new(local, "track", policy);
                entry.variant = variant;
                Some(entry)
            })
            .collect()
    }

    /// Whether a permission path is the enrolled path of its stream or below
    /// one: some enrollment at or above it exposes that stream. A nested
    /// enrollment for other platforms does not hide the enclosing one, whose
    /// stream the path belongs to on machines where the nested one is not
    /// selected; this is decided without knowing which machine reads it.
    fn enrolls_stream(&self, path: &str) -> bool {
        let (stem, _) = path.split_once('/').unwrap_or((path, ""));
        let (_, variant) = stem
            .split_once('@')
            .map_or((stem, None), |(root, variant)| (root, Some(variant)));
        let portable = plain(path);
        self.enrollment
            .iter()
            .filter(|entry| portable == entry.path || strictly_below(&portable, &entry.path))
            .any(|entry| match variant {
                Some(name) => entry.variants.iter().any(|variant| variant.name() == name),
                None => entry.variants.is_empty(),
            })
    }

    pub(crate) fn remove_unenrolled_permissions(&mut self) {
        self.permissions = self
            .permissions
            .iter()
            .filter(|(path, _)| self.owns_stream(path))
            .map(|(path, bits)| (path.clone(), *bits))
            .collect();
    }

    /// Replace only active streams. Inactive platform permissions remain in
    /// the same tree, just like their file contents.
    pub(crate) fn capture_permissions(
        &mut self,
        entries: &[super::tracked::TrackedEntry],
        modes: &BTreeMap<String, u32>,
    ) -> Result<()> {
        if !cfg!(unix) {
            self.remove_unenrolled_permissions();
            return Ok(());
        }
        let roots = super::sync::layout::Roots::current();
        let active: Vec<_> = entries
            .iter()
            .map(|entry| entry.tree_path(&entry.path))
            .collect::<Result<_>>()?;
        // an active entry replaces its own path, everything below it, and
        // (in the variant-less stream) the directories between it and the root
        self.permissions.retain(|path, _| {
            !active.iter().any(|prefix| {
                path == prefix
                    || strictly_below(path, prefix)
                    || (plain(path) == *path && strictly_below(&plain(prefix), path))
            })
        });
        for (display, bits) in modes {
            let path = crate::file::replace_path(display);
            let owner = super::tracked::owning_entry(entries, &path);
            let contains_entries = entries
                .iter()
                .any(|entry| entry.path.starts_with(&path) && entry.path != path);
            let portable = match owner {
                Some(entry) => roots
                    .branch_path(&path, entry.variant.as_deref())
                    .ok_or_else(|| eyre::eyre!("cannot map permission path {display}"))?,
                // a directory containing entries is one filesystem object:
                // recorded once, without a variant; one outside every root,
                // or the root itself, is never recorded
                None if contains_entries => match roots.branch_path(&path, None) {
                    Some(portable) if portable.contains('/') => portable,
                    _ => continue,
                },
                None => continue,
            };
            self.permissions.insert(portable, *bits);
        }
        self.remove_unenrolled_permissions();
        Ok(())
    }

    /// Merge enrollment by portable path rather than JSON line positions.
    /// Independent additions and policy edits commute; deletion versus a
    /// policy edit remains a conflict, never an implicit reenrollment.
    pub(crate) fn merge(base: &Self, local: &Self, remote: &Self) -> Result<Self> {
        use std::collections::BTreeMap;
        fn choose<T: Clone + Eq>(base: &T, local: &T, remote: &T, field: &str) -> Result<T> {
            if local == remote || remote == base {
                Ok(local.clone())
            } else if local == base {
                Ok(remote.clone())
            } else {
                bail!(
                    "enrollment conflict in {field}; reconcile repository metadata before syncing"
                )
            }
        }
        base.validate()?;
        local.validate()?;
        remote.validate()?;
        if local == remote || remote == base {
            return Ok(local.clone());
        }
        if local == base {
            return Ok(remote.clone());
        }
        let indexed = |manifest: &Self| {
            manifest
                .enrollment
                .iter()
                .map(|entry| (entry.path.clone(), entry.clone()))
                .collect::<BTreeMap<_, _>>()
        };
        let (base_entries, local_entries, remote_entries) =
            (indexed(base), indexed(local), indexed(remote));
        let paths: std::collections::BTreeSet<_> = base_entries
            .keys()
            .chain(local_entries.keys())
            .chain(remote_entries.keys())
            .collect();
        let mut enrollment = vec![];
        for path in paths {
            let (before, ours, theirs) = (
                base_entries.get(path),
                local_entries.get(path),
                remote_entries.get(path),
            );
            let entry = match (before, ours, theirs) {
                (Some(before), Some(ours), Some(theirs)) => Some(Enrollment {
                    path: path.clone(),
                    autosave: choose(
                        &before.autosave,
                        &ours.autosave,
                        &theirs.autosave,
                        &format!("{path}: autosave"),
                    )?,
                    encrypt: choose(
                        &before.encrypt,
                        &ours.encrypt,
                        &theirs.encrypt,
                        &format!("{path}: encryption"),
                    )?,
                    variants: choose(
                        &before.variants,
                        &ours.variants,
                        &theirs.variants,
                        &format!("{path}: variants"),
                    )?,
                    exclude: choose(
                        &before.exclude,
                        &ours.exclude,
                        &theirs.exclude,
                        &format!("{path}: exclude"),
                    )?,
                    include: choose(
                        &before.include,
                        &ours.include,
                        &theirs.include,
                        &format!("{path}: include"),
                    )?,
                }),
                _ => choose(&before, &ours, &theirs, path)?.cloned(),
            };
            if let Some(entry) = entry {
                enrollment.push(entry);
            }
        }
        let permission_paths: std::collections::BTreeSet<_> = base
            .permissions
            .keys()
            .chain(local.permissions.keys())
            .chain(remote.permissions.keys())
            .collect();
        let mut permissions = BTreeMap::new();
        for path in permission_paths {
            if let Some(bits) = choose(
                &base.permissions.get(path),
                &local.permissions.get(path),
                &remote.permissions.get(path),
                &format!("{path}: permissions"),
            )? {
                permissions.insert(path.clone(), *bits);
            }
        }
        let merged = Self {
            format: 1,
            enrollment,
            exclude: choose(&base.exclude, &local.exclude, &remote.exclude, "exclusions")?,
            recipients: choose(
                &base.recipients,
                &local.recipients,
                &remote.recipients,
                "recipients",
            )?,
            permissions,
        };
        merged.validate()?;
        Ok(merged)
    }

    /// Restore enrollment from Git without needing a captured mise config.
    /// Deployment inputs are validated separately; none are inferred here.
    pub(crate) fn tracking(&self) -> Result<super::tracked::TrackedSet> {
        self.validate()?;
        let roots = super::sync::layout::Roots::current();
        let environments = super::select::active_environments();
        let mut tracked = super::tracked::TrackedSet {
            manifest: self.clone(),
            exclude: self.exclude.clone(),
            ..Default::default()
        };
        for enrollment in &self.enrollment {
            let variant = match super::select::select(&enrollment.variants, &environments) {
                super::select::Selection::Single => None,
                super::select::Selection::Variant(variant) => Some(variant.name()),
                super::select::Selection::NoMatch => continue,
                super::select::Selection::Ambiguous(_) => {
                    bail!("ambiguous variants for {}", enrollment.path)
                }
            };
            let local = roots
                .locate(&enrollment.path)
                .path()
                .ok_or_else(|| eyre::eyre!("invalid enrollment path {}", enrollment.path))?
                .to_path_buf();
            super::tracked::ensure_portable_ancestors(&local)?;
            let mut policy =
                crate::system::files::FilePolicy::for_mode(crate::system::files::FileMode::Track);
            policy.autosave = enrollment.autosave;
            policy.encrypt = enrollment.encrypt;
            let mut entry = super::tracked::TrackedEntry::new(local, "track", policy);
            entry.variant = variant;
            entry.exclude = enrollment.exclude.clone();
            entry.include = enrollment.include.clone();
            tracked.entries.push(entry);
        }
        Ok(tracked)
    }

    /// Every declared encrypted stream, including platforms inactive here.
    pub(crate) fn encrypted_paths(&self) -> std::collections::BTreeSet<String> {
        let mut paths = std::collections::BTreeSet::new();
        for entry in self.enrollment.iter().filter(|entry| entry.encrypt) {
            if entry.variants.is_empty() {
                paths.insert(entry.path.clone());
            } else {
                let (root, relative) = entry.path.split_once('/').unwrap_or((&entry.path, ""));
                for variant in &entry.variants {
                    let stem = format!("{root}@{}", variant.name());
                    paths.insert(if relative.is_empty() {
                        stem
                    } else {
                        format!("{stem}/{relative}")
                    });
                }
            }
        }
        paths
    }

    /// The enrollment that owns a portable path: the most specific one,
    /// by component count, as [`crate::system::history::tracked::owning_entry`]
    /// decides for live paths. Byte length is not the same rule.
    fn owner(&self, path: &str) -> Option<&Enrollment> {
        self.enrollment
            .iter()
            .filter(|entry| path == entry.path || strictly_below(path, &entry.path))
            .max_by_key(|entry| entry.path.split('/').count())
    }

    /// Carry inactive streams and repository-owned files from the same parent
    /// tree. Paths explicitly untracked since that parent are deliberately not
    /// carried; no live file is removed by this tree construction.
    pub(crate) fn preserve_other_files(&self, repo: &HistoryRepo, tree: &str) -> Result<String> {
        let Some(parent) = repo.ref_oid(HistoryRepo::HISTORY_REF)? else {
            return Ok(tree.into());
        };
        let previous = Self::read(repo, &parent)?.unwrap_or_default();
        let environments = super::select::active_environments();
        let mut overlays = vec![];
        for file in repo.ls_tree(&parent)? {
            if file.path.starts_with(".mise-history/") {
                continue;
            }
            let (stem, relative) = file.path.split_once('/').unwrap_or((&file.path, ""));
            let (root, variant) = stem
                .split_once('@')
                .map_or((stem, None), |(root, variant)| (root, Some(variant)));
            let portable = if relative.is_empty() {
                root.into()
            } else {
                format!("{root}/{relative}")
            };
            let carry = match self.owner(&portable) {
                None => previous.owner(&portable).is_none(),
                Some(entry) => {
                    let selected = match super::select::select(&entry.variants, &environments) {
                        super::select::Selection::Single => None,
                        super::select::Selection::Variant(variant) => Some(variant.name()),
                        super::select::Selection::NoMatch => Some(String::new()),
                        super::select::Selection::Ambiguous(_) => {
                            bail!("ambiguous variants for {}", entry.path)
                        }
                    };
                    variant != selected.as_deref()
                        && variant.is_some_and(|name| {
                            entry.variants.iter().any(|variant| variant.name() == name)
                        })
                }
            };
            if carry {
                overlays.push(Overlay {
                    path: file.path,
                    object: Some((file.mode, file.oid)),
                });
            }
        }
        repo.compose(tree, &overlays)
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.format != 1 {
            bail!("unsupported dotfile repository format {}", self.format);
        }
        let mut paths = std::collections::BTreeSet::new();
        for entry in &self.enrollment {
            if !super::sync::layout::is_safe_branch_path(&entry.path)
                || !(entry.path.starts_with("home/")
                    || entry.path == "config"
                    || entry.path.starts_with("config/"))
                || !paths.insert(&entry.path)
            {
                bail!("invalid or repeated enrollment path {}", entry.path);
            }
            let mut variants = std::collections::BTreeSet::new();
            super::select::validate(&entry.variants)?;
            let include: &[String] = entry.include.as_deref().unwrap_or_default();
            for (key, patterns) in [("exclude", entry.exclude.as_slice()), ("include", include)] {
                for pattern in patterns {
                    if let Err(err) = glob::Pattern::new(pattern) {
                        bail!(
                            "invalid {key} pattern {pattern:?} for {}: {err}",
                            entry.path
                        );
                    }
                }
            }
            for variant in &entry.variants {
                let name = variant.name();
                if name.contains('@')
                    || !super::sync::layout::is_safe_branch_path(&name)
                    || !variants.insert(name)
                {
                    bail!("invalid or repeated variant for {}", entry.path);
                }
            }
        }
        for (path, bits) in &self.permissions {
            if *bits > 0o777
                || !super::sync::layout::is_safe_branch_path(path)
                || !self.owns_stream(path)
            {
                bail!("invalid or unenrolled permission path {path}");
            }
        }
        Ok(())
    }

    pub(crate) fn read(repo: &HistoryRepo, tree: &str) -> Result<Option<Self>> {
        let Some((mode, oid)) = repo.object_at(tree, PATH)? else {
            return Ok(None);
        };
        if mode != "100644" {
            bail!("dotfile enrollment metadata must be a regular file");
        }
        let bytes = repo.cat_object_bounded(&oid, 4 * 1024 * 1024)?;
        Self::from_bytes(&bytes).map(Some)
    }

    pub(crate) fn read_gix(tree: &gix::Tree<'_>) -> Result<Option<Self>> {
        let Some(entry) = tree.lookup_entry_by_path(PATH)? else {
            return Ok(None);
        };
        if entry.mode().value() != 0o100644 {
            bail!("dotfile enrollment metadata must be a regular file");
        }
        if entry.id().header()?.size() > 4 * 1024 * 1024 {
            bail!("dotfile enrollment metadata is too large");
        }
        let object = entry.object()?;
        Self::from_bytes(&object.data).map(Some)
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        #[derive(Deserialize)]
        struct FormatHeader {
            format: u64,
        }
        let header: FormatHeader = serde_json::from_slice(bytes)?;
        if header.format != 1 {
            bail!("unsupported dotfile repository format {}", header.format);
        }
        let manifest: Self = serde_json::from_slice(bytes)?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub(crate) fn write(&self, repo: &HistoryRepo, tree: &str) -> Result<String> {
        self.validate()?;
        let oid = repo.hash_blob(&serde_json::to_vec_pretty(self)?)?;
        let marker = repo.hash_blob(super::sync::format::marker_content().as_bytes())?;
        repo.compose(
            tree,
            &[
                Overlay {
                    path: PATH.into(),
                    object: Some(("100644".into(), oid)),
                },
                Overlay {
                    path: super::sync::layout::MARKER_PATH.into(),
                    object: Some(("100644".into(), marker)),
                },
            ],
        )
    }
}

/// Whether a portable path is inside the directory `prefix` (not `prefix`
/// itself).
fn strictly_below(path: &str, prefix: &str) -> bool {
    path.strip_prefix(prefix)
        .is_some_and(|rest| rest.starts_with('/'))
}

/// A portable path without its variant: `home@linux/.zshrc` is `home/.zshrc`.
fn plain(path: &str) -> String {
    let (stem, relative) = path.split_once('/').unwrap_or((path, ""));
    let root = stem.split('@').next().unwrap_or(stem);
    if relative.is_empty() {
        root.to_string()
    } else {
        format!("{root}/{relative}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn future_format_is_reported_before_unknown_fields() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let repo = HistoryRepo::open_or_init_in(temp.path())?.unwrap();
        for (json, message) in [
            (
                r#"{"format":2,"future_field":true}"#,
                "unsupported dotfile repository format 2",
            ),
            (r#"{"format":1,"future_field":true}"#, "unknown field"),
            (r#"{"format":1,"format":1}"#, "duplicate field"),
            (
                r#"{"format":1,"enrollment":[],"enrollment":[]}"#,
                "duplicate field",
            ),
        ] {
            let tree = repo.compose(
                &repo.mktree("")?,
                &[Overlay {
                    path: PATH.into(),
                    object: Some(("100644".into(), repo.hash_blob(json.as_bytes())?)),
                }],
            )?;
            assert!(
                Manifest::read(&repo, &tree)
                    .unwrap_err()
                    .to_string()
                    .contains(message)
            );
        }
        Ok(())
    }

    #[test]
    fn gix_reader_rejects_oversized_manifest() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let repo = HistoryRepo::open_or_init_in(temp.path())?.unwrap();
        let tree = repo.compose(
            &repo.mktree("")?,
            &[Overlay {
                path: PATH.into(),
                object: Some((
                    "100644".into(),
                    repo.hash_blob(&vec![b' '; 4 * 1024 * 1024 + 1])?,
                )),
            }],
        )?;
        let gix = gix::open_opts(repo.dir(), gix::open::Options::isolated())?;
        let tree = gix.find_tree(gix::ObjectId::from_hex(tree.as_bytes())?)?;
        assert!(
            Manifest::read_gix(&tree)
                .unwrap_err()
                .to_string()
                .contains("too large")
        );
        Ok(())
    }

    #[test]
    fn permission_merge_is_per_path_and_rejects_divergence() {
        let base = Manifest {
            enrollment: vec![enrollment("home/configs")],
            permissions: BTreeMap::from([
                ("home/configs/a".into(), 0o600),
                ("home/configs/b".into(), 0o600),
            ]),
            ..Default::default()
        };
        let mut local = base.clone();
        let mut remote = base.clone();
        local.permissions.insert("home/configs/a".into(), 0o640);
        remote.permissions.insert("home/configs/b".into(), 0o400);
        let merged = Manifest::merge(&base, &local, &remote).unwrap();
        assert_eq!(merged.permissions["home/configs/a"], 0o640);
        assert_eq!(merged.permissions["home/configs/b"], 0o400);
        remote.permissions.insert("home/configs/a".into(), 0o660);
        assert!(Manifest::merge(&base, &local, &remote).is_err());
    }

    #[test]
    fn permissions_require_enrolled_paths_and_valid_bits() {
        let mut manifest = Manifest {
            enrollment: vec![enrollment("home/configs")],
            permissions: BTreeMap::from([("home/untracked".into(), 0o600)]),
            ..Default::default()
        };
        assert!(manifest.validate().is_err());
        manifest.remove_unenrolled_permissions();
        assert!(manifest.permissions.is_empty());
        manifest.permissions.insert("home/configs/a".into(), 0o4600);
        assert!(manifest.validate().is_err());
        // a directory between the root and an enrolled path is owned; the
        // root itself, a sibling, and another stream are not
        manifest.permissions.clear();
        manifest.enrollment = vec![enrollment("home/.claude/settings.json")];
        manifest.permissions.insert("home/.claude".into(), 0o700);
        manifest.validate().unwrap();
        for path in ["home", "home/.claudia", "home@linux/.claude"] {
            manifest.permissions.insert(path.into(), 0o700);
            assert!(manifest.validate().is_err(), "{path}");
            manifest.remove_unenrolled_permissions();
            assert_eq!(
                manifest.permissions,
                BTreeMap::from([("home/.claude".into(), 0o700)]),
                "{path}"
            );
        }
    }

    /// Tracking one file inside a private directory records that
    /// directory's mode, never home's.
    #[cfg(unix)]
    #[test]
    fn permission_capture_records_the_parents_of_a_tracked_file() {
        use super::super::tracked::TrackedEntry;
        use crate::system::files::{FileMode, FilePolicy};
        let mut manifest = Manifest {
            enrollment: vec![
                enrollment("home/.claude/settings.json"),
                enrollment("home/.claude/projects/notes"),
            ],
            ..Default::default()
        };
        let home = &*crate::dirs::HOME;
        let policy = FilePolicy::for_mode(FileMode::Track);
        let entries = vec![
            TrackedEntry::new(home.join(".claude/settings.json"), "track", policy),
            TrackedEntry::new(home.join(".claude/projects/notes"), "track", policy),
        ];
        let modes = BTreeMap::from([
            (crate::file::display_path(home), 0o700),
            (crate::file::display_path(home.join(".claude")), 0o700),
            (
                crate::file::display_path(home.join(".claude/settings.json")),
                0o600,
            ),
        ]);
        manifest.capture_permissions(&entries, &modes).unwrap();
        assert_eq!(
            manifest.permissions,
            BTreeMap::from([
                ("home/.claude".into(), 0o700),
                ("home/.claude/settings.json".into(), 0o600),
            ])
        );
        manifest.validate().unwrap();
        // the parent went back to the default: its record goes away
        let modes = BTreeMap::from([(
            crate::file::display_path(home.join(".claude/settings.json")),
            0o600,
        )]);
        manifest.capture_permissions(&entries, &modes).unwrap();
        assert_eq!(
            manifest.permissions,
            BTreeMap::from([("home/.claude/settings.json".into(), 0o600)])
        );
    }

    /// A stream marker lives only in the first component: a directory whose
    /// own name contains `@` is an ordinary containing directory, and its
    /// record is replaced on save like any other.
    #[cfg(unix)]
    #[test]
    fn a_directory_named_with_an_at_sign_returns_to_default() {
        use super::super::tracked::TrackedEntry;
        use crate::system::files::{FileMode, FilePolicy};
        let mut manifest = Manifest {
            enrollment: vec![enrollment("home/.private@work/settings.json")],
            ..Default::default()
        };
        let home = &*crate::dirs::HOME;
        let entries = vec![TrackedEntry::new(
            home.join(".private@work/settings.json"),
            "track",
            FilePolicy::for_mode(FileMode::Track),
        )];
        let modes =
            BTreeMap::from([(crate::file::display_path(home.join(".private@work")), 0o700)]);
        manifest.capture_permissions(&entries, &modes).unwrap();
        assert_eq!(
            manifest.permissions,
            BTreeMap::from([("home/.private@work".into(), 0o700)])
        );
        manifest.validate().unwrap();
        manifest
            .capture_permissions(&entries, &BTreeMap::new())
            .unwrap();
        assert!(manifest.permissions.is_empty());
    }

    /// A directory containing files of several streams is one filesystem
    /// object: it is recorded once, without a variant, and every stream
    /// replaces that record on save.
    #[cfg(unix)]
    #[test]
    fn a_containing_directory_is_recorded_once_across_streams() {
        use super::super::tracked::TrackedEntry;
        use crate::system::files::{FileMode, FilePolicy};
        let linux = Variant {
            os: vec!["linux".into()],
            ..Default::default()
        };
        let mut manifest = Manifest {
            enrollment: vec![
                enrollment("home/.claude/settings.json"),
                Enrollment {
                    variants: vec![linux],
                    ..enrollment("home/.claude/notes")
                },
            ],
            permissions: BTreeMap::from([("home@linux/.claude".into(), 0o700)]),
            ..Default::default()
        };
        // a per-stream record for a containing directory is not owned
        assert!(manifest.validate().is_err());
        manifest.remove_unenrolled_permissions();
        assert!(manifest.permissions.is_empty());
        let home = &*crate::dirs::HOME;
        let policy = FilePolicy::for_mode(FileMode::Track);
        let mut notes = TrackedEntry::new(home.join(".claude/notes"), "track", policy);
        notes.variant = Some("linux".into());
        let entries = vec![
            TrackedEntry::new(home.join(".claude/settings.json"), "track", policy),
            notes,
        ];
        let modes = BTreeMap::from([(crate::file::display_path(home.join(".claude")), 0o700)]);
        manifest.capture_permissions(&entries, &modes).unwrap();
        assert_eq!(
            manifest.permissions,
            BTreeMap::from([("home/.claude".into(), 0o700)])
        );
        manifest.validate().unwrap();
        // saving only the variant stream still replaces the shared record
        manifest
            .capture_permissions(&entries[1..], &BTreeMap::new())
            .unwrap();
        assert!(manifest.permissions.is_empty());
    }

    /// An inactive directory enrollment for another platform does not hide
    /// the record of that directory as the parent of an active file.
    #[cfg(unix)]
    #[test]
    fn an_inactive_directory_enrollment_keeps_the_parent_record() {
        use super::super::tracked::TrackedEntry;
        use crate::system::files::{FileMode, FilePolicy};
        let variant = |os: &str| Variant {
            os: vec![os.into()],
            ..Default::default()
        };
        let mut manifest = Manifest {
            enrollment: vec![
                Enrollment {
                    variants: vec![variant("macos")],
                    ..enrollment("home/.claude")
                },
                Enrollment {
                    variants: vec![variant("linux")],
                    ..enrollment("home/.claude/settings.json")
                },
            ],
            permissions: BTreeMap::from([
                ("home@macos/.claude".into(), 0o750),
                ("home/.claude".into(), 0o700),
            ]),
            ..Default::default()
        };
        manifest.validate().unwrap();
        manifest.remove_unenrolled_permissions();
        assert_eq!(manifest.permissions.len(), 2);
        // on Linux only the file is active: its parent is recorded in the
        // shared record and the macOS directory stream is left alone
        let home = &*crate::dirs::HOME;
        let mut entry = TrackedEntry::new(
            home.join(".claude/settings.json"),
            "track",
            FilePolicy::for_mode(FileMode::Track),
        );
        entry.variant = Some("linux".into());
        let modes = BTreeMap::from([(crate::file::display_path(home.join(".claude")), 0o700)]);
        manifest
            .capture_permissions(std::slice::from_ref(&entry), &modes)
            .unwrap();
        assert_eq!(
            manifest.permissions,
            BTreeMap::from([
                ("home@macos/.claude".into(), 0o750),
                ("home/.claude".into(), 0o700),
            ])
        );
        manifest.validate().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn permission_capture_preserves_inactive_streams() {
        use super::super::tracked::TrackedEntry;
        use crate::system::files::{FileMode, FilePolicy};
        let variants = vec![
            Variant {
                os: vec!["linux".into()],
                ..Default::default()
            },
            Variant {
                os: vec!["macos".into()],
                ..Default::default()
            },
        ];
        let mut manifest = Manifest {
            enrollment: vec![Enrollment {
                variants,
                ..enrollment("home/.config-file")
            }],
            permissions: BTreeMap::from([
                ("home@linux/.config-file".into(), 0o600),
                ("home@macos/.config-file".into(), 0o640),
            ]),
            ..Default::default()
        };
        let path = crate::dirs::HOME.join(".config-file");
        let mut entry =
            TrackedEntry::new(path.clone(), "track", FilePolicy::for_mode(FileMode::Track));
        entry.variant = Some("linux".into());
        manifest
            .capture_permissions(
                std::slice::from_ref(&entry),
                &BTreeMap::from([(crate::file::display_path(&path), 0o400)]),
            )
            .unwrap();
        assert_eq!(manifest.permissions["home@linux/.config-file"], 0o400);
        assert_eq!(manifest.permissions["home@macos/.config-file"], 0o640);
        manifest
            .capture_permissions(&[entry], &BTreeMap::new())
            .unwrap();
        assert!(!manifest.permissions.contains_key("home@linux/.config-file"));
        assert_eq!(manifest.permissions["home@macos/.config-file"], 0o640);
        manifest.validate().unwrap();
    }

    fn enrollment(path: &str) -> Enrollment {
        Enrollment {
            path: path.into(),
            autosave: true,
            encrypt: false,
            variants: vec![],
            exclude: vec![],
            include: None,
        }
    }

    #[test]
    fn enrollment_merge_preserves_independent_additions_and_policy_changes() {
        let base = Manifest {
            enrollment: vec![enrollment("home/.shared")],
            ..Default::default()
        };
        let mut local = base.clone();
        local.enrollment[0].autosave = false;
        local.enrollment.push(enrollment("home/.local-added"));
        let mut remote = base.clone();
        remote.enrollment[0].encrypt = true;
        remote.enrollment.push(enrollment("home/.remote-added"));
        let merged = Manifest::merge(&base, &local, &remote).unwrap();
        assert_eq!(merged.enrollment.len(), 3);
        let shared = merged
            .enrollment
            .iter()
            .find(|entry| entry.path == "home/.shared")
            .unwrap();
        assert!(!shared.autosave && shared.encrypt);
        assert_eq!(merged, Manifest::merge(&base, &remote, &local).unwrap());
    }

    #[test]
    fn enrollment_removal_commutes_with_unrelated_addition_but_not_policy_edit() {
        let base = Manifest {
            enrollment: vec![enrollment("home/.removed")],
            ..Default::default()
        };
        let local = Manifest::default();
        let mut remote = base.clone();
        remote.enrollment.push(enrollment("home/.new"));
        let merged = Manifest::merge(&base, &local, &remote).unwrap();
        assert_eq!(merged.enrollment, vec![enrollment("home/.new")]);
        remote.enrollment[0].autosave = false;
        assert!(
            Manifest::merge(&base, &local, &remote)
                .unwrap_err()
                .to_string()
                .contains("home/.removed")
        );
    }

    #[test]
    fn enrollment_security_policy_conflicts_are_not_silently_combined() {
        let base = Manifest::default();
        let mut local = base.clone();
        let mut remote = base.clone();
        local.recipients = vec!["recipient-a".into()];
        remote.recipients = vec!["recipient-b".into()];
        assert!(Manifest::merge(&base, &local, &remote).is_err());
        local.recipients.clear();
        remote.recipients.clear();
        local.enrollment = vec![enrollment("home/.new")];
        remote.enrollment = local.enrollment.clone();
        remote.enrollment[0].encrypt = true;
        assert!(Manifest::merge(&base, &local, &remote).is_err());
    }

    #[test]
    fn ordinary_commit_needs_only_explicit_portable_inventory() {
        let temp = tempfile::tempdir().unwrap();
        let repo = HistoryRepo::open_or_init_in(temp.path()).unwrap().unwrap();
        let manifest = Manifest {
            enrollment: vec![Enrollment {
                path: "home/.zshrc".into(),
                autosave: true,
                encrypt: false,
                variants: vec![],
                exclude: vec![],
                include: None,
            }],
            ..Default::default()
        };
        let tree = repo
            .compose(
                &repo.empty_object("tree").unwrap(),
                &[Overlay {
                    path: "home/.zshrc".into(),
                    object: Some((
                        "100644".into(),
                        repo.hash_blob(b"export EDITOR=vim").unwrap(),
                    )),
                }],
            )
            .unwrap();
        let tree = manifest.write(&repo, &tree).unwrap();
        let commit = repo
            .commit_tree(&tree, vec![], "ordinary Git save")
            .unwrap();
        let record = repo.read_meta(&commit).unwrap();
        assert_eq!(record.description, "ordinary Git save");
        assert_eq!(record.tree.coverage.entries.len(), 1);
        assert_eq!(
            record.tree.coverage.entries[0].path,
            crate::file::display_path(crate::dirs::HOME.join(".zshrc"))
        );
        assert_eq!(record.tree.roots.len(), 1);
        assert_eq!(record.tree.roots[0].files, 1);
        assert!(record.operation.is_none());
    }

    #[test]
    fn capture_preserves_inactive_variants_and_untracking_preserves_ancestors() {
        let temp = tempfile::tempdir().unwrap();
        let Some(repo) = HistoryRepo::open_or_init_in(temp.path()).unwrap() else {
            return;
        };
        let active = Variant {
            os: vec![std::env::consts::OS.into()],
            ..Default::default()
        };
        let inactive = Variant {
            os: vec!["other-test-platform".into()],
            ..Default::default()
        };
        let active_path = format!("home@{}/.zshrc", active.name());
        let inactive_path = format!("home@{}/.zshrc", inactive.name());
        let mut manifest = Manifest {
            enrollment: vec![Enrollment {
                path: "home/.zshrc".into(),
                autosave: true,
                encrypt: false,
                variants: vec![active, inactive],
                exclude: vec![],
                include: None,
            }],
            ..Default::default()
        };
        let empty = repo.empty_object("tree").unwrap();
        let before = repo.hash_blob(b"before").unwrap();
        let after = repo.hash_blob(b"after").unwrap();
        let prior_tree = repo
            .compose(
                &empty,
                &[
                    Overlay {
                        path: active_path.clone(),
                        object: Some(("100644".into(), before.clone())),
                    },
                    Overlay {
                        path: inactive_path.clone(),
                        object: Some(("100755".into(), before.clone())),
                    },
                    Overlay {
                        path: "README.md".into(),
                        object: Some(("100644".into(), before.clone())),
                    },
                ],
            )
            .unwrap();
        let prior_tree = manifest.write(&repo, &prior_tree).unwrap();
        let parent = repo.commit_tree(&prior_tree, vec![], "initial").unwrap();
        repo.update_ref(HistoryRepo::HISTORY_REF, &parent, None)
            .unwrap();
        let captured = repo
            .compose(
                &empty,
                &[Overlay {
                    path: active_path.clone(),
                    object: Some(("100644".into(), after.clone())),
                }],
            )
            .unwrap();
        let next = manifest.preserve_other_files(&repo, &captured).unwrap();
        assert_eq!(
            repo.object_at(&next, &active_path).unwrap(),
            Some(("100644".into(), after))
        );
        assert_eq!(
            repo.object_at(&next, &inactive_path).unwrap(),
            Some(("100755".into(), before.clone()))
        );
        assert!(repo.object_at(&next, "README.md").unwrap().is_some());
        manifest.enrollment.clear();
        let untracked = manifest.preserve_other_files(&repo, &empty).unwrap();
        assert!(repo.object_at(&untracked, &active_path).unwrap().is_none());
        assert!(
            repo.object_at(&untracked, &inactive_path)
                .unwrap()
                .is_none()
        );
        assert!(repo.object_at(&untracked, "README.md").unwrap().is_some());
        assert_eq!(
            repo.object_at(&parent, &active_path).unwrap(),
            Some(("100644".into(), before))
        );
    }

    #[test]
    fn manifest_rejects_unportable_or_repeated_enrollment() {
        let enrollment = Enrollment {
            path: "home/.zshrc".into(),
            autosave: true,
            encrypt: false,
            variants: vec![],
            exclude: vec![],
            include: None,
        };
        let mut manifest = Manifest {
            enrollment: vec![enrollment.clone()],
            ..Default::default()
        };
        manifest.validate().unwrap();
        manifest.enrollment.push(enrollment);
        assert!(manifest.validate().is_err());
        manifest.enrollment.pop();
        for path in [
            "fs/etc/passwd",
            "home/../x",
            "home/.git/config",
            "home/a\\b",
        ] {
            manifest.enrollment[0].path = path.into();
            assert!(manifest.validate().is_err());
        }
    }
    #[test]
    fn an_unparsable_enrollment_exclude_pattern_is_rejected() {
        let mut manifest = Manifest {
            enrollment: vec![Enrollment {
                path: "home/.codex".into(),
                autosave: true,
                encrypt: false,
                variants: vec![],
                exclude: vec!["sessions".into()],
                include: None,
            }],
            ..Default::default()
        };
        assert!(manifest.validate().is_ok());
        manifest.enrollment[0].exclude.push("[".into());
        let error = manifest.validate().unwrap_err().to_string();
        assert!(error.contains("invalid exclude pattern"), "{error}");
        assert!(error.contains("home/.codex"), "{error}");
    }
}
