//! Publication from mise's bare repository: a commit built with a
//! temporary index from the fetched upstream tree plus the accepted
//! changes, pushed with a lease on the branch. No working tree, no user
//! checkout, never a force push: unrelated files and manual commits
//! survive by construction.

use std::collections::BTreeMap;

use eyre::Result;

use super::format::marker_content;
use super::layout::MARKER_PATH;
use super::network::{PUBLISH_REF, PushOutcome, Remote};
use super::reconcile::Object;
use crate::system::history::shadow::{HistoryRepo, Overlay};

pub(crate) struct Publication<'a> {
    pub upstream_commit: Option<&'a str>,
    /// `None` deletes the path.
    pub changes: BTreeMap<String, Option<Object>>,
    /// Write the format marker (an empty repository, or a confirmed
    /// adoption of an unmarked one).
    pub add_marker: bool,
    pub message: String,
}

/// Builds the publication commit; `None` when nothing would change.
pub(crate) fn build(repo: &HistoryRepo, publication: &Publication<'_>) -> Result<Option<String>> {
    let base = match publication.upstream_commit {
        Some(commit) => repo.output_tree_of(commit)?,
        None => repo.empty_object("tree")?,
    };
    let mut overlays: Vec<Overlay> = publication
        .changes
        .iter()
        .map(|(path, object)| Overlay {
            path: path.clone(),
            object: object.clone(),
        })
        .collect();
    if publication.add_marker
        && !publication.changes.contains_key(MARKER_PATH)
        && (publication.upstream_commit.is_none()
            || repo
                .object_at(publication.upstream_commit.unwrap_or_default(), MARKER_PATH)?
                .is_none())
    {
        let oid = repo.hash_blob(marker_content().as_bytes())?;
        overlays.push(Overlay {
            path: MARKER_PATH.to_string(),
            object: Some(("100644".into(), oid)),
        });
    }
    if overlays.is_empty() {
        return Ok(None);
    }
    let tree = repo.compose(&base, &overlays)?;
    if tree == base {
        return Ok(None);
    }
    let parents: Vec<&str> = publication.upstream_commit.into_iter().collect();
    let commit = repo.commit_tree(&tree, parents, &publication.message)?;
    let previous = repo.ref_oid(PUBLISH_REF)?;
    repo.update_ref(PUBLISH_REF, &commit, previous.as_deref())?;
    Ok(Some(commit))
}

/// Pushes the publication commit to the branch, leased on the upstream
/// head it was built from.
pub(crate) fn push(
    remote: &Remote<'_>,
    branch: &str,
    commit: &str,
    upstream_commit: Option<&str>,
) -> Result<PushOutcome> {
    remote.push(
        &[format!("{commit}:refs/heads/{branch}")],
        Some((branch, upstream_commit)),
    )
}

/// The message of a publication commit.
pub(crate) fn message(machine: &str, changes: &BTreeMap<String, Option<Object>>) -> String {
    let mut paths: Vec<&String> = changes.keys().collect();
    paths.sort();
    let shown: Vec<String> = paths.iter().take(5).map(|p| p.to_string()).collect();
    let more = paths.len().saturating_sub(5);
    let list = if more > 0 {
        format!("{} +{more} more", shown.join(", "))
    } else {
        shown.join(", ")
    };
    format!("mise bootstrap dotfiles history: {machine} published {list}")
}
