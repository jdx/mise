use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use aube::embed::{ManifestError, PackageJson, WorkspaceDiscoveryOptions};
use eyre::{Context, Result};

use super::{ProjectId, WorkspaceProject, WorkspaceProvider, WorkspaceTask};

const PACKAGE_JSON: &str = "package.json";
const PNPM_WORKSPACE: &str = "pnpm-workspace.yaml";

/// Discovers Node projects from npm, pnpm, Yarn, and Bun workspace definitions.
#[derive(Debug, Default)]
pub struct NodeWorkspaceProvider;

struct WorkspaceDefinition {
    source: &'static str,
    package_manager: Option<String>,
    include_named_root: bool,
}

impl WorkspaceProvider for NodeWorkspaceProvider {
    fn id(&self) -> &str {
        "node"
    }

    fn discover(&self, workspace_root: &Path) -> Result<Vec<WorkspaceProject>> {
        let Some(definition) = workspace_definition(workspace_root)? else {
            return Ok(Vec::new());
        };
        let canonical_root = workspace_root.canonicalize().wrap_err_with(|| {
            format!(
                "failed to resolve Node workspace root {}",
                workspace_root.display()
            )
        })?;
        let mut roots = aube::embed::discover_workspace_packages(
            &canonical_root,
            WorkspaceDiscoveryOptions::confined_to_root(),
        )
        .map_err(|error| {
            eyre::eyre!(
                "failed to discover Node workspace packages under {}: {error}",
                workspace_root.display()
            )
        })?
        .into_iter()
        .filter_map(|root| {
            let relative = match root.strip_prefix(&canonical_root) {
                Ok(relative) if relative.as_os_str().is_empty() => PathBuf::from("."),
                Ok(relative) => relative.to_path_buf(),
                Err(_) => {
                    return Some(Err(eyre::eyre!(
                        "Node workspace package {} is outside workspace root {}",
                        root.display(),
                        canonical_root.display()
                    )));
                }
            };
            if relative
                .components()
                .any(|component| component.as_os_str() == ".git")
            {
                None
            } else {
                Some(Ok(relative))
            }
        })
        .collect::<Result<BTreeSet<_>>>()?;
        if definition.include_named_root {
            let root_manifest_path = workspace_root.join(PACKAGE_JSON);
            if root_manifest_path.is_file()
                && read_package_json_if_valid(&root_manifest_path)?
                    .and_then(|manifest| manifest.name)
                    .is_some()
            {
                roots.insert(PathBuf::from("."));
            }
        }

        let manifests = roots
            .into_iter()
            .map(|root| {
                let manifest_path = workspace_root.join(&root).join(PACKAGE_JSON);
                let manifest = read_package_json(&manifest_path)?;
                let name = manifest.name.clone().ok_or_else(|| {
                    eyre::eyre!(
                        "Node workspace package at {} is missing the package.json \"name\" field",
                        root.display()
                    )
                })?;
                let id = ProjectId::new(self.id(), &name)?;
                Ok((root, manifest, name, id))
            })
            .collect::<Result<Vec<_>>>()?;
        let project_ids = manifests
            .iter()
            .map(|(_, _, name, id)| (name.clone(), id.clone()))
            .collect::<BTreeMap<_, _>>();

        manifests
            .into_iter()
            .map(|(root, manifest, _, id)| {
                let dependencies = manifest
                    .all_dependencies()
                    .map(|(name, _)| name)
                    .chain(manifest.optional_dependencies.keys().map(String::as_str))
                    .chain(manifest.peer_dependencies.keys().map(String::as_str))
                    .filter_map(|name| project_ids.get(name).cloned())
                    .filter(|dependency| dependency != &id)
                    .collect();
                let source = root.join(PACKAGE_JSON);
                let mut project = WorkspaceProject::new(id, root);
                project.dependencies = dependencies;
                project.metadata.insert(
                    "workspace_source".to_string(),
                    definition.source.to_string(),
                );
                if let Some(package_manager) = &definition.package_manager {
                    project
                        .metadata
                        .insert("package_manager".to_string(), package_manager.clone());
                }
                let package_manager = definition.package_manager.as_deref().unwrap_or("npm");
                project.tasks = workspace_tasks(&manifest, package_manager, &source);
                Ok(project)
            })
            .collect()
    }

    fn discover_project_tasks(
        &self,
        workspace_root: &Path,
        project_root: &Path,
    ) -> Result<BTreeMap<String, WorkspaceTask>> {
        let Some(definition) = workspace_definition(workspace_root)? else {
            return Ok(BTreeMap::new());
        };
        let source = project_root.join(PACKAGE_JSON);
        let manifest = read_package_json(&workspace_root.join(&source))?;
        let package_manager = definition.package_manager.as_deref().unwrap_or("npm");
        Ok(workspace_tasks(&manifest, package_manager, &source))
    }
}

fn workspace_tasks(
    manifest: &PackageJson,
    package_manager: &str,
    source: &Path,
) -> BTreeMap<String, WorkspaceTask> {
    manifest
        .scripts
        .iter()
        .map(|(name, script)| {
            let command = shell_words::join([package_manager, "run", name, "--"]);
            (
                name.clone(),
                WorkspaceTask {
                    command,
                    description: script.clone(),
                    source: source.to_path_buf(),
                },
            )
        })
        .collect()
}

fn workspace_definition(workspace_root: &Path) -> Result<Option<WorkspaceDefinition>> {
    let pnpm_workspace_path = workspace_root.join(PNPM_WORKSPACE);
    if pnpm_workspace_path.is_file() {
        return Ok(Some(WorkspaceDefinition {
            source: PNPM_WORKSPACE,
            package_manager: Some("pnpm".to_string()),
            include_named_root: true,
        }));
    }

    let root_manifest_path = workspace_root.join(PACKAGE_JSON);
    let root_manifest = root_manifest_path
        .is_file()
        .then(|| read_package_json_if_valid(&root_manifest_path))
        .transpose()?
        .flatten();
    let Some(root_manifest) = root_manifest else {
        return Ok(None);
    };
    let Some(workspaces) = root_manifest.workspaces.as_ref() else {
        return Ok(None);
    };
    if workspaces.patterns().is_empty() {
        return Ok(None);
    }
    let package_manager = detect_package_manager(
        workspace_root,
        root_manifest
            .extra
            .get("packageManager")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
    );
    let include_named_root = package_manager.as_deref() == Some("yarn");
    Ok(Some(WorkspaceDefinition {
        source: PACKAGE_JSON,
        package_manager,
        include_named_root,
    }))
}

fn read_package_json(path: &Path) -> Result<PackageJson> {
    PackageJson::from_path(path).map_err(|error| {
        eyre::eyre!(
            "failed to parse Node package manifest {}: {error}",
            path.display()
        )
    })
}

fn read_package_json_if_valid(path: &Path) -> Result<Option<PackageJson>> {
    match PackageJson::from_path(path) {
        Ok(manifest) => Ok(Some(manifest)),
        Err(ManifestError::Parse(_)) => Ok(None),
        Err(error) => Err(eyre::eyre!(
            "failed to read Node package manifest {}: {error}",
            path.display()
        )),
    }
}

fn detect_package_manager(workspace_root: &Path, configured: Option<String>) -> Option<String> {
    configured
        .as_deref()
        .map(|value| value.split_once('@').map_or(value, |(name, _)| name))
        .filter(|manager| matches!(*manager, "npm" | "pnpm" | "yarn" | "bun"))
        .map(str::to_string)
        .or_else(|| {
            [
                ("bun", ["bun.lock", "bun.lockb"].as_slice()),
                ("pnpm", ["pnpm-lock.yaml"].as_slice()),
                ("yarn", ["yarn.lock"].as_slice()),
                (
                    "npm",
                    ["package-lock.json", "npm-shrinkwrap.json"].as_slice(),
                ),
            ]
            .into_iter()
            .find_map(|(manager, lockfiles)| {
                lockfiles
                    .iter()
                    .any(|lockfile| workspace_root.join(lockfile).is_file())
                    .then(|| manager.to_string())
            })
        })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    fn project_summary(projects: &[WorkspaceProject]) -> Vec<(&str, &Path, Option<&str>)> {
        projects
            .iter()
            .map(|project| {
                (
                    project.id.as_str(),
                    project.root.as_path(),
                    project.metadata.get("package_manager").map(String::as_str),
                )
            })
            .collect()
    }

    #[test]
    fn infers_package_scripts_as_workspace_tasks() {
        let temp = tempdir().unwrap();
        write(
            &temp.path().join(PACKAGE_JSON),
            r#"{"packageManager":"pnpm@10.0.0","workspaces":["packages/*"]}"#,
        );
        write(
            &temp.path().join("packages/app/package.json"),
            r#"{"name":"@scope/app","scripts":{"build":"vite build","test:unit":"vitest"}}"#,
        );

        let projects = NodeWorkspaceProvider.discover(temp.path()).unwrap();
        let project = &projects[0];

        assert_eq!(
            project.tasks.get("build"),
            Some(&WorkspaceTask {
                command: "pnpm run build --".to_string(),
                description: "vite build".to_string(),
                source: PathBuf::from("packages/app/package.json"),
            })
        );
        assert_eq!(
            project
                .tasks
                .get("test:unit")
                .map(|task| task.command.as_str()),
            Some("pnpm run test:unit --")
        );
    }

    #[test]
    fn root_overrides_reload_package_scripts() {
        let temp = tempdir().unwrap();
        write(
            &temp.path().join(PACKAGE_JSON),
            r#"{"packageManager":"pnpm@10.0.0","workspaces":["packages/*"]}"#,
        );
        write(
            &temp.path().join("packages/app/package.json"),
            r#"{"name":"app","scripts":{"old":"old command"}}"#,
        );
        write(
            &temp.path().join("overrides/app/package.json"),
            r#"{"name":"app","scripts":{"new":"new command"}}"#,
        );
        let overrides = BTreeMap::from([(
            "node:app".to_string(),
            super::super::WorkspaceProjectOverride {
                root: Some("overrides/app".into()),
                ..Default::default()
            },
        )]);

        let graph = super::super::WorkspaceProjectGraph::discover_all_with_overrides(
            &[&NodeWorkspaceProvider],
            temp.path(),
            &overrides,
        )
        .unwrap();
        let project = graph.get(&ProjectId::new("node", "app").unwrap()).unwrap();

        assert_eq!(project.root, Path::new("overrides/app"));
        assert!(!project.tasks.contains_key("old"));
        assert_eq!(
            project.tasks.get("new"),
            Some(&WorkspaceTask {
                command: "pnpm run new --".to_string(),
                description: "new command".to_string(),
                source: PathBuf::from("overrides/app/package.json"),
            })
        );
    }

    #[test]
    fn discovers_package_json_workspaces_for_npm_yarn_and_bun() {
        for manager in ["npm", "yarn", "bun"] {
            let temp = tempdir().unwrap();
            write(
                &temp.path().join(PACKAGE_JSON),
                &format!(r#"{{"packageManager":"{manager}@1.0.0","workspaces":["packages/*"]}}"#),
            );
            write(
                &temp.path().join("packages/app/package.json"),
                r#"{"name":"@scope/app"}"#,
            );
            write(
                &temp.path().join("packages/lib/package.json"),
                r#"{"name":"lib"}"#,
            );

            let projects = NodeWorkspaceProvider.discover(temp.path()).unwrap();

            assert_eq!(
                project_summary(&projects),
                vec![
                    ("node:@scope/app", Path::new("packages/app"), Some(manager)),
                    ("node:lib", Path::new("packages/lib"), Some(manager)),
                ]
            );
            assert!(projects.iter().all(|project| {
                project.metadata.get("workspace_source").map(String::as_str) == Some(PACKAGE_JSON)
            }));
        }
    }

    #[test]
    fn supports_yarn_object_workspace_syntax() {
        let temp = tempdir().unwrap();
        write(
            &temp.path().join(PACKAGE_JSON),
            r#"{"workspaces":{"packages":["apps/*"],"nohoist":["**/react"]}}"#,
        );
        write(
            &temp.path().join("apps/web/package.json"),
            r#"{"name":"web"}"#,
        );

        let projects = NodeWorkspaceProvider.discover(temp.path()).unwrap();

        assert_eq!(
            project_summary(&projects),
            vec![("node:web", Path::new("apps/web"), None)]
        );
    }

    #[test]
    fn yarn_includes_a_named_root_package() {
        let temp = tempdir().unwrap();
        write(
            &temp.path().join(PACKAGE_JSON),
            r#"{"name":"root","packageManager":"yarn@4.0.0","workspaces":["packages/*"]}"#,
        );
        write(
            &temp.path().join("packages/app/package.json"),
            r#"{"name":"app"}"#,
        );

        let projects = NodeWorkspaceProvider.discover(temp.path()).unwrap();

        assert_eq!(
            project_summary(&projects),
            vec![
                ("node:root", Path::new("."), Some("yarn")),
                ("node:app", Path::new("packages/app"), Some("yarn")),
            ]
        );
    }

    #[test]
    fn supports_string_workspace_syntax() {
        let temp = tempdir().unwrap();
        write(
            &temp.path().join(PACKAGE_JSON),
            r#"{"workspaces":"packages/*"}"#,
        );
        write(
            &temp.path().join("packages/app/package.json"),
            r#"{"name":"app"}"#,
        );

        let projects = NodeWorkspaceProvider.discover(temp.path()).unwrap();

        assert_eq!(
            project_summary(&projects),
            vec![("node:app", Path::new("packages/app"), None)]
        );
    }

    #[test]
    fn pnpm_workspace_takes_precedence_and_supports_exclusions() {
        let temp = tempdir().unwrap();
        write(
            &temp.path().join(PACKAGE_JSON),
            r#"{"name":"root","workspaces":["ignored/*"]}"#,
        );
        write(
            &temp.path().join(PNPM_WORKSPACE),
            "packages:\n  - packages/*\n  - '!packages/private'\n",
        );
        write(
            &temp.path().join("ignored/app/package.json"),
            r#"{"name":"ignored"}"#,
        );
        write(
            &temp.path().join("packages/app/package.json"),
            r#"{"name":"app"}"#,
        );
        write(
            &temp.path().join("packages/private/package.json"),
            r#"{"name":"private"}"#,
        );

        let projects = NodeWorkspaceProvider.discover(temp.path()).unwrap();

        assert_eq!(
            project_summary(&projects),
            vec![
                ("node:root", Path::new("."), Some("pnpm")),
                ("node:app", Path::new("packages/app"), Some("pnpm")),
            ]
        );
        assert_eq!(
            projects[1]
                .metadata
                .get("workspace_source")
                .map(String::as_str),
            Some(PNPM_WORKSPACE)
        );
    }

    #[test]
    fn full_glob_patterns_ignore_installed_dependencies() {
        let temp = tempdir().unwrap();
        write(
            &temp.path().join(PACKAGE_JSON),
            r#"{"workspaces":["{apps,packages}/**","!**/excluded/**"]}"#,
        );
        write(
            &temp.path().join("apps/web/package.json"),
            r#"{"name":"web"}"#,
        );
        write(
            &temp.path().join("packages/app/package.json"),
            r#"{"name":"app"}"#,
        );
        write(
            &temp.path().join("packages/excluded/test/package.json"),
            r#"{"name":"excluded"}"#,
        );
        write(
            &temp
                .path()
                .join("packages/app/node_modules/transitive/package.json"),
            r#"{"name":"transitive"}"#,
        );

        let projects = NodeWorkspaceProvider.discover(temp.path()).unwrap();

        assert_eq!(
            project_summary(&projects),
            vec![
                ("node:web", Path::new("apps/web"), None),
                ("node:app", Path::new("packages/app"), None),
            ]
        );
    }

    #[test]
    fn workspace_discovery_does_not_honor_dot_ignore_files() {
        let temp = tempdir().unwrap();
        write(
            &temp.path().join(PACKAGE_JSON),
            r#"{"workspaces":["packages/*"]}"#,
        );
        write(&temp.path().join(".ignore"), "packages/ignored\n");
        write(
            &temp.path().join("packages/ignored/package.json"),
            r#"{"name":"ignored"}"#,
        );

        let projects = NodeWorkspaceProvider.discover(temp.path()).unwrap();

        assert_eq!(
            project_summary(&projects),
            vec![("node:ignored", Path::new("packages/ignored"), None)]
        );
    }

    #[test]
    fn workspace_discovery_excludes_git_metadata() {
        let temp = tempdir().unwrap();
        write(&temp.path().join(PACKAGE_JSON), r#"{"workspaces":["**"]}"#);
        write(
            &temp.path().join("packages/app/package.json"),
            r#"{"name":"app"}"#,
        );
        write(
            &temp.path().join(".git/objects/example/package.json"),
            r#"{"name":"git-object"}"#,
        );

        let projects = NodeWorkspaceProvider.discover(temp.path()).unwrap();

        assert_eq!(
            project_summary(&projects),
            vec![("node:app", Path::new("packages/app"), None)]
        );
    }

    #[test]
    fn pnpm_skips_an_unnamed_root_package() {
        let temp = tempdir().unwrap();
        write(&temp.path().join(PACKAGE_JSON), r#"{"private":true}"#);
        write(&temp.path().join(PNPM_WORKSPACE), "packages: []\n");

        let projects = NodeWorkspaceProvider.discover(temp.path()).unwrap();

        assert!(projects.is_empty());
    }

    #[test]
    fn pnpm_discovery_does_not_require_a_valid_root_manifest() {
        let temp = tempdir().unwrap();
        write(&temp.path().join(PACKAGE_JSON), "{");
        write(
            &temp.path().join(PNPM_WORKSPACE),
            "packages:\n  - packages/*\n",
        );
        write(
            &temp.path().join("packages/app/package.json"),
            r#"{"name":"app"}"#,
        );

        let projects = NodeWorkspaceProvider.discover(temp.path()).unwrap();

        assert_eq!(
            project_summary(&projects),
            vec![("node:app", Path::new("packages/app"), Some("pnpm"))]
        );
    }

    #[test]
    fn ignores_an_invalid_root_manifest_without_a_workspace_definition() {
        let temp = tempdir().unwrap();
        write(&temp.path().join(PACKAGE_JSON), "{");

        let projects = NodeWorkspaceProvider.discover(temp.path()).unwrap();

        assert!(projects.is_empty());
    }

    #[test]
    fn detects_pnpm_from_its_lockfile() {
        let temp = tempdir().unwrap();
        write(
            &temp.path().join(PACKAGE_JSON),
            r#"{"workspaces":["packages/*"]}"#,
        );
        write(
            &temp.path().join("pnpm-lock.yaml"),
            "lockfileVersion: '9.0'\n",
        );
        write(
            &temp.path().join("packages/app/package.json"),
            r#"{"name":"app"}"#,
        );

        let projects = NodeWorkspaceProvider.discover(temp.path()).unwrap();

        assert_eq!(
            project_summary(&projects),
            vec![("node:app", Path::new("packages/app"), Some("pnpm"))]
        );
    }

    #[test]
    fn infers_edges_for_declared_internal_dependencies() {
        let temp = tempdir().unwrap();
        write(
            &temp.path().join(PACKAGE_JSON),
            r#"{"workspaces":["packages/*"]}"#,
        );
        write(
            &temp.path().join("packages/app/package.json"),
            r#"{
                "name":"app",
                "dependencies":{"runtime":"workspace:*","external":"^1.0.0"},
                "devDependencies":{"build":"*"},
                "optionalDependencies":{"optional":"catalog:"},
                "peerDependencies":{"peer":"^2.0.0"}
            }"#,
        );
        for name in ["runtime", "build", "optional", "peer"] {
            write(
                &temp.path().join(format!("packages/{name}/package.json")),
                &format!(r#"{{"name":"{name}"}}"#),
            );
        }

        let projects = NodeWorkspaceProvider.discover(temp.path()).unwrap();
        let app = projects
            .iter()
            .find(|project| project.id.as_str() == "node:app")
            .unwrap();

        assert_eq!(
            app.dependencies,
            BTreeSet::from([
                ProjectId::new("node", "build").unwrap(),
                ProjectId::new("node", "optional").unwrap(),
                ProjectId::new("node", "peer").unwrap(),
                ProjectId::new("node", "runtime").unwrap(),
            ])
        );
    }

    #[test]
    fn ignores_declared_self_dependencies() {
        let temp = tempdir().unwrap();
        write(
            &temp.path().join(PACKAGE_JSON),
            r#"{"workspaces":["packages/*"]}"#,
        );
        write(
            &temp.path().join("packages/app/package.json"),
            r#"{"name":"app","dependencies":{"app":"*"}}"#,
        );

        let graph = crate::task::workspace::WorkspaceProjectGraph::discover(
            &NodeWorkspaceProvider,
            temp.path(),
        )
        .unwrap();
        let app = graph.get(&ProjectId::new("node", "app").unwrap()).unwrap();

        assert!(app.dependencies.is_empty());
    }

    #[test]
    fn rejects_escaping_patterns_and_unnamed_packages() {
        let temp = tempdir().unwrap();
        write(
            &temp.path().join(PACKAGE_JSON),
            r#"{"workspaces":["../outside"]}"#,
        );
        let err = NodeWorkspaceProvider.discover(temp.path()).unwrap_err();
        assert!(
            err.to_string()
                .contains("failed to discover Node workspace packages under")
        );

        write(
            &temp.path().join(PACKAGE_JSON),
            r#"{"workspaces":["packages/*"]}"#,
        );
        write(&temp.path().join("packages/app/package.json"), "{}");
        let err = NodeWorkspaceProvider.discover(temp.path()).unwrap_err();
        assert!(
            err.to_string()
                .contains("missing the package.json \"name\"")
        );
    }
}
