use std::path::{Path, PathBuf};

use eyre::Result;

use crate::deps::rule::DepsProviderConfig;
use crate::deps::{DepsCommand, DepsProvider};

use super::ProviderBase;

/// Deps provider for `dart pub` and `flutter pub` (pubspec.yaml + pubspec.lock).
///
/// Both ecosystems share the same `pub` package manager and only differ in the
/// executable they invoke, so they're handled by a single provider type
/// parameterised on the program name (also used as the provider id).
#[derive(Debug)]
pub struct DartDepsProvider {
    base: ProviderBase,
}

impl DartDepsProvider {
    pub fn new(program: &str, project_root: &Path, config: DepsProviderConfig) -> Self {
        Self {
            base: ProviderBase::new(program, project_root, config),
        }
    }

    fn program(&self) -> &str {
        &self.base.id
    }
}

impl DepsProvider for DartDepsProvider {
    fn base(&self) -> &ProviderBase {
        &self.base
    }

    fn sources(&self) -> Vec<PathBuf> {
        let root = self.base.config_root();
        vec![root.join("pubspec.yaml"), root.join("pubspec.lock")]
    }

    fn outputs(&self) -> Vec<PathBuf> {
        vec![
            self.base
                .config_root()
                .join(".dart_tool/package_config.json"),
        ]
    }

    fn install_command(&self) -> Result<DepsCommand> {
        if let Some(run) = &self.base.config.run {
            return DepsCommand::from_string(run, &self.base.project_root, &self.base.config);
        }

        let program = self.program().to_string();
        Ok(DepsCommand {
            program: program.clone(),
            args: vec!["pub".to_string(), "get".to_string()],
            env: self.base.config.env.clone(),
            cwd: Some(self.base.config_root()),
            description: self
                .base
                .config
                .description
                .clone()
                .unwrap_or_else(|| format!("{program} pub get")),
        })
    }

    fn is_applicable(&self) -> bool {
        self.base.config_root().join("pubspec.yaml").exists()
    }

    fn add_command(&self, packages: &[&str], dev: bool) -> Result<DepsCommand> {
        let mut args = vec!["pub".to_string(), "add".to_string()];
        if dev {
            args.push("--dev".to_string());
        }
        args.extend(packages.iter().map(|p| p.to_string()));

        let program = self.program().to_string();
        Ok(DepsCommand {
            program: program.clone(),
            args,
            env: self.base.config.env.clone(),
            cwd: Some(self.base.config_root()),
            description: format!("{program} pub add {}", packages.join(" ")),
        })
    }

    fn remove_command(&self, packages: &[&str]) -> Result<DepsCommand> {
        let mut args = vec!["pub".to_string(), "remove".to_string()];
        args.extend(packages.iter().map(|p| p.to_string()));

        let program = self.program().to_string();
        Ok(DepsCommand {
            program: program.clone(),
            args,
            env: self.base.config.env.clone(),
            cwd: Some(self.base.config_root()),
            description: format!("{program} pub remove {}", packages.join(" ")),
        })
    }
}
