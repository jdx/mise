//! Retention: which checkpoints stay.
//!
//! `history.keep.count` caps the number of checkpoints and
//! `history.keep.age` their age; both apply to every trigger. An operation
//! pair counts as two and is kept or expired together. Candidates go oldest
//! first, automatic captures before explicit saves, before operation pairs.
//! Protected above the caps: pinned checkpoints and the pair of an
//! operation still in progress. The newest checkpoint always survives.

use std::collections::BTreeSet;

use eyre::Result;

use super::checkpoint::Store;
use super::store::{Entry, OperationStatus, Trigger};
use crate::config::Settings;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Policy {
    /// 0 keeps everything.
    pub count: usize,
    /// 0 keeps everything.
    pub age_secs: u64,
}

impl Policy {
    pub(crate) fn from_settings() -> Self {
        let settings = Settings::get();
        let age_secs = crate::duration::parse_duration(&settings.history.keep.age)
            .map(|age| age.as_secs())
            .unwrap_or_else(|err| {
                warn!("history.keep.age: {err}");
                0
            });
        Self {
            count: settings.history.keep.count,
            age_secs,
        }
    }
}

/// The ids to delete, oldest first, given every checkpoint (oldest first)
/// and the current time as unix seconds.
pub(crate) fn plan(entries: &[Entry], now: i64, policy: Policy) -> Vec<u64> {
    let protected = protected(entries);
    let pair_of = pairs(entries);
    let mut drop: BTreeSet<u64> = BTreeSet::new();
    let age_of = |entry: &Entry| -> i64 {
        chrono::DateTime::parse_from_rfc3339(&entry.checkpoint.created_at)
            .map(|created| now - created.timestamp())
            .unwrap_or(0)
    };
    if policy.age_secs > 0 {
        for entry in entries {
            if !protected.contains(&entry.id) && age_of(entry) > policy.age_secs as i64 {
                drop.insert(entry.id);
            }
        }
    }
    if policy.count > 0 {
        let surviving = entries.len().saturating_sub(drop.len());
        if surviving > policy.count {
            let mut excess = surviving - policy.count;
            // automatic captures first, then saves, then operation pairs
            let priority = |entry: &Entry| match entry.checkpoint.trigger {
                Trigger::Edit => 0,
                Trigger::Save | Trigger::Agent | Trigger::Baseline | Trigger::Update => 1,
                _ => 2,
            };
            let mut candidates: Vec<&Entry> = entries
                .iter()
                .filter(|entry| !protected.contains(&entry.id) && !drop.contains(&entry.id))
                .collect();
            candidates.sort_by_key(|entry| (priority(entry), entry.id));
            for entry in candidates {
                if excess == 0 {
                    break;
                }
                if drop.insert(entry.id) {
                    excess -= 1;
                    // a pair goes together, and both count toward the cap
                    if let Some(other) = pair_of.get(&entry.id)
                        && !protected.contains(other)
                        && drop.insert(*other)
                    {
                        excess = excess.saturating_sub(1);
                    }
                }
            }
        }
    }
    // pairs go together
    for id in drop.clone() {
        if let Some(other) = pair_of.get(&id) {
            drop.insert(*other);
        }
    }
    drop.retain(|id| !protected.contains(id));
    drop.into_iter().collect()
}

fn protected(entries: &[Entry]) -> BTreeSet<u64> {
    let mut protected = BTreeSet::new();
    if let Some(newest) = entries.last() {
        protected.insert(newest.id);
    }
    for entry in entries {
        let checkpoint = &entry.checkpoint;
        if checkpoint.pinned {
            protected.insert(entry.id);
        }
        if checkpoint.status() == Some(OperationStatus::Pending) {
            protected.insert(entry.id);
        }
    }
    let pair_of = pairs(entries);
    for id in protected.clone() {
        if let Some(other) = pair_of.get(&id) {
            protected.insert(*other);
        }
    }
    protected
}

/// Each half of an operation pair mapped to the other.
fn pairs(entries: &[Entry]) -> std::collections::BTreeMap<u64, u64> {
    let mut pairs = std::collections::BTreeMap::new();
    for entry in entries {
        if let Some(before) = entry
            .checkpoint
            .operation
            .as_ref()
            .and_then(|operation| operation.before.as_deref())
            && let Some(other) = entries
                .iter()
                .find(|candidate| candidate.checkpoint.uuid == before)
        {
            pairs.insert(entry.id, other.id);
            pairs.insert(other.id, entry.id);
        }
    }
    pairs
}

/// Applies the policy to the store (the caller holds the store lock).
pub(crate) fn prune(store: &Store) -> Result<Vec<u64>> {
    let policy = Policy::from_settings();
    let entries = store.list()?;
    let now = chrono::Utc::now().timestamp();
    let ids = plan(&entries, now, policy);
    for id in &ids {
        store.remove(*id)?;
    }
    if !ids.is_empty() {
        debug!("history: pruned checkpoints {ids:?}");
        if let Some(repo) = store.repo() {
            // old promoted versions stay reachable through the chain's
            // ancestry otherwise, however many checkpoints were pruned
            if let Err(err) = repo.compact_promotions() {
                warn!("history: could not compact the promotion chain: {err:#}");
            }
            if let Err(err) = repo.gc() {
                warn!("history: gc failed: {err:#}");
            }
        }
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::history::checkpoint::test_checkpoint;
    use crate::system::history::store::{Operation, OperationKind};

    fn entry(id: u64, trigger: Trigger, age_secs: i64) -> Entry {
        let mut checkpoint = test_checkpoint(&format!("uuid-{id}"), None);
        checkpoint.trigger = trigger;
        checkpoint.created_at = (chrono::Utc::now() - chrono::Duration::seconds(age_secs))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        Entry {
            id,
            commit: String::new(),
            checkpoint,
        }
    }

    fn pair(before_id: u64, outcome_id: u64, age_secs: i64) -> (Entry, Entry) {
        let before = entry(before_id, Trigger::BootstrapBefore, age_secs);
        let mut outcome = entry(outcome_id, Trigger::Bootstrap, age_secs);
        outcome.checkpoint.operation = Some(Operation {
            kind: OperationKind::Bootstrap,
            status: OperationStatus::Completed,
            command: "bootstrap".into(),
            argv: vec![],
            cwd: Default::default(),
            user: None,
            finished_at: None,
            error: None,
            before: Some(before.checkpoint.uuid.clone()),
            to: None,
            undoes: None,
            applied: None,
            affected: vec![],
            sources: vec![],
            directories: vec![],
            message: None,
            journal: vec![],
        });
        (before, outcome)
    }

    #[test]
    fn count_cap_drops_oldest_automatic_first() {
        let entries = vec![
            entry(1, Trigger::Save, 100),
            entry(2, Trigger::Edit, 90),
            entry(3, Trigger::Edit, 80),
            entry(4, Trigger::Save, 70),
            entry(5, Trigger::Edit, 60),
        ];
        let now = chrono::Utc::now().timestamp();
        let policy = Policy {
            count: 3,
            age_secs: 0,
        };
        assert_eq!(plan(&entries, now, policy), vec![2, 3]);
        let unlimited = Policy {
            count: 0,
            age_secs: 0,
        };
        assert!(plan(&entries, now, unlimited).is_empty());
    }

    #[test]
    fn age_cap_and_pairs_go_together() {
        let (before, outcome) = pair(1, 2, 10_000);
        let entries = vec![
            before,
            outcome,
            entry(3, Trigger::Edit, 5),
            entry(4, Trigger::Edit, 1),
        ];
        let now = chrono::Utc::now().timestamp();
        let policy = Policy {
            count: 0,
            age_secs: 3600,
        };
        assert_eq!(plan(&entries, now, policy), vec![1, 2]);
        // a save goes before an operation pair, and dropping one half of a
        // pair for the count cap drops the other
        let (before, outcome) = pair(1, 2, 100);
        let entries = vec![
            before,
            outcome,
            entry(3, Trigger::Save, 50),
            entry(4, Trigger::Save, 1),
        ];
        let policy = Policy {
            count: 3,
            age_secs: 0,
        };
        assert_eq!(plan(&entries, now, policy), vec![3]);
        let policy = Policy {
            count: 2,
            age_secs: 0,
        };
        assert_eq!(plan(&entries, now, policy), vec![1, 2, 3]);
    }

    #[test]
    fn pinned_newest_and_pending_survive() {
        let mut pinned = entry(1, Trigger::Edit, 1000);
        pinned.checkpoint.pinned = true;
        let (before, mut pending) = pair(2, 3, 900);
        pending.checkpoint.operation.as_mut().unwrap().status = OperationStatus::Pending;
        let entries = vec![
            pinned,
            before,
            pending,
            entry(4, Trigger::Edit, 800),
            entry(5, Trigger::Edit, 1),
        ];
        let now = chrono::Utc::now().timestamp();
        let policy = Policy {
            count: 1,
            age_secs: 60,
        };
        assert_eq!(plan(&entries, now, policy), vec![4]);
    }
}
