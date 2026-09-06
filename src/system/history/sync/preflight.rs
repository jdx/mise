//! Read-only discovery of the declarations an incoming configuration would
//! activate. No live config is replaced, trusted, rendered, or executed here.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eyre::{Result, bail};

use super::layout::{Roots, is_configuration};
use super::reconcile::{Object, PathPlan};
use crate::config::ConfigMap;
use crate::config::config_file::mise_toml::MiseToml;
use crate::system::history::shadow::HistoryRepo;
use crate::system::history::tracked::{EntryKind, TrackedSet};

pub(super) fn prospective(
    repo: &HistoryRepo,
    tracked: &TrackedSet,
    plans: &[PathPlan],
) -> Result<TrackedSet> {
    let roots = Roots::current();
    let incoming: BTreeMap<PathBuf, Option<Object>> = plans
        .iter()
        .filter(|plan| is_configuration(&plan.branch_path))
        .filter_map(|plan| {
            Some((
                roots.locate(&plan.branch_path).path()?.to_path_buf(),
                plan.apply.clone()?,
            ))
        })
        .collect();
    if incoming.is_empty() {
        return Ok(tracked.clone());
    }
    let paths: BTreeSet<PathBuf> = incoming.keys().cloned().collect();
    let global = match &*crate::env::MISE_GLOBAL_CONFIG_FILE {
        Some(path) => vec![path.clone()].into_iter().collect(),
        None => crate::config::config_files_with_incoming(&roots.config_dir, &paths),
    };
    let mut files = ConfigMap::new();
    let mut excludes = vec![];
    for path in crate::config::system_config_files()
        .into_iter()
        .chain(global)
    {
        if path.extension().is_none_or(|ext| ext != "toml") {
            continue;
        }
        let body = match incoming.get(&path) {
            Some(Some((mode, oid))) if mode == "100644" || mode == "100755" => {
                String::from_utf8(repo.cat_object(oid)?)?
            }
            Some(Some(_)) => bail!(
                "incoming configuration must be a regular file: {}",
                path.display()
            ),
            Some(None) => continue,
            None => match std::fs::read_to_string(&path) {
                Ok(body) => body,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                Err(err) => return Err(err.into()),
            },
        };
        let parsed = MiseToml::for_history_preflight(&body, &path)?;
        if let Some(history) = parsed.history_config() {
            excludes.extend(history.exclude);
            if history.origin.is_some_and(|origin| origin.encrypt_backups) {
                bail!(
                    "encrypted backups are not supported yet ({})",
                    path.display()
                );
            }
        }
        files.insert(
            path,
            Arc::new(parsed) as Arc<dyn crate::config::config_file::ConfigFile>,
        );
    }
    // File composition expects highest precedence first.
    files.reverse();
    crate::system::files::validate_incoming_files(&files)?;
    let requests = crate::system::files::files_from_config_files(&files);
    crate::system::files::validate_composed_file_footprints(&requests)?;
    let mut prospective = tracked.clone();
    // Live discovery can carry diagnostics for ignored project declarations;
    // those are not part of this global incoming setup.
    prospective.invalid.clear();
    prospective.entries.retain(|entry| {
        entry
            .declared_in
            .as_ref()
            .is_none_or(|path| !files.contains_key(path) && !incoming.contains_key(path))
    });
    prospective.exclude = excludes;
    prospective.add_requests(requests);
    if let Some(invalid) = prospective.invalid.first() {
        bail!("{}: {}", invalid.path, invalid.reason);
    }
    prospective.exclude_set()?;
    Ok(prospective)
}

/// Required source files must exist in the complete proposed write set or
/// already be available locally. A queued deletion is not an available source.
pub(super) fn sources(repo: &HistoryRepo, tracked: &TrackedSet, plans: &[PathPlan]) -> Result<()> {
    let roots = Roots::current();
    for entry in &tracked.entries {
        if entry.kind != EntryKind::Source {
            continue;
        }
        let planned = plans
            .iter()
            .find(|plan| roots.locate(&plan.branch_path).path() == Some(entry.path.as_path()));
        match planned.and_then(|plan| plan.apply.as_ref()) {
            Some(Some((mode, oid))) => {
                if mode != "100644" && mode != "100755" {
                    bail!(
                        "required source is not a regular file: {}",
                        entry.path.display()
                    );
                }
                repo.cat_object(oid)?;
            }
            Some(None) => bail!(
                "incoming setup deletes required source {}",
                entry.path.display()
            ),
            None if !entry.path.exists() && !has_incoming_child(&roots, &entry.path, plans) => {
                bail!(
                    "incoming setup is missing required source {}",
                    entry.path.display()
                );
            }
            None => {}
        }
    }
    Ok(())
}

fn has_incoming_child(roots: &Roots, path: &Path, plans: &[PathPlan]) -> bool {
    plans.iter().any(|plan| {
        plan.apply.as_ref().is_some_and(Option::is_some)
            && roots
                .locate(&plan.branch_path)
                .path()
                .is_some_and(|p| p.starts_with(path))
    })
}
