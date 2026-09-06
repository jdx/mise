//! Other machines' recovery refs as fetched into `refs/machines/<id>/<uuid>`:
//! listed for `mise bootstrap dotfiles machines`, and readable as checkpoint entries
//! so `rollback --to <machine>/<ref>` recovers their files. Their operation
//! journals are data only: never inverted, replayed, or executed.

use std::collections::BTreeMap;

use eyre::{Result, bail};
use serde::Serialize;

use super::network::MACHINES_PREFIX;
use crate::system::history::shadow::HistoryRepo;
use crate::system::history::store::{Entry, Machine};

#[derive(Clone, Debug, Serialize)]
pub(crate) struct MachineInfo {
    pub id: String,
    pub name: String,
    pub checkpoints: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest: Option<String>,
    /// This machine (its refs are local checkpoints, not fetched ones).
    pub this: bool,
}

fn fetched(repo: &HistoryRepo) -> Result<BTreeMap<String, Vec<(String, Entry)>>> {
    let mut by_machine: BTreeMap<String, Vec<(String, Entry)>> = BTreeMap::new();
    for (name, commit) in repo.list_refs(MACHINES_PREFIX)? {
        let Some(rest) = name.strip_prefix(MACHINES_PREFIX) else {
            continue;
        };
        let Some((machine_id, uuid)) = rest.split_once('/') else {
            continue;
        };
        let mut checkpoint = match repo.read_meta(&commit) {
            Ok(checkpoint) => checkpoint,
            Err(err) => {
                debug!("history: skipping {name}: {err}");
                continue;
            }
        };
        // the snapshot is the wrapper commit's own `snapshot/` tree: a
        // pushed wrapper may have been rebuilt with backup-excluded files
        // removed, so the tree id its metadata names is not the one here
        checkpoint.tree.snapshot = repo.object_at(&commit, "snapshot")?.map(|(_, oid)| oid);
        by_machine.entry(machine_id.to_string()).or_default().push((
            uuid.to_string(),
            Entry {
                id: 0,
                commit,
                checkpoint,
            },
        ));
    }
    for entries in by_machine.values_mut() {
        entries.sort_by(|a, b| a.1.checkpoint.created_at.cmp(&b.1.checkpoint.created_at));
        for (index, (_, entry)) in entries.iter_mut().enumerate() {
            entry.id = index as u64 + 1;
        }
    }
    Ok(by_machine)
}

/// Every machine with recovery refs, this one first.
pub(crate) fn list(
    repo: &HistoryRepo,
    this: &Machine,
    local: &[Entry],
) -> Result<Vec<MachineInfo>> {
    let mut out = vec![MachineInfo {
        id: this.id.clone(),
        name: this.name.clone(),
        checkpoints: local.len(),
        latest: local
            .last()
            .map(|entry| entry.checkpoint.created_at.clone()),
        this: true,
    }];
    for (id, entries) in fetched(repo)? {
        if id == this.id {
            continue;
        }
        let name = entries
            .last()
            .map(|(_, entry)| entry.checkpoint.machine.name.clone())
            .unwrap_or_else(|| id.clone());
        out.push(MachineInfo {
            id,
            name,
            checkpoints: entries.len(),
            latest: entries
                .last()
                .map(|(_, entry)| entry.checkpoint.created_at.clone()),
            this: false,
        });
    }
    Ok(out)
}

/// A machine's fetched checkpoints, oldest first, numbered from 1.
pub(crate) fn entries(repo: &HistoryRepo, machine: &str) -> Result<(Machine, Vec<Entry>)> {
    let fetched = fetched(repo)?;
    let matching: Vec<(&String, &Vec<(String, Entry)>)> = fetched
        .iter()
        .filter(|(id, entries)| {
            *id == machine
                || entries
                    .last()
                    .is_some_and(|(_, entry)| entry.checkpoint.machine.name == machine)
        })
        .collect();
    match matching.as_slice() {
        [] => bail!(
            "no machine {machine:?} has recovery refs here; `mise bootstrap dotfiles machines` lists them (after `mise bootstrap dotfiles sync`)"
        ),
        [(id, entries)] => {
            let name = entries
                .last()
                .map(|(_, entry)| entry.checkpoint.machine.name.clone())
                .unwrap_or_else(|| (*id).clone());
            Ok((
                Machine {
                    id: (*id).clone(),
                    name,
                },
                entries.iter().map(|(_, entry)| entry.clone()).collect(),
            ))
        }
        many => bail!(
            "{} machines are named {machine:?}; use an id: {}",
            many.len(),
            many.iter()
                .map(|(id, _)| id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}
