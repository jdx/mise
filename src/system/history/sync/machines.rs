//! Other machines' recovery refs as fetched into `refs/machines/<id>/<uuid>`:
//! listed for `mise bootstrap dotfiles machines`, and readable as checkpoint entries
//! so `rollback --to <machine>/<ref>` recovers their files. Their operation
//! journals are data only: never inverted, replayed, or executed.
//!
//! An encrypted ref is listed from its header alone (machine, time, count);
//! its content is decrypted only when a checkpoint of that machine is
//! resolved, once, into a local plaintext wrapper under
//! `refs/machines-plain/<id>/<remote commit>`.

use std::collections::{BTreeMap, BTreeSet};

use eyre::{Result, bail};
use serde::Serialize;

use super::encrypted::{self, ReadError};
use super::network::{MACHINES_PREFIX, PLAIN_PREFIX};
use crate::agecrypt::DecryptError;
use crate::system::history::shadow::HistoryRepo;
use crate::system::history::store::{Checkpoint, Entry, Machine};

#[derive(Clone, Debug, Serialize)]
pub(crate) struct MachineInfo {
    pub id: String,
    pub name: String,
    pub checkpoints: usize,
    /// How many of the checkpoints are encrypted (for this machine: all of
    /// them when the connection encrypts, else none).
    pub encrypted: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest: Option<String>,
    /// This machine (its refs are local checkpoints, not fetched ones).
    pub this: bool,
}

/// One fetched ref, read as far as its layout allows without a key.
#[derive(Clone, Debug)]
struct Fetched {
    uuid: String,
    commit: String,
    machine_name: String,
    created_at: String,
    /// The record, for a plaintext backup; an encrypted one is read when
    /// it is needed.
    plain: Option<Checkpoint>,
}

fn fetched(repo: &HistoryRepo) -> Result<BTreeMap<String, Vec<Fetched>>> {
    let mut by_machine: BTreeMap<String, Vec<Fetched>> = BTreeMap::new();
    let mut live: BTreeSet<String> = BTreeSet::new();
    for (name, commit) in repo.list_refs(MACHINES_PREFIX)? {
        let Some(rest) = name.strip_prefix(MACHINES_PREFIX) else {
            continue;
        };
        let Some((machine_id, uuid)) = rest.split_once('/') else {
            continue;
        };
        live.insert(commit.clone());
        let item = match encrypted::header_of(repo, &commit) {
            Ok(Some(header)) => Fetched {
                uuid: uuid.to_string(),
                commit,
                machine_name: header.machine.name,
                created_at: header.created_at,
                plain: None,
            },
            Ok(None) => match repo.read_meta(&commit) {
                Ok(mut checkpoint) => {
                    // the snapshot is the wrapper commit's own `snapshot/`
                    // tree: a pushed wrapper may have been rebuilt with
                    // backup-excluded files removed, so the tree id its
                    // metadata names is not the one here
                    checkpoint.tree.snapshot =
                        repo.object_at(&commit, "snapshot")?.map(|(_, oid)| oid);
                    Fetched {
                        uuid: uuid.to_string(),
                        commit,
                        machine_name: checkpoint.machine.name.clone(),
                        created_at: checkpoint.created_at.clone(),
                        plain: Some(checkpoint),
                    }
                }
                Err(err) => {
                    debug!("history: skipping {name}: {err}");
                    continue;
                }
            },
            Err(err) => {
                debug!("history: skipping {name}: {err}");
                continue;
            }
        };
        by_machine
            .entry(machine_id.to_string())
            .or_default()
            .push(item);
    }
    for entries in by_machine.values_mut() {
        entries.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    }
    prune_plain(repo, &live)?;
    Ok(by_machine)
}

/// Drops the decrypted copies of refs the origin no longer has (or that
/// were uploaded again under another scheme).
fn prune_plain(repo: &HistoryRepo, live: &BTreeSet<String>) -> Result<()> {
    for (name, _) in repo.list_refs(PLAIN_PREFIX)? {
        let Some(rest) = name.strip_prefix(PLAIN_PREFIX) else {
            continue;
        };
        let Some((_, remote_commit)) = rest.split_once('/') else {
            continue;
        };
        if !live.contains(remote_commit) {
            repo.delete_ref(&name)?;
        }
    }
    Ok(())
}

/// Every machine with recovery refs, this one first.
pub(crate) fn list(
    repo: &HistoryRepo,
    this: &Machine,
    local: &[Entry],
    this_encrypts: bool,
) -> Result<Vec<MachineInfo>> {
    let mut out = vec![MachineInfo {
        id: this.id.clone(),
        name: this.name.clone(),
        checkpoints: local.len(),
        encrypted: if this_encrypts { local.len() } else { 0 },
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
            .map(|entry| entry.machine_name.clone())
            .unwrap_or_else(|| id.clone());
        out.push(MachineInfo {
            id,
            name,
            checkpoints: entries.len(),
            encrypted: entries.iter().filter(|entry| entry.plain.is_none()).count(),
            latest: entries.last().map(|entry| entry.created_at.clone()),
            this: false,
        });
    }
    Ok(out)
}

/// Whether any other machine's fetched backups are encrypted: a fresh
/// machine set up from the repository follows suit.
pub(crate) fn any_encrypted(repo: &HistoryRepo, except_machine: &str) -> Result<bool> {
    Ok(fetched(repo)?
        .iter()
        .filter(|(id, _)| id.as_str() != except_machine)
        .any(|(_, entries)| entries.iter().any(|entry| entry.plain.is_none())))
}

/// A machine's fetched checkpoints, oldest first, numbered from 1. An
/// encrypted checkpoint is decrypted here (once; the plaintext copy is
/// kept locally), so every entry returned can be rolled back to.
pub(crate) async fn entries(repo: &HistoryRepo, machine: &str) -> Result<(Machine, Vec<Entry>)> {
    let fetched = fetched(repo)?;
    let matching: Vec<(&String, &Vec<Fetched>)> = fetched
        .iter()
        .filter(|(id, entries)| {
            *id == machine
                || entries
                    .last()
                    .is_some_and(|entry| entry.machine_name == machine)
        })
        .collect();
    let (id, items) = match matching.as_slice() {
        [] => bail!(
            "no machine {machine:?} has recovery refs here; `mise bootstrap dotfiles machines` lists them (after `mise bootstrap dotfiles sync`)"
        ),
        [(id, items)] => (*id, *items),
        many => bail!(
            "{} machines are named {machine:?}; use an id: {}",
            many.len(),
            many.iter()
                .map(|(id, _)| id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };
    let name = items
        .last()
        .map(|entry| entry.machine_name.clone())
        .unwrap_or_else(|| id.clone());
    let mut entries = vec![];
    for (index, item) in items.iter().enumerate() {
        let (commit, checkpoint) = match &item.plain {
            Some(checkpoint) => (item.commit.clone(), checkpoint.clone()),
            None => {
                let plain = materialized(repo, id, item, &name).await?;
                let mut checkpoint = repo.read_meta(&plain)?;
                checkpoint.tree.snapshot = repo.object_at(&plain, "snapshot")?.map(|(_, oid)| oid);
                (plain, checkpoint)
            }
        };
        entries.push(Entry {
            id: index as u64 + 1,
            commit,
            checkpoint,
        });
    }
    Ok((
        Machine {
            id: id.clone(),
            name,
        },
        entries,
    ))
}

/// The local plaintext wrapper of an encrypted ref: the one kept from an
/// earlier decryption, else decrypted now and kept.
async fn materialized(
    repo: &HistoryRepo,
    machine_id: &str,
    item: &Fetched,
    machine_name: &str,
) -> Result<String> {
    let plain_ref = format!("{PLAIN_PREFIX}{machine_id}/{}", item.commit);
    if let Some(commit) = repo.ref_oid(&plain_ref)? {
        return Ok(commit);
    }
    match encrypted::materialize(repo, &item.commit).await {
        Ok(commit) => {
            if let Err(err) = repo.update_ref(&plain_ref, &commit, None) {
                debug!("history: keeping the decrypted backup {plain_ref}: {err}");
            }
            Ok(commit)
        }
        Err(ReadError::Decrypt(DecryptError::NoIdentities)) => bail!(
            "checkpoint {} of {machine_name} is encrypted and this machine has no age identity; put the identity in ~/.config/mise/age.txt, settings.age.key_file, or MISE_AGE_KEY",
            item.uuid
        ),
        Err(ReadError::Decrypt(err)) => bail!(
            "checkpoint {} of {machine_name} is encrypted and none of this machine's identities can decrypt it: {err}",
            item.uuid
        ),
        Err(ReadError::Corrupt(reason)) => bail!(
            "checkpoint {} of {machine_name}: the encrypted backup is damaged: {reason}",
            item.uuid
        ),
        Err(ReadError::NotEncrypted) => bail!(
            "checkpoint {} of {machine_name} is neither a plaintext nor an encrypted backup",
            item.uuid
        ),
        Err(ReadError::Other(err)) => Err(err),
    }
}
