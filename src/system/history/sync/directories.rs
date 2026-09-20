//! Directory permissions are metadata-only writes in the same incoming batch.
use std::path::PathBuf;

use eyre::{Result, bail};

use super::layout::{Located, Roots};
use crate::system::history::{
    journal, manifest::Manifest, shadow::HistoryRepo, tracked::TrackedSet,
};

#[derive(Clone, Debug)]
pub(super) struct Step {
    pub path: PathBuf,
    before: Option<(u64, u64, u32)>,
    desired: u32,
    written: Option<(u64, u64, u32)>,
}

#[cfg(unix)]
fn observe(path: &std::path::Path) -> Result<Option<(u64, u64, u32)>> {
    use std::os::unix::fs::MetadataExt;
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => Ok(Some((meta.dev(), meta.ino(), meta.mode() & 0o777))),
        Ok(_) => bail!(
            "{} is not a directory; resolve its type before pulling",
            crate::file::display_path(path)
        ),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}

#[cfg(not(unix))]
fn observe(_path: &std::path::Path) -> Result<Option<(u64, u64, u32)>> {
    Ok(None)
}

/// Why a directory's recorded permissions apply here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Claim {
    /// An enrolled path's own stream, or one below an enrolled path.
    Enrolled,
    /// The variant-less record of a directory strictly between the root and
    /// an enrolled path of any stream. The root itself is never claimed.
    Containing,
}

fn claim(roots: &Roots, tracked: &TrackedSet, branch_path: &str) -> Option<Claim> {
    if super::run::eligible(roots, tracked, branch_path) {
        return Some(Claim::Enrolled);
    }
    let path = match roots.locate(branch_path) {
        Located::Tracked {
            path,
            variant: None,
        }
        | Located::Config(path) => path,
        Located::Tracked { .. } | Located::Marker | Located::Unmapped => return None,
    };
    (path != roots.home
        && path != roots.config_dir
        && tracked
            .entries
            .iter()
            .any(|entry| entry.path.starts_with(&path) && entry.path != path))
    .then_some(Claim::Containing)
}

pub(super) fn plan(repo: &HistoryRepo, tracked: &TrackedSet, tree: &str) -> Result<Vec<Step>> {
    if !cfg!(unix) {
        return Ok(vec![]);
    }
    let local = repo.ref_oid(HistoryRepo::HISTORY_REF)?;
    let saved = local
        .as_deref()
        .map(|head| Manifest::read(repo, head))
        .transpose()?
        .flatten()
        .unwrap_or_default();
    let roots = Roots::current();
    let mut steps = vec![];
    let paths: std::collections::BTreeSet<_> = tracked
        .manifest
        .permissions
        .keys()
        .chain(saved.permissions.keys())
        .collect();
    // one step per directory: an enrolled directory's own stream outranks
    // the record it gets as the parent of other enrolled paths
    let mut claimed: std::collections::BTreeMap<PathBuf, (Claim, &String)> = Default::default();
    for portable in paths {
        let Some(claim) = claim(&roots, tracked, portable) else {
            continue;
        };
        if repo
            .object_at(tree, portable)?
            .is_none_or(|(mode, _)| mode != "040000")
        {
            continue;
        }
        let path = roots.locate(portable).path().unwrap().to_path_buf();
        let best = claimed.entry(path).or_insert((claim, portable));
        if claim < best.0 {
            *best = (claim, portable);
        }
    }
    for (path, (_, portable)) in claimed {
        let before = observe(&path)?;
        let desired = tracked
            .manifest
            .permissions
            .get(portable)
            .copied()
            .unwrap_or(0o755);
        // a directory that already has the incoming mode needs nothing,
        // whether its local record agrees, predates the capture of
        // containing directories, or is an unsaved chmod to the same bits
        if before.map(|(_, _, bits)| bits) == Some(desired) {
            continue;
        }
        let was_directory = local
            .as_deref()
            .map(|head| repo.object_at(head, portable))
            .transpose()?
            .flatten()
            .is_some_and(|(mode, _)| mode == "040000");
        if was_directory
            && before.map(|(_, _, bits)| bits)
                != Some(saved.permissions.get(portable).copied().unwrap_or(0o755))
        {
            bail!(
                "{} has unsaved directory permission changes; save them before pulling. Sharing is paused",
                crate::file::display_path(&path)
            );
        }
        steps.push(Step {
            path,
            before,
            desired,
            written: None,
        });
    }
    steps.sort_by_key(|step| step.path.components().count());
    Ok(steps)
}

impl Step {
    pub(super) fn validate(&self) -> Result<()> {
        if observe(&self.path)? != self.before {
            bail!(
                "{} changed after permission planning; retry pull",
                crate::file::display_path(&self.path)
            );
        }
        Ok(())
    }

    pub(super) fn apply(&mut self) -> Result<()> {
        self.validate()?;
        let pending = journal::begin_changes_with(
            "history",
            "directory permissions",
            [(self.path.clone(), journal::Capture::Shallow)],
        )?;
        if self.before.is_none() {
            crate::file::create_dir_all(&self.path)?;
        }
        set_mode(&self.path, self.desired)?;
        self.written = observe(&self.path)?;
        journal::commit_changes(pending);
        Ok(())
    }

    pub(super) fn verify_written(&self) -> Result<()> {
        if observe(&self.path)? != self.written {
            bail!(
                "{} changed during application; left untouched",
                crate::file::display_path(&self.path)
            );
        }
        Ok(())
    }

    pub(super) fn recover(&self) -> Result<()> {
        if self.written.is_none() {
            return Ok(());
        }
        self.verify_written()?;
        match self.before {
            Some((_, _, mode)) => set_mode(&self.path, mode),
            None => std::fs::remove_dir(&self.path).map_err(Into::into),
        }
    }
}

fn set_mode(path: &std::path::Path, bits: u32) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(bits))?;
    }
    #[cfg(not(unix))]
    let _ = (path, bits);
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn directories_containing_tracked_files_are_eligible() {
        use crate::system::files::{FileMode, FilePolicy};
        use crate::system::history::tracked::TrackedEntry;
        let roots = Roots {
            home: "/home/u".into(),
            config_dir: "/config/mise".into(),
        };
        let policy = FilePolicy::for_mode(FileMode::Track);
        let mut variant = TrackedEntry::new("/home/u/.ssh/config".into(), "track", policy);
        variant.variant = Some("linux".into());
        let tracked = TrackedSet {
            entries: vec![
                TrackedEntry::new("/home/u/.claude/settings.json".into(), "track", policy),
                TrackedEntry::new("/config/mise/tasks/private/build".into(), "track", policy),
                variant,
            ],
            ..Default::default()
        };
        for path in [
            "home/.claude",
            "home/.claude/settings.json",
            "config/tasks",
            "config/tasks/private",
            "home/.ssh",
        ] {
            assert!(claim(&roots, &tracked, path).is_some(), "{path}");
        }
        assert_eq!(
            claim(&roots, &tracked, "home/.claude/settings.json"),
            Some(Claim::Enrolled)
        );
        assert_eq!(
            claim(&roots, &tracked, "home/.ssh"),
            Some(Claim::Containing)
        );
        // a containing directory has no per-stream record
        for path in [
            "home",
            "config",
            "home/.claudia",
            "home/.claude/other",
            "home@linux/.claude",
            "home@linux/.ssh",
            "config@linux/tasks",
            "fs/etc",
        ] {
            assert!(claim(&roots, &tracked, path).is_none(), "{path}");
        }
    }

    /// A directory that is both an enrolled stream and the parent of other
    /// enrolled paths gets one step, from its own stream: two steps for one
    /// missing directory would create it twice and abort a fresh bootstrap.
    #[test]
    fn one_step_per_directory_across_claims() -> Result<()> {
        use crate::system::files::{FileMode, FilePolicy};
        use crate::system::history::manifest::Enrollment;
        use crate::system::history::shadow::Overlay;
        use crate::system::history::tracked::{TrackedEntry, normalize};

        let state = tempfile::tempdir()?;
        let Some(repo) = HistoryRepo::open_or_init_in(state.path())? else {
            return Ok(());
        };
        let roots = Roots::current();
        let scratch = tempfile::Builder::new()
            .prefix(".history-claims-")
            .tempdir_in(&roots.home)?;
        let private = normalize(scratch.path()).join("private");
        let policy = FilePolicy::for_mode(FileMode::Track);
        let mut directory = TrackedEntry::new(private.clone(), "track", policy);
        directory.variant = Some("linux".into());
        let file = TrackedEntry::new(private.join("settings.json"), "track", policy);
        let own = directory.tree_path(&private)?;
        let containing = roots.branch_path(&private, None).unwrap();
        let blob = repo.hash_blob(b"{}")?;
        let files = repo.compose(
            &repo.empty_object("tree")?,
            &[
                Overlay {
                    path: directory.tree_path(&private.join("notes"))?,
                    object: Some(("100644".into(), blob.clone())),
                },
                Overlay {
                    path: file.tree_path(&file.path)?,
                    object: Some(("100644".into(), blob)),
                },
            ],
        )?;
        let variant = crate::system::history::select::Variant {
            os: vec!["linux".into()],
            ..Default::default()
        };
        let mut manifest = Manifest {
            enrollment: vec![
                Enrollment {
                    path: containing.clone(),
                    autosave: true,
                    encrypt: false,
                    variants: vec![variant],
                },
                Enrollment {
                    path: file.tree_path(&file.path)?,
                    autosave: true,
                    encrypt: false,
                    variants: vec![],
                },
            ],
            permissions: std::collections::BTreeMap::from([
                (own.clone(), 0o700),
                (containing.clone(), 0o750),
            ]),
            ..Default::default()
        };
        // a fresh machine: no local history, the directory does not exist
        let tree = manifest.write(&repo, &files)?;
        let tracked = TrackedSet {
            entries: vec![directory, file],
            manifest: manifest.clone(),
            ..Default::default()
        };
        let steps = plan(&repo, &tracked, &tree)?;
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].path, private);
        assert_eq!(steps[0].desired, 0o700);
        assert!(steps[0].before.is_none());

        // without its own stream, the containing record applies
        manifest.permissions.remove(&own);
        let tree = manifest.write(&repo, &files)?;
        let tracked = TrackedSet {
            manifest,
            ..tracked
        };
        let steps = plan(&repo, &tracked, &tree)?;
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].desired, 0o750);
        Ok(())
    }

    /// A machine that already holds the parent at the incoming mode pulls
    /// without saving first, even though its own manifest (written before
    /// containing directories were captured) records nothing for it.
    #[test]
    fn matching_parent_without_a_local_record_is_not_unsaved() -> Result<()> {
        use crate::system::files::{FileMode, FilePolicy};
        use crate::system::history::checkpoint::test_checkpoint;
        use crate::system::history::manifest::Enrollment;
        use crate::system::history::shadow::Overlay;
        use crate::system::history::tracked::{TrackedEntry, normalize};
        use std::os::unix::fs::PermissionsExt;

        let state = tempfile::tempdir()?;
        let Some(repo) = HistoryRepo::open_or_init_in(state.path())? else {
            return Ok(());
        };
        let roots = Roots::current();
        let parent = tempfile::Builder::new()
            .prefix(".history-parent-")
            .tempdir_in(&roots.home)?;
        let parent_path = normalize(parent.path());
        let settings = parent_path.join("settings.json");
        std::fs::write(&settings, "{}")?;
        std::fs::set_permissions(&parent_path, std::fs::Permissions::from_mode(0o700))?;
        let policy = FilePolicy::for_mode(FileMode::Track);
        let entry = TrackedEntry::new(settings.clone(), "track", policy);
        let file = entry.tree_path(&settings)?;
        let portable = roots.branch_path(&parent_path, None).unwrap();
        let blob = repo.hash_blob(b"{}")?;
        let files = repo.compose(
            &repo.empty_object("tree")?,
            &[Overlay {
                path: file.clone(),
                object: Some(("100644".into(), blob)),
            }],
        )?;
        let mut manifest = Manifest {
            enrollment: vec![Enrollment {
                path: file,
                autosave: true,
                encrypt: false,
                variants: vec![],
            }],
            ..Default::default()
        };
        // the local head predates the capture of containing directories
        let local = manifest.write(&repo, &files)?;
        repo.write_checkpoint(Some(&local), &test_checkpoint("local", Some(&local)))?;
        manifest.permissions.insert(portable.clone(), 0o700);
        let incoming = manifest.write(&repo, &files)?;
        let tracked = TrackedSet {
            entries: vec![entry],
            manifest,
            ..Default::default()
        };
        assert!(plan(&repo, &tracked, &incoming)?.is_empty());

        // a different unsaved mode still pauses sharing
        std::fs::set_permissions(&parent_path, std::fs::Permissions::from_mode(0o710))?;
        let err = plan(&repo, &tracked, &incoming).unwrap_err();
        assert!(
            err.to_string().contains("unsaved directory permission"),
            "{err}"
        );

        // the default matches the missing local record: the incoming mode
        // is applied
        std::fs::set_permissions(&parent_path, std::fs::Permissions::from_mode(0o755))?;
        let steps = plan(&repo, &tracked, &incoming)?;
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].path, parent_path);
        assert_eq!(steps[0].desired, 0o700);
        Ok(())
    }

    #[test]
    fn permission_transaction_preserves_concurrent_changes() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("private");
        std::fs::create_dir(&path)?;
        set_mode(&path, 0o755)?;
        let mut step = Step {
            path: path.clone(),
            before: observe(&path)?,
            desired: 0o700,
            written: None,
        };
        step.apply()?;
        std::fs::write(path.join("user-edit"), "keep")?;
        step.recover()?;
        assert_eq!(observe(&path)?.unwrap().2, 0o755);
        assert_eq!(std::fs::read_to_string(path.join("user-edit"))?, "keep");
        step.before = observe(&path)?;
        step.apply()?;
        set_mode(&path, 0o750)?;
        assert!(step.recover().is_err());
        assert_eq!(observe(&path)?.unwrap().2, 0o750);
        Ok(())
    }
}
