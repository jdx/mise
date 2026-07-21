use std::path::{Path, PathBuf};

use eyre::Result;
use path_absolutize::Absolutize;

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
        let root = self.base.config_root();
        let pub_dir = root.join(".dart_tool/pub");
        let output = workspace_package_config(&pub_dir)
            .unwrap_or_else(|| root.join(".dart_tool/package_config.json"));
        vec![output]
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

/// In a pub workspace, `package_config.json` lives at the workspace root rather
/// than in the member package.
fn workspace_package_config(pub_dir: &Path) -> Option<PathBuf> {
    let contents = crate::file::read_to_string(pub_dir.join("workspace_ref.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&contents).ok()?;
    let workspace_root = json.get("workspaceRoot")?.as_str()?;
    let resolved = pub_dir
        .join(workspace_root)
        .join(".dart_tool/package_config.json");
    // Collapse `..` segments
    Some(
        resolved
            .absolutize()
            .map(|p| p.to_path_buf())
            .unwrap_or(resolved),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn provider(root: &Path) -> DartDepsProvider {
        DartDepsProvider::new("dart", root, DepsProviderConfig::default())
    }

    #[test]
    fn outputs_default_without_workspace_ref() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("pkg");
        fs::create_dir_all(&root).unwrap();
        assert_eq!(
            provider(&root).outputs(),
            vec![root.join(".dart_tool/package_config.json")]
        );
    }

    #[test]
    fn outputs_follow_workspace_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("monorepo/modules/foo");
        let pub_dir = root.join(".dart_tool/pub");
        fs::create_dir_all(&pub_dir).unwrap();
        // "../../../.." is relative to the pub dir and resolves to the monorepo root.
        fs::write(
            pub_dir.join("workspace_ref.json"),
            r#"{"workspaceRoot": "../../../.."}"#,
        )
        .unwrap();

        let expected = tmp.path().join("monorepo/.dart_tool/package_config.json");
        assert_eq!(provider(&root).outputs(), vec![expected]);
    }

    #[test]
    fn outputs_fall_back_on_malformed_or_missing_key() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("pkg");
        let pub_dir = root.join(".dart_tool/pub");
        fs::create_dir_all(&pub_dir).unwrap();
        let default = root.join(".dart_tool/package_config.json");

        // Malformed JSON
        fs::write(pub_dir.join("workspace_ref.json"), "not json").unwrap();
        assert_eq!(provider(&root).outputs(), vec![default.clone()]);

        // Valid JSON but no workspaceRoot key
        fs::write(pub_dir.join("workspace_ref.json"), r#"{"other": 1}"#).unwrap();
        assert_eq!(provider(&root).outputs(), vec![default]);
    }
}
