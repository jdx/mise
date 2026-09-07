//! Reconcile explicit local declaration edits with the repository's inventory.
//! The observation cache is replaceable bookkeeping, never file history.

use std::path::{Path, PathBuf};

use eyre::Result;
use serde::{Deserialize, Serialize};

use super::manifest::Manifest;
use super::shadow::HistoryRepo;
use super::tracked::TrackedSet;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Observation {
    head: String,
    declarations: Manifest,
}

fn cache_path(state_dir: &Path) -> PathBuf {
    super::store::index_dir_in(state_dir).join("declarations.json")
}

/// Read-only resolution. Explicit CLI enrollment/removal wins; otherwise only
/// changes to local declarations update the inventory, not unchanged copies.
pub(crate) fn resolve(
    state_dir: &Path,
    repo: &HistoryRepo,
    tracked: &TrackedSet,
    enroll: &[PathBuf],
    untrack: &[PathBuf],
) -> Result<TrackedSet> {
    let Some(current) = &tracked.declarations else {
        return Ok(tracked.clone());
    };
    let Some(head) = repo.ref_oid(HistoryRepo::HISTORY_REF)? else {
        return Ok(tracked.clone());
    };
    let Some(saved) = Manifest::read(repo, &head)? else {
        return Ok(tracked.clone());
    };
    let previous = match std::fs::read(cache_path(state_dir)) {
        Ok(bytes) => match serde_json::from_slice::<Observation>(&bytes) {
            Ok(observation)
                if repo
                    .ref_oid(&format!("{}^{{commit}}", observation.head))?
                    .is_some()
                    && repo
                        .merge_bases(&observation.head, &head)?
                        .contains(&observation.head) =>
            {
                Some(observation.declarations)
            }
            _ => None,
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let historical = if previous.is_none() && !current.enrollment.is_empty() {
        repo.rev_list(&head, usize::MAX)?
            .iter()
            .map(|commit| Manifest::read(repo, commit))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect()
    } else {
        vec![]
    };
    let mut manifest = reconcile(&saved, current, previous.as_ref(), &historical);
    let roots = super::sync::layout::Roots::current();
    for path in enroll {
        if let Some(portable) = roots.branch_path(path, None)
            && let Some(declaration) = current
                .enrollment
                .iter()
                .find(|entry| entry.path == portable)
        {
            manifest.enrollment.retain(|entry| entry.path != portable);
            manifest.enrollment.push(declaration.clone());
        }
    }
    for path in untrack.iter().chain(&tracked.disabled) {
        if let Some(portable) = roots.branch_path(path, None) {
            manifest.enrollment.retain(|entry| entry.path != portable);
        }
    }
    manifest.enrollment.sort_by(|a, b| a.path.cmp(&b.path));
    manifest.remove_unenrolled_permissions();
    let mut resolved = manifest.tracking()?;
    for entry in &mut resolved.entries {
        entry.declared_in = tracked
            .entries
            .iter()
            .find(|declared| declared.path == entry.path)
            .and_then(|declared| declared.declared_in.clone());
    }
    resolved.required_sources = tracked.required_sources.clone();
    resolved.invalid = tracked.invalid.clone();
    resolved.declarations = Some(current.clone());
    resolved.disabled = tracked.disabled.clone();
    Ok(resolved)
}

fn reconcile(
    saved: &Manifest,
    current: &Manifest,
    previous: Option<&Manifest>,
    historical: &[Manifest],
) -> Manifest {
    let mut result = saved.clone();
    for entry in &current.enrollment {
        let old = previous.and_then(|previous| {
            previous
                .enrollment
                .iter()
                .find(|old| old.path == entry.path)
        });
        if old == Some(entry) {
            continue;
        }
        // When the cache is missing, an old declaration seen in Git is not
        // authority to resurrect a remotely untracked path or old policy.
        if previous.is_none()
            && historical
                .iter()
                .any(|manifest| manifest.enrollment.contains(entry))
        {
            continue;
        }
        if let Some(existing) = result
            .enrollment
            .iter_mut()
            .find(|saved| saved.path == entry.path)
        {
            if let Some(old) = old {
                if entry.autosave != old.autosave {
                    existing.autosave = entry.autosave;
                }
                if entry.encrypt != old.encrypt {
                    existing.encrypt = entry.encrypt;
                }
                if entry.variants != old.variants {
                    existing.variants = entry.variants.clone();
                }
            } else {
                *existing = entry.clone();
            }
        } else {
            result.enrollment.push(entry.clone());
        }
    }
    if let Some(previous) = previous {
        for old in &previous.enrollment {
            if !current
                .enrollment
                .iter()
                .any(|entry| entry.path == old.path)
            {
                result.enrollment.retain(|entry| entry.path != old.path);
            }
        }
        // Exclusions are an ordered program, not a set: sorting or removing
        // duplicates changes the meaning of negation and repeated patterns.
        if current.exclude != previous.exclude {
            result.exclude = saved
                .exclude
                .iter()
                .filter(|item| !previous.exclude.contains(item) && !current.exclude.contains(item))
                .chain(current.exclude.iter())
                .cloned()
                .collect();
        }
        if current.recipients != previous.recipients && !current.recipients.is_empty() {
            result.recipients = current.recipients.clone();
        }
    } else {
        for exclusion in &current.exclude {
            if !result.exclude.contains(exclusion) {
                result.exclude.push(exclusion.clone());
            }
        }
        if result.recipients.is_empty() {
            result.recipients = current.recipients.clone();
        }
    }
    result.enrollment.sort_by(|a, b| a.path.cmp(&b.path));
    result
}

/// Confirm local declarations only after the corresponding tree was saved.
pub(crate) fn confirm(state_dir: &Path, repo: &HistoryRepo, tracked: &TrackedSet) -> Result<()> {
    if let Some(declarations) = &tracked.declarations
        && let Some(head) = repo.ref_oid(HistoryRepo::HISTORY_REF)?
    {
        super::store::write_json(
            &cache_path(state_dir),
            &Observation {
                head,
                declarations: declarations.clone(),
            },
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::history::manifest::Enrollment;

    fn manifest(path: &str) -> Manifest {
        Manifest {
            enrollment: vec![Enrollment {
                path: path.into(),
                autosave: true,
                encrypt: false,
                variants: vec![],
            }],
            ..Default::default()
        }
    }

    #[test]
    fn exclusion_edits_preserve_order_repeats_and_remote_additions() {
        let mut previous = manifest("home/config");
        previous.exclude = vec!["~/config/**".into()];
        let mut current = previous.clone();
        current.exclude.push("!~/config/keep".into());
        let mut saved = previous.clone();
        saved.exclude.push("~/remote/**".into());
        let merged = reconcile(&saved, &current, Some(&previous), &[]);
        assert_eq!(
            merged.exclude,
            vec!["~/remote/**", "~/config/**", "!~/config/keep"]
        );
        assert_eq!(
            reconcile(&merged, &current, Some(&current), &[]).exclude,
            merged.exclude
        );
        let mut repeated = current.clone();
        repeated.exclude.push("~/config/**".into());
        assert_eq!(
            reconcile(&current, &repeated, Some(&current), &[]).exclude,
            repeated.exclude
        );
    }

    #[test]
    fn unchanged_config_cannot_resurrect_remote_untracking_even_without_cache() {
        let old = manifest("home/.zshrc");
        let removed = Manifest::default();
        assert!(
            reconcile(&removed, &old, Some(&old), &[])
                .enrollment
                .is_empty()
        );
        assert!(
            reconcile(&removed, &old, None, std::slice::from_ref(&old))
                .enrollment
                .is_empty()
        );
    }

    #[test]
    fn manifest_only_enrollment_survives_and_new_local_declarations_are_added() {
        let saved = manifest("home/.zshrc");
        assert_eq!(
            reconcile(
                &saved,
                &Manifest::default(),
                Some(&Manifest::default()),
                &[]
            ),
            saved
        );
        let current = manifest("home/.new");
        let result = reconcile(&saved, &current, Some(&Manifest::default()), &[]);
        assert_eq!(result.enrollment.len(), 2);
        let removed = reconcile(&result, &Manifest::default(), Some(&current), &[]);
        assert_eq!(removed.enrollment, saved.enrollment);
    }

    #[test]
    fn policy_edits_preserve_independent_remote_changes() {
        let old = manifest("home/.zshrc");
        let mut saved = old.clone();
        saved.enrollment[0].encrypt = true;
        let mut current = old.clone();
        current.enrollment[0].autosave = false;
        let result = reconcile(&saved, &current, Some(&old), &[]);
        assert!(result.enrollment[0].encrypt);
        assert!(!result.enrollment[0].autosave);
    }

    #[test]
    fn invalid_or_missing_observation_rebuilds_without_reenrolling_old_declarations() -> Result<()>
    {
        let temp = tempfile::tempdir()?;
        let repo = HistoryRepo::open_or_init_in(temp.path())?.unwrap();
        let old = manifest("home/.zshrc");
        let tree = old.write(&repo, &repo.empty_object("tree")?)?;
        let initial = repo.commit_tree(&tree, vec![], "enroll")?;
        let removed = Manifest::default().write(&repo, &repo.empty_object("tree")?)?;
        let head = repo.commit_tree(&removed, vec![&initial], "untrack")?;
        repo.update_history_head(&head, None)?;
        let declared = TrackedSet {
            declarations: Some(old.clone()),
            ..Default::default()
        };
        assert!(
            resolve(temp.path(), &repo, &declared, &[], &[])?
                .entries
                .is_empty()
        );
        crate::file::create_dir_all(cache_path(temp.path()).parent().unwrap())?;
        std::fs::write(cache_path(temp.path()), b"invalid cache")?;
        assert!(
            resolve(temp.path(), &repo, &declared, &[], &[])?
                .entries
                .is_empty()
        );
        super::super::store::write_json(
            &cache_path(temp.path()),
            &Observation {
                head: "0000000000000000000000000000000000000000".into(),
                declarations: Manifest::default(),
            },
        )?;
        assert!(
            resolve(temp.path(), &repo, &declared, &[], &[])?
                .entries
                .is_empty()
        );
        let path = super::super::sync::layout::Roots::current()
            .home
            .join(".zshrc");
        assert_eq!(
            resolve(temp.path(), &repo, &declared, &[path], &[])?.manifest,
            old
        );
        Ok(())
    }
}
