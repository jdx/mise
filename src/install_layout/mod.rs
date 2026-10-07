//! The identity-based install layout (`experimental`): every installation lives
//! at `installs/<label>-<hash>/`, described by a receipt, with compatibility
//! links at the familiar `installs/<short>/<version>` paths.
//!
//! See jdx/mise#13678 for the design. The pieces:
//!
//! * [`identity`] hashes what an installation answers (backend, version,
//!   platform, install-affecting options, pinned inputs) into a stable name.
//! * [`label`] derives the readable half of the directory name.
//! * [`record`] defines the on-disk receipt, catalog record and selection.
//! * [`resolver`] connects a tool version to all of the above: it computes its
//!   identity, locates or allocates its directory, and keeps links, receipt and
//!   selection in step.
//! * [`snapshots`] remembers what a config's templated versions rendered to,
//!   for `mise prune`.
//! * [`catalog`] allocates collision-free directory names under
//!   `installs/.mise/` and remembers unlocked selections.
//!
//! Nothing here reads mise settings or backends; callers hand in the inputs.

pub(crate) mod catalog;
pub(crate) mod identity;
pub(crate) mod label;
pub(crate) mod record;
pub mod resolver;
pub mod snapshots;
