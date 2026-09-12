//! Directory permissions are metadata-only writes in the same incoming batch.
use std::path::PathBuf;

use eyre::{Result, bail};

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
    let roots = super::layout::Roots::current();
    let mut steps = vec![];
    let paths: std::collections::BTreeSet<_> = tracked
        .manifest
        .permissions
        .keys()
        .chain(saved.permissions.keys())
        .collect();
    for portable in paths {
        if !super::run::eligible(&roots, tracked, portable)
            || repo
                .object_at(tree, portable)?
                .is_none_or(|(mode, _)| mode != "040000")
        {
            continue;
        }
        let path = roots.locate(portable).path().unwrap().to_path_buf();
        let before = observe(&path)?;
        let desired = tracked
            .manifest
            .permissions
            .get(portable)
            .copied()
            .unwrap_or(0o755);
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
        if before.map(|(_, _, bits)| bits) != Some(desired) {
            steps.push(Step {
                path,
                before,
                desired,
                written: None,
            });
        }
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
