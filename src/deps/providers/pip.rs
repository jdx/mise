use std::path::{Path, PathBuf};

use eyre::Result;

use crate::deps::rule::DepsProviderConfig;
use crate::deps::{DepsCommand, DepsProvider, DepsProviderApplicability};

use super::ProviderBase;

/// Deps provider for pip (requirements.txt)
#[derive(Debug)]
pub(crate) struct PipDepsProvider {
    base: ProviderBase,
}

impl PipDepsProvider {
    pub(crate) fn new(project_root: &Path, config: DepsProviderConfig) -> Self {
        Self {
            base: ProviderBase::new("pip", project_root, config),
        }
    }
}

impl DepsProvider for PipDepsProvider {
    fn base(&self) -> &ProviderBase {
        &self.base
    }

    fn sources(&self) -> Vec<PathBuf> {
        self.base
            .sources(vec![self.base.config_root().join("requirements.txt")])
    }

    fn outputs(&self) -> Vec<PathBuf> {
        self.base.outputs(vec![])
    }

    fn optional_outputs(&self) -> Vec<PathBuf> {
        // `pip install` installs into whatever python is on PATH and doesn't
        // create `.venv` itself. Track it as optional so projects that use a
        // local venv detect deletion, without forcing a re-run for projects
        // that don't.
        self.base
            .optional_outputs(vec![self.base.config_root().join(".venv")])
    }

    fn install_command(&self) -> Result<DepsCommand> {
        self.base
            .install_command("pip", &["install", "-r", "requirements.txt"], "pip install")
    }

    fn applicability(&self) -> DepsProviderApplicability {
        DepsProviderApplicability::require_file(&self.base.config_root().join("requirements.txt"))
    }
}
