//! Configuration history: automatic checkpoints of tracked files, the
//! operations that change them, and the recovery they make possible.
//!
//! - [`store`] — the on-disk layout, the checkpoint record, and the index.
//! - [`shadow`] — the bare repository mise owns.
//! - [`tracked`] — what a capture covers and under which policies.
//! - [`checkpoint`] — the one entry point every capture goes through.
//! - [`scope`] — the operation a mutating command records into.
//! - [`journal`] — the write-ahead journal of what an operation changed.
//! - [`retention`] — which checkpoints stay.

pub(crate) mod checkpoint;
pub(crate) mod config;
pub(crate) mod journal;
pub(crate) mod replay;
pub(crate) mod retention;
pub(crate) mod scope;
pub(crate) mod select;
pub(crate) mod shadow;
pub(crate) mod store;
pub(crate) mod tracked;

pub(crate) use scope::OperationScope;

/// Tracking and its recovery/synchronization interfaces are experimental.
pub(crate) fn ensure_experimental() -> eyre::Result<()> {
    crate::config::Settings::get().ensure_experimental("dotfile tracking")
}
