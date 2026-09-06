use std::path::{Path, PathBuf};

use eyre::Result;

use crate::deps::rule::DepsProviderConfig;
use crate::deps::{DepsCommand, DepsProvider, DepsProviderApplicability};

use super::ProviderBase;

/// Deps provider for Poetry (poetry.lock)
#[derive(Debug)]
pub(crate) struct PoetryDepsProvider {
    base: ProviderBase,
}

impl PoetryDepsProvider {
    pub(crate) fn new(project_root: &Path, config: DepsProviderConfig) -> Self {
        Self {
            base: ProviderBase::new("poetry", project_root, config),
        }
    }
}

impl DepsProvider for PoetryDepsProvider {
    fn base(&self) -> &ProviderBase {
        &self.base
    }

    fn sources(&self) -> Vec<PathBuf> {
        let root = self.base.config_root();
        self.base
            .sources(vec![root.join("poetry.lock"), root.join("pyproject.toml")])
    }

    fn outputs(&self) -> Vec<PathBuf> {
        self.base.outputs(vec![])
    }

    fn optional_outputs(&self) -> Vec<PathBuf> {
        // Poetry only writes `.venv` in the project when `virtualenvs.in-project`
        // is enabled; otherwise the venv lives elsewhere. Track as optional so
        // in-project setups detect deletion without breaking the default mode.
        self.base
            .optional_outputs(vec![self.base.config_root().join(".venv")])
    }

    fn install_command(&self) -> Result<DepsCommand> {
        self.base
            .install_command("poetry", &["install"], "poetry install")
    }

    fn applicability(&self) -> DepsProviderApplicability {
        DepsProviderApplicability::require_file(&self.base.config_root().join("poetry.lock"))
    }
}
