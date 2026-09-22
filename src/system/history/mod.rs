//! Configuration history: automatic checkpoints of tracked files, the
//! operations that change them, and the recovery they make possible.
//!
//! - [`store`] — the on-disk layout, the checkpoint record, and the index.
//! - [`shadow`] — the bare repository mise owns.
//! - [`tracked`] — what a capture covers and under which policies.
//! - [`checkpoint`] — the one entry point every capture goes through.
//! - [`scope`] — the operation a mutating command records into.
//! - [`journal`] — the write-ahead journal of what an operation changed.

pub(crate) mod checkpoint;
pub(crate) mod config;
pub(crate) mod describe_command;
pub(crate) mod enrollment;
pub(crate) mod health;
pub(crate) mod journal;
pub(crate) mod manifest;
pub(crate) mod notify;
pub(crate) mod recovery;
pub(crate) mod replay;
pub(crate) mod scope;
pub(crate) mod select;
pub(crate) mod shadow;
pub(crate) mod store;
pub(crate) mod sync;
pub(crate) mod tracked;
pub(crate) mod watch;

pub(crate) use scope::OperationScope;

use eyre::Result;

use checkpoint::Store;
use store::Entry;
use tracked::TrackedSet;

/// Opens the store and lists its checkpoints, oldest first. Reading never
/// changes the store: an operation that died is closed by the next one
/// that takes the operation lock (a save, a bootstrap, the watcher).
pub(crate) async fn open() -> Result<(Store, TrackedSet, Vec<Entry>)> {
    let store = Store::open()?;
    let tracked = TrackedSet::effective().await?;
    let entries = store.list()?;
    Ok((store, tracked, entries))
}

/// Resolves a checkpoint reference. With a path scope, `latest[~N]` counts
/// only the checkpoints where that path changed.
pub(crate) fn resolve(spec: &str, entries: &[Entry], path: Option<&str>) -> Result<Entry> {
    let scoped: Vec<Entry> = match path {
        Some(path) => entries
            .iter()
            .filter(|entry| entry.checkpoint.changes.touches(path))
            .cloned()
            .collect(),
        None => entries.to_vec(),
    };
    let id = if spec.starts_with("latest") {
        store::resolve_ref(spec, &scoped)?
    } else {
        store::resolve_ref(spec, entries)?
    };
    entries
        .iter()
        .find(|entry| entry.id == id)
        .cloned()
        .ok_or_else(|| eyre::eyre!("no history checkpoint {id}"))
}

pub(crate) fn short(oid: &str) -> String {
    oid.chars().take(7).collect()
}
