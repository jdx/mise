//! What core needs from the command-line frontend. `cli` registers these at
//! startup (`cli::register_frontend`), so config, toolset and task code can
//! reach a command's behavior without depending on the `cli` module.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};

use eyre::Result;

use crate::config::Config;
use crate::toolset::ToolVersion;

type LockfilesAfterInstall =
    fn(Arc<Config>, Vec<ToolVersion>) -> Pin<Box<dyn Future<Output = Result<()>> + Send>>;

pub struct Frontend {
    /// `mise lock` for the versions an install just added.
    pub lockfiles_after_install: LockfilesAfterInstall,
    /// Every subcommand name and alias, for "did you mean" suggestions.
    pub subcommand_names: fn() -> Vec<String>,
}

static FRONTEND: OnceLock<Frontend> = OnceLock::new();

pub fn register(frontend: Frontend) {
    let _ = FRONTEND.set(frontend);
}

fn get() -> &'static Frontend {
    FRONTEND
        .get()
        .expect("frontend::register must run before core calls into the CLI")
}

pub(crate) async fn generate_lockfiles_after_install(
    config: Arc<Config>,
    installed: &[ToolVersion],
) -> Result<()> {
    (get().lockfiles_after_install)(config, installed.to_vec()).await
}

pub(crate) fn subcommand_names() -> Vec<String> {
    (get().subcommand_names)()
}
