//! The per-path transition table. For every setup-branch path the inputs
//! are **S** (the local saved version), **T** (the fetched upstream
//! version), and the recorded **A** (acknowledged), **U** (reconciled), and
//! **L** (applied) versions:
//!
//! | # | condition             | publication          | application                 |
//! |---|-----------------------|----------------------|-----------------------------|
//! | 1 | S == A, T == U        | none                 | none                        |
//! | 2 | S != A, T == U        | publish S            | none (live already holds S) |
//! | 3 | S == A, T != U        | none                 | write T                     |
//! | 4 | S != A, T != U, clean | publish merge(A,S,T) | write the merge             |
//! | 5 | S != A, T != U, clash | none                 | none: a conflict to decide  |
//!
//! Publication always merges relative to A, never relative to L, so a stale
//! local file is never published as a reversal, and repeating a sync
//! changes nothing. A path never reconciled before is adopted: one side
//! present becomes the base; both present and different need a decision.

use std::collections::{BTreeMap, BTreeSet};

use eyre::Result;
use serde::{Deserialize, Serialize};

use super::state::{SyncRecord, SyncState};
use crate::system::history::shadow::HistoryRepo;

/// A blob with its mode.
pub(crate) type Object = (String, String);

/// The fetched upstream head, by setup-branch path.
#[derive(Debug, Default)]
pub(crate) struct Upstream {
    pub commit: Option<String>,
    pub files: BTreeMap<String, Object>,
}

pub(crate) fn upstream(repo: &HistoryRepo, commit: Option<&str>) -> Result<Upstream> {
    let mut files = BTreeMap::new();
    if let Some(commit) = commit {
        for entry in repo.ls_tree(commit)? {
            if let Some((mode, oid)) = repo.object_at(commit, &entry.path)? {
                files.insert(entry.path, (mode, oid));
            }
        }
    }
    Ok(Upstream {
        commit: commit.map(str::to_string),
        files,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ConflictKind {
    /// Both sides changed the same content and the merge is not clean.
    SameHunk,
    /// Upstream deleted a file this machine changed.
    DeleteModify,
    /// This machine deleted a file upstream changed.
    ModifyDelete,
    /// A file became a symlink (or the reverse) on one side while the
    /// other side changed it.
    TypeChange,
    /// Both sides have the file, with no common base, and they differ.
    NeedsAdoption,
    /// Both sides changed a file that is not text.
    Binary,
    /// A manual-save entry has live edits that are not saved.
    UnsavedEdits,
}

impl ConflictKind {
    pub(crate) fn describe(self) -> &'static str {
        match self {
            Self::SameHunk => "both sides changed the same lines",
            Self::DeleteModify => "deleted upstream, changed here",
            Self::ModifyDelete => "deleted here, changed upstream",
            Self::TypeChange => "changed type on one side and content on the other",
            Self::NeedsAdoption => "needs adoption: both sides have a version and no common base",
            Self::Binary => "both sides changed a binary file",
            Self::UnsavedEdits => "unsaved edits: save or discard them first",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Conflict {
    pub branch_path: String,
    pub kind: ConflictKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
}

/// What one path needs after reconciliation.
#[derive(Clone, Debug, Default)]
pub(crate) struct PathPlan {
    pub branch_path: String,
    /// `Some(None)` publishes a deletion.
    pub publish: Option<Option<Object>>,
    /// `Some(None)` applies a deletion.
    pub apply: Option<Option<Object>>,
    pub conflict: Option<Conflict>,
    /// The record after publication succeeds (application updates
    /// `applied` and `acknowledged` when it is written).
    pub next: SyncRecord,
}

impl PathPlan {
    pub(crate) fn is_noop(&self) -> bool {
        self.publish.is_none() && self.apply.is_none() && self.conflict.is_none()
    }
}

fn oid(object: Option<&Object>) -> Option<String> {
    object.map(|(_, oid)| oid.clone())
}

fn kind(object: Option<&Object>) -> Option<&str> {
    object.map(|(mode, _)| if mode == "120000" { "link" } else { "file" })
}

/// Runs the table for every path of `shared` (S), `upstream` (T), and the
/// recorded state. `unsaved` names manual-save paths whose live file
/// differs from S: an application there is held.
pub(crate) fn reconcile(
    repo: &HistoryRepo,
    shared: &BTreeMap<String, Object>,
    upstream: &Upstream,
    state: &SyncState,
    unsaved: &BTreeSet<String>,
) -> Result<Vec<PathPlan>> {
    let mut paths: BTreeSet<&String> = BTreeSet::new();
    paths.extend(shared.keys());
    paths.extend(upstream.files.keys());
    paths.extend(state.keys());
    let mut plans = vec![];
    for branch_path in paths {
        if branch_path == super::layout::MARKER_PATH || branch_path.starts_with(".mise-history/") {
            continue;
        }
        let s = shared.get(branch_path);
        let t = upstream.files.get(branch_path);
        let record = state.get(branch_path).cloned().unwrap_or_default();
        let mut plan = PathPlan {
            branch_path: branch_path.clone(),
            next: record.clone(),
            ..Default::default()
        };
        plan.next.upstream_commit = upstream.commit.clone();
        let (s_oid, t_oid) = (oid(s), oid(t));
        let a = record.acknowledged.clone();
        let u = record.reconciled.clone();

        // private upstream content is never applied here, and never
        // published from here (it is not in `shared` by construction)
        if s.is_none() && t.is_some() && super::privacy::is_private_branch_path(branch_path) {
            plan.next.reconciled = t_oid;
            plans.push(plan);
            continue;
        }

        if record.is_new() {
            match (s, t) {
                (Some(_), None) => {
                    plan.publish = Some(s.cloned());
                    plan.next.acknowledged = s_oid.clone();
                    plan.next.reconciled = s_oid.clone();
                    plan.next.applied = s_oid;
                }
                (None, Some(_)) => {
                    plan.apply = Some(t.cloned());
                    plan.next.reconciled = t_oid;
                }
                (Some(_), Some(_)) if s_oid == t_oid => {
                    plan.next.acknowledged = s_oid.clone();
                    plan.next.reconciled = s_oid.clone();
                    plan.next.applied = s_oid;
                }
                (Some(_), Some(_)) => {
                    plan.conflict = Some(Conflict {
                        branch_path: branch_path.clone(),
                        kind: ConflictKind::NeedsAdoption,
                        local: s_oid,
                        remote: t_oid,
                        base: None,
                    });
                }
                (None, None) => {}
            }
            if !plan.is_noop() || plan.next != record {
                plans.push(plan);
            }
            continue;
        }

        let local_changed = s_oid != a;
        // an upstream version reconciled but never written here (the merge
        // this machine published, or a version recorded while applying
        // waited) is still incoming: a local change made meanwhile merges
        // with it instead of publishing over it
        let pending = record.reconciled != record.applied && t_oid != s_oid;
        let remote_changed = t_oid != u || pending;
        match (local_changed, remote_changed) {
            (false, false) => {
                // reconciled but never written here: the merge this
                // machine published, or an upstream version recorded while
                // applying waited; it stays pending until it is applied
                if t.is_some() && record.reconciled != record.applied {
                    if unsaved.contains(branch_path) {
                        plan.conflict = Some(Conflict {
                            branch_path: branch_path.clone(),
                            kind: ConflictKind::UnsavedEdits,
                            local: s_oid,
                            remote: t_oid,
                            base: a,
                        });
                    } else {
                        plan.apply = Some(t.cloned());
                    }
                }
            }
            (true, false) => {
                plan.publish = Some(s.cloned());
                plan.next.acknowledged = s_oid.clone();
                plan.next.reconciled = s_oid.clone();
                plan.next.applied = s_oid;
            }
            (false, true) => {
                if unsaved.contains(branch_path) {
                    plan.conflict = Some(Conflict {
                        branch_path: branch_path.clone(),
                        kind: ConflictKind::UnsavedEdits,
                        local: s_oid,
                        remote: t_oid,
                        base: a,
                    });
                } else {
                    plan.apply = Some(t.cloned());
                    plan.next.reconciled = t_oid;
                }
            }
            (true, true) => {
                let merged = merge(repo, a.as_deref(), s, t)?;
                match merged {
                    Merged::Clean(object) => {
                        let object_oid = oid(object.as_ref());
                        if object_oid != t_oid {
                            plan.publish = Some(object.clone());
                        }
                        if unsaved.contains(branch_path) {
                            plan.conflict = Some(Conflict {
                                branch_path: branch_path.clone(),
                                kind: ConflictKind::UnsavedEdits,
                                local: s_oid.clone(),
                                remote: t_oid,
                                base: a,
                            });
                            plan.publish = None;
                        } else if object_oid == s_oid {
                            // the merge is what is saved here already
                            // (upstream's change was a subset): nothing to
                            // write
                            plan.next.acknowledged = s_oid.clone();
                            plan.next.reconciled = s_oid.clone();
                            plan.next.applied = s_oid;
                        } else {
                            plan.apply = Some(object);
                            plan.next.acknowledged = s_oid;
                            plan.next.reconciled = object_oid;
                        }
                    }
                    Merged::Conflict(kind) => {
                        plan.conflict = Some(Conflict {
                            branch_path: branch_path.clone(),
                            kind,
                            local: s_oid,
                            remote: t_oid,
                            base: a,
                        });
                    }
                }
            }
        }
        if !plan.is_noop() || plan.next != record {
            plans.push(plan);
        }
    }
    Ok(plans)
}

enum Merged {
    Clean(Option<Object>),
    Conflict(ConflictKind),
}

/// Three-way merge of two changed sides against the acknowledged base.
fn merge(
    repo: &HistoryRepo,
    base: Option<&str>,
    ours: Option<&Object>,
    theirs: Option<&Object>,
) -> Result<Merged> {
    Ok(match (ours, theirs) {
        (None, None) => Merged::Clean(None),
        (Some(_), None) => Merged::Conflict(ConflictKind::DeleteModify),
        (None, Some(_)) => Merged::Conflict(ConflictKind::ModifyDelete),
        (Some(o), Some(t)) => {
            if o.1 == t.1 && o.0 == t.0 {
                return Ok(Merged::Clean(Some(o.clone())));
            }
            if kind(ours) != kind(theirs) || o.0 == "120000" {
                return Ok(Merged::Conflict(ConflictKind::TypeChange));
            }
            let Some(base) = base else {
                return Ok(Merged::Conflict(ConflictKind::NeedsAdoption));
            };
            let base_bytes = repo.cat_object(base)?;
            let ours_bytes = repo.cat_object(&o.1)?;
            let theirs_bytes = repo.cat_object(&t.1)?;
            match repo.merge3(&base_bytes, &ours_bytes, &theirs_bytes)? {
                Some(merged) => {
                    let oid = repo.hash_blob(&merged)?;
                    Merged::Clean(Some((t.0.clone(), oid)))
                }
                None => {
                    let binary = [&base_bytes, &ours_bytes, &theirs_bytes]
                        .iter()
                        .any(|bytes| bytes.contains(&0));
                    Merged::Conflict(if binary {
                        ConflictKind::Binary
                    } else {
                        ConflictKind::SameHunk
                    })
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(oid: &str) -> Object {
        ("100644".to_string(), oid.to_string())
    }

    fn rec(a: Option<&str>, u: Option<&str>, l: Option<&str>) -> SyncRecord {
        SyncRecord {
            acknowledged: a.map(str::to_string),
            reconciled: u.map(str::to_string),
            applied: l.map(str::to_string),
            upstream_commit: None,
        }
    }

    fn run(
        shared: &[(&str, &str)],
        upstream: &[(&str, &str)],
        state: &[(&str, SyncRecord)],
    ) -> Vec<PathPlan> {
        let tmp = tempfile::tempdir().unwrap();
        let repo = HistoryRepo::open_or_init_in(tmp.path()).unwrap().unwrap();
        run_in(&repo, shared, upstream, state)
    }

    fn run_in(
        repo: &HistoryRepo,
        shared: &[(&str, &str)],
        upstream: &[(&str, &str)],
        state: &[(&str, SyncRecord)],
    ) -> Vec<PathPlan> {
        let shared: BTreeMap<String, Object> = shared
            .iter()
            .map(|(p, o)| (p.to_string(), obj(o)))
            .collect();
        let upstream = Upstream {
            commit: Some("c".into()),
            files: upstream
                .iter()
                .map(|(p, o)| (p.to_string(), obj(o)))
                .collect(),
        };
        let state: SyncState = state
            .iter()
            .map(|(p, r)| (p.to_string(), r.clone()))
            .collect();
        reconcile(repo, &shared, &upstream, &state, &BTreeSet::new()).unwrap()
    }

    #[test]
    fn a_local_edit_over_a_pending_merge_merges_again() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = HistoryRepo::open_or_init_in(tmp.path()).unwrap().unwrap();
        let base = repo.hash_blob(b"a\nb\nc\n").unwrap();
        let local = repo.hash_blob(b"a\nb\nc\nlocal\n").unwrap();
        let merged = repo.hash_blob(b"remote\na\nb\nc\nlocal\n").unwrap();
        let edited = repo.hash_blob(b"a\nb\nc\nlocal\nmore\n").unwrap();
        // this machine published the merge of its edit with the other
        // side's; the merge waits to be pulled (applied is still the base)
        let record = rec(Some(&local), Some(&merged), Some(&base));
        // a further local edit meanwhile: the other side's line must reach
        // the branch again, not be published over
        let plans = run_in(
            &repo,
            &[("tracked/home/.zshrc", &edited)],
            &[("tracked/home/.zshrc", &merged)],
            &[("tracked/home/.zshrc", record.clone())],
        );
        let plan = &plans[0];
        assert!(plan.conflict.is_none(), "{:?}", plan.conflict);
        let published = plan.publish.clone().flatten().expect("published");
        let bytes = repo.cat_object(&published.1).unwrap();
        assert_eq!(bytes, b"remote\na\nb\nc\nlocal\nmore\n");
        assert_eq!(plan.apply, Some(Some(published.clone())));
        assert_eq!(plan.next.acknowledged.as_deref(), Some(edited.as_str()));
        assert_eq!(plan.next.reconciled.as_deref(), Some(published.1.as_str()));

        // an upstream deletion waiting to be pulled, edited meanwhile: a
        // decision, not a resurrection
        let plans = run_in(
            &repo,
            &[("tracked/home/.zshrc", &edited)],
            &[],
            &[("tracked/home/.zshrc", rec(Some(&local), None, Some(&local)))],
        );
        assert_eq!(
            plans[0].conflict.as_ref().map(|c| c.kind),
            Some(ConflictKind::DeleteModify)
        );
    }

    #[test]
    fn rows_one_two_three() {
        // row 1: nothing changed on either side
        let plans = run(
            &[("config.toml", "x")],
            &[("config.toml", "x")],
            &[("config.toml", rec(Some("x"), Some("x"), Some("x")))],
        );
        assert!(plans.iter().all(|p| p.is_noop()));

        // row 2: local change publishes
        let plans = run(
            &[("config.toml", "y")],
            &[("config.toml", "x")],
            &[("config.toml", rec(Some("x"), Some("x"), Some("x")))],
        );
        let plan = &plans[0];
        assert_eq!(plan.publish, Some(Some(obj("y"))));
        assert!(plan.apply.is_none());
        assert_eq!(plan.next.acknowledged.as_deref(), Some("y"));
        assert_eq!(plan.next.reconciled.as_deref(), Some("y"));

        // row 3: upstream change applies
        let plans = run(
            &[("config.toml", "x")],
            &[("config.toml", "z")],
            &[("config.toml", rec(Some("x"), Some("x"), Some("x")))],
        );
        let plan = &plans[0];
        assert!(plan.publish.is_none());
        assert_eq!(plan.apply, Some(Some(obj("z"))));
        assert_eq!(plan.next.reconciled.as_deref(), Some("z"));
    }

    #[test]
    fn adoption_and_deletions() {
        // local only: published and adopted
        let plans = run(&[("tracked/home/.zshrc", "s")], &[], &[]);
        assert_eq!(plans[0].publish, Some(Some(obj("s"))));
        assert_eq!(plans[0].next.acknowledged.as_deref(), Some("s"));

        // remote only: applied
        let plans = run(&[], &[("tracked/home/.vimrc", "t")], &[]);
        assert_eq!(plans[0].apply, Some(Some(obj("t"))));

        // both, different, no base: needs adoption
        let plans = run(&[("config.toml", "a")], &[("config.toml", "b")], &[]);
        assert_eq!(
            plans[0].conflict.as_ref().map(|c| c.kind),
            Some(ConflictKind::NeedsAdoption)
        );

        // local deletion of an unchanged upstream file publishes the deletion
        let plans = run(
            &[],
            &[("config.toml", "x")],
            &[("config.toml", rec(Some("x"), Some("x"), Some("x")))],
        );
        assert_eq!(plans[0].publish, Some(None));

        // upstream deletion while local changed: delete/modify
        let plans = run(
            &[("config.toml", "y")],
            &[],
            &[("config.toml", rec(Some("x"), Some("x"), Some("x")))],
        );
        assert_eq!(
            plans[0].conflict.as_ref().map(|c| c.kind),
            Some(ConflictKind::DeleteModify)
        );
    }
}
