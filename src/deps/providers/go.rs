use std::path::{Path, PathBuf};

use eyre::Result;

use crate::deps::rule::DepsProviderConfig;
use crate::deps::{DepsCommand, DepsProvider, DepsProviderApplicability};

use super::ProviderBase;

/// Deps provider for Go (go.sum)
#[derive(Debug)]
pub(crate) struct GoDepsProvider {
    base: ProviderBase,
}

impl GoDepsProvider {
    pub(crate) fn new(project_root: &Path, config: DepsProviderConfig) -> Self {
        Self {
            base: ProviderBase::new("go", project_root, config),
        }
    }
}

impl DepsProvider for GoDepsProvider {
    fn base(&self) -> &ProviderBase {
        &self.base
    }

    fn sources(&self) -> Vec<PathBuf> {
        let root = self.base.config_root();
        // Both go.mod and go.sum count as sources: go.mod declares the modules,
        // go.sum pins their checksums. A `go mod tidy` that updates only go.sum
        // should still trigger a re-run.
        self.base
            .sources(vec![root.join("go.mod"), root.join("go.sum")])
    }

    fn outputs(&self) -> Vec<PathBuf> {
        self.base.outputs(vec![])
    }

    fn optional_outputs(&self) -> Vec<PathBuf> {
        // Go downloads modules to GOPATH/pkg/mod by default. Track `vendor/` as
        // optional so vendored projects detect deletion without forcing a
        // re-run for non-vendored projects.
        self.base
            .optional_outputs(vec![self.base.config_root().join("vendor")])
    }

    fn install_command(&self) -> Result<DepsCommand> {
        // Use `go mod vendor` if vendor/ exists, otherwise `go mod download`
        let vendor = self.base.config_root().join("vendor");
        let subcommand = if vendor.exists() {
            "vendor"
        } else {
            "download"
        };

        self.base
            .install_command("go", &["mod", subcommand], &format!("go mod {subcommand}"))
    }

    fn applicability(&self) -> DepsProviderApplicability {
        // Check for go.mod (the source/lockfile), not go.sum (which may be an output)
        DepsProviderApplicability::require_file(&self.base.config_root().join("go.mod"))
    }
}
