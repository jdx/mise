//! Bootstrap generations: a durable record of every mutating bootstrap run.
//!
//! A generation captures what a run was (`command`, `argv`), a snapshot of
//! the global config directory and `dotfiles.root` taken before and after
//! the run in a mise-owned bare git repository (the "shadow" repo), the
//! global lockfile, and a journal of what the run changed. Together they
//! give a named machine state that `mise bootstrap generations` can list
//! and show.
//!
//! Layout under `$MISE_STATE_DIR/bootstrap/` (created `0700`):
//!
//! ```text
//! generations.git/        bare shadow repository; refs/generations/<id>/{before,after}
//! generations/NNNNNN.json one record per generation
//! ```
//!
//! Recording never fails a bootstrap: any store problem degrades to a
//! warning and the run proceeds unrecorded.

pub(crate) mod journal;
pub(crate) mod scope;
pub(crate) mod shadow;
pub(crate) mod store;

pub(crate) use scope::GenerationScope;
