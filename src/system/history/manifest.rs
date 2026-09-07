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
    fn owns_stream(&self, path: &str) -> bool {
        let (stem, relative) = path.split_once('/').unwrap_or((path, ""));
        let (root, variant) = stem
            .split_once('@')
            .map_or((stem, None), |(root, variant)| (root, Some(variant)));
        let portable = if relative.is_empty() {
            root.to_string()
        } else {
            format!("{root}/{relative}")
        };
        self.owner(&portable).is_some_and(|entry| match variant {
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
        self.permissions.retain(|path, _| {
            !active.iter().any(|prefix| {
                path == prefix
                    || path
                        .strip_prefix(prefix)
                        .is_some_and(|rest| rest.starts_with('/'))
            })
        });
        for (display, bits) in modes {
            let path = crate::file::replace_path(display);
            if let Some(entry) = entries
                .iter()
                .filter(|entry| path.starts_with(&entry.path))
                .max_by_key(|entry| entry.path.components().count())
            {
                let portable = roots
                    .branch_path(&path, entry.variant.as_deref())
                    .ok_or_else(|| eyre::eyre!("cannot map permission path {display}"))?;
                self.permissions.insert(portable, *bits);
            }
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
            let mut policy =
                crate::system::files::FilePolicy::for_mode(crate::system::files::FileMode::Track);
            policy.autosave = enrollment.autosave;
            policy.encrypt = enrollment.encrypt;
            let mut entry = super::tracked::TrackedEntry::new(local, "track", policy);
            entry.variant = variant;
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

    fn owner(&self, path: &str) -> Option<&Enrollment> {
        self.enrollment
            .iter()
            .filter(|entry| {
                path == entry.path
                    || path
                        .strip_prefix(&entry.path)
                        .is_some_and(|rest| rest.starts_with('/'))
            })
            .max_by_key(|entry| entry.path.len())
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
        let manifest: Self =
            serde_json::from_slice(&repo.cat_object_bounded(&oid, 4 * 1024 * 1024)?)?;
        manifest.validate()?;
        Ok(Some(manifest))
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
