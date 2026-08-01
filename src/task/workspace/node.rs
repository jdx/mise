use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use aube::embed::{ManifestError, PackageJson, WorkspaceDiscoveryOptions};
use eyre::{Context, Result};
use serde::Deserialize;

use super::{
    ProjectId, WorkspaceDiscoveryContext, WorkspaceProject, WorkspaceProvenance, WorkspaceProvider,
    WorkspaceTask, WorkspaceTaskSuggestions,
};

const PACKAGE_JSON: &str = "package.json";
const PNPM_WORKSPACE: &str = "pnpm-workspace.yaml";
const TURBO_JSON: &str = "turbo.json";

/// Discovers Node projects from npm, pnpm, Yarn, and Bun workspace definitions.
#[derive(Debug, Default)]
pub struct NodeWorkspaceProvider;

struct WorkspaceDefinition {
    source: &'static str,
    package_manager: Option<String>,
    include_named_root: bool,
    root_manifest: Option<PackageJson>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct TurboJson {
    tasks: BTreeMap<String, TurboTask>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct TurboTask {
    inputs: Option<Vec<String>>,
    outputs: Option<Vec<String>>,
    cache: Option<bool>,
    #[serde(rename = "dependsOn")]
    depends_on: Option<Vec<String>>,
}

impl WorkspaceProvider for NodeWorkspaceProvider {
    fn id(&self) -> &str {
        "node"
    }

    fn discover(&self, workspace_root: &Path) -> Result<Vec<WorkspaceProject>> {
        let context = WorkspaceDiscoveryContext::new();
        self.discover_with_context(workspace_root, &context)
    }

    fn discover_with_context(
        &self,
        workspace_root: &Path,
        context: &WorkspaceDiscoveryContext,
    ) -> Result<Vec<WorkspaceProject>> {
        let Some(definition) = workspace_definition(workspace_root, context)? else {
            return Ok(Vec::new());
        };
        let canonical_root = context.canonicalize(workspace_root).wrap_err_with(|| {
            format!(
                "failed to resolve Node workspace root {}",
                workspace_root.display()
            )
        })?;
        let turbo = read_turbo_json(workspace_root, context)?;
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
        let mut root_manifest = definition.root_manifest.clone();
        if definition.include_named_root && root_manifest.is_none() {
            let root_manifest_path = workspace_root.join(PACKAGE_JSON);
            root_manifest = context
                .is_file(&root_manifest_path)
                .then(|| read_package_json_if_valid(context, &root_manifest_path))
                .transpose()?
                .flatten();
        }
        if definition.include_named_root
            && root_manifest
                .as_ref()
                .and_then(|manifest| manifest.name.as_ref())
                .is_some()
        {
            roots.insert(PathBuf::from("."));
        }

        let manifests = roots
            .into_iter()
            .map(|root| {
                let manifest_path = workspace_root.join(&root).join(PACKAGE_JSON);
                let manifest = if root == Path::new(".") {
                    match root_manifest.clone() {
                        Some(manifest) => manifest,
                        None => read_package_json(context, &manifest_path)?,
                    }
                } else {
                    read_package_json(context, &manifest_path)?
                };
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
                project.provenance = WorkspaceProvenance {
                    provider: Some("node".to_string()),
                    source: Some(source.clone()),
                };
                project.dependency_provenance = project
                    .dependencies
                    .iter()
                    .cloned()
                    .map(|dependency| {
                        (
                            dependency,
                            WorkspaceProvenance {
                                provider: Some("node".to_string()),
                                source: Some(source.clone()),
                            },
                        )
                    })
                    .collect();
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
                project.tasks = workspace_tasks(
                    &manifest,
                    package_manager,
                    &source,
                    turbo.as_ref(),
                    workspace_root,
                );
                Ok(project)
            })
            .collect()
    }

    fn discover_project_tasks(
        &self,
        workspace_root: &Path,
        project_root: &Path,
    ) -> Result<BTreeMap<String, WorkspaceTask>> {
        let context = WorkspaceDiscoveryContext::new();
        self.discover_project_tasks_with_context(workspace_root, project_root, &context)
    }

    fn discover_project_tasks_with_context(
        &self,
        workspace_root: &Path,
        project_root: &Path,
        context: &WorkspaceDiscoveryContext,
    ) -> Result<BTreeMap<String, WorkspaceTask>> {
        let Some(definition) = workspace_definition(workspace_root, context)? else {
            return Ok(BTreeMap::new());
        };
        let source = project_root.join(PACKAGE_JSON);
        let manifest_path = workspace_root.join(&source);
        if !context.is_file(&manifest_path) {
            return Ok(BTreeMap::new());
        }
        let Some(manifest) = read_package_json_if_valid(context, &manifest_path)? else {
            return Ok(BTreeMap::new());
        };
        let package_manager = definition.package_manager.as_deref().unwrap_or("npm");
        let turbo = read_turbo_json(workspace_root, context)?;
        Ok(workspace_tasks(
            &manifest,
            package_manager,
            &source,
            turbo.as_ref(),
            workspace_root,
        ))
    }
}

fn workspace_tasks(
    manifest: &PackageJson,
    package_manager: &str,
    source: &Path,
    turbo: Option<&TurboJson>,
    workspace_root: &Path,
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
                    provenance: WorkspaceProvenance {
                        provider: Some("node".to_string()),
                        source: Some(source.to_path_buf()),
                    },
                    suggestions: turbo
                        .and_then(|turbo| turbo.task(manifest.name.as_deref(), name))
                        .map(|task| task.suggestions(workspace_root))
                        .unwrap_or_default(),
                },
            )
        })
        .collect()
}

impl TurboJson {
    fn task(&self, package: Option<&str>, task: &str) -> Option<&TurboTask> {
        package
            .and_then(|package| self.tasks.get(&format!("{package}#{task}")))
            .or_else(|| self.tasks.get(task))
    }
}

impl TurboTask {
    fn suggestions(&self, workspace_root: &Path) -> WorkspaceTaskSuggestions {
        let supported_patterns = |patterns: &Option<Vec<String>>| {
            patterns
                .as_ref()
                .filter(|patterns| patterns.iter().all(|pattern| !pattern.contains('$')))
                .cloned()
        };
        let inputs = supported_patterns(&self.inputs)
            .filter(|patterns| !patterns.is_empty())
            .unwrap_or_default();
        let outputs = supported_patterns(&self.outputs);
        let depends = self
            .depends_on
            .as_ref()
            .filter(|dependencies| {
                dependencies.iter().all(|dependency| {
                    let task = dependency.strip_prefix('^').unwrap_or(dependency);
                    !task.is_empty() && !task.contains('#') && !task.contains('$')
                })
            })
            .cloned();
        let provenance = || WorkspaceProvenance {
            provider: Some("node".to_string()),
            source: Some(PathBuf::from(TURBO_JSON)),
        };

        WorkspaceTaskSuggestions {
            provenance: super::WorkspaceTaskSuggestionProvenance {
                inputs: (!inputs.is_empty()).then(provenance),
                outputs: outputs.as_ref().map(|_| provenance()),
                cache: self.cache.map(|_| provenance()),
                depends: depends.as_ref().map(|_| provenance()),
            },
            inputs,
            outputs,
            cache: self.cache,
            depends,
            config_sources: vec![workspace_root.join(TURBO_JSON)],
        }
    }
}

fn read_turbo_json(
    workspace_root: &Path,
    context: &WorkspaceDiscoveryContext,
) -> Result<Option<TurboJson>> {
    let path = workspace_root.join(TURBO_JSON);
    if !context.is_file(&path) {
        return Ok(None);
    }
    let contents = match context.read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) => {
            warn!("failed to read optional {}: {error}", path.display());
            return Ok(None);
        }
    };
    match serde_json::from_str(&contents) {
        Ok(turbo) => Ok(Some(turbo)),
        Err(error) => {
            warn!("failed to parse optional {}: {error}", path.display());
            Ok(None)
        }
    }
}

fn workspace_definition(
    workspace_root: &Path,
    context: &WorkspaceDiscoveryContext,
) -> Result<Option<WorkspaceDefinition>> {
    let pnpm_workspace_path = workspace_root.join(PNPM_WORKSPACE);
    if context.is_file(&pnpm_workspace_path) {
        return Ok(Some(WorkspaceDefinition {
            source: PNPM_WORKSPACE,
            package_manager: Some("pnpm".to_string()),
            include_named_root: true,
            root_manifest: None,
        }));
    }

    let root_manifest_path = workspace_root.join(PACKAGE_JSON);
    let root_manifest = context
        .is_file(&root_manifest_path)
        .then(|| read_package_json_if_valid(context, &root_manifest_path))
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
        context,
    );
    let include_named_root = package_manager.as_deref() == Some("yarn");
    Ok(Some(WorkspaceDefinition {
        source: PACKAGE_JSON,
        package_manager,
        include_named_root,
        root_manifest: Some(root_manifest),
    }))
}

fn read_package_json(context: &WorkspaceDiscoveryContext, path: &Path) -> Result<PackageJson> {
    let contents = context.read_to_string(path).map_err(|error| {
        eyre::eyre!(
            "failed to read Node package manifest {}: {error}",
            path.display()
        )
    })?;
    PackageJson::parse(path, contents.to_string()).map_err(|error| {
        eyre::eyre!(
            "failed to parse Node package manifest {}: {error}",
            path.display()
        )
    })
}

fn read_package_json_if_valid(
    context: &WorkspaceDiscoveryContext,
    path: &Path,
) -> Result<Option<PackageJson>> {
    let contents = context.read_to_string(path).map_err(|error| {
        eyre::eyre!(
            "failed to read Node package manifest {}: {error}",
            path.display()
        )
    })?;
    match PackageJson::parse(path, contents.to_string()) {
        Ok(manifest) => Ok(Some(manifest)),
        Err(ManifestError::Parse(_)) => Ok(None),
        Err(error) => Err(eyre::eyre!(
            "failed to read Node package manifest {}: {error}",
            path.display()
        )),
    }
}

fn detect_package_manager(
    workspace_root: &Path,
    configured: Option<String>,
    context: &WorkspaceDiscoveryContext,
) -> Option<String> {
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
                    .any(|lockfile| context.is_file(&workspace_root.join(lockfile)))
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
                provenance: WorkspaceProvenance {
                    provider: Some("node".to_string()),
                    source: Some(PathBuf::from("packages/app/package.json")),
                },
                suggestions: WorkspaceTaskSuggestions::default(),
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
    fn imports_authoritative_turbo_task_suggestions() {
        let temp = tempdir().unwrap();
        write(
            &temp.path().join(PACKAGE_JSON),
            r#"{"packageManager":"pnpm@10.0.0","workspaces":["packages/*"]}"#,
        );
        write(
            &temp.path().join("packages/app/package.json"),
            r#"{"name":"@scope/app","scripts":{"build":"vite build"}}"#,
        );
        write(
            &temp.path().join(TURBO_JSON),
            r#"{
                "tasks": {
                    "build": {
                        "inputs": ["src/**", "package.json"],
                        "outputs": ["dist/**"],
                        "cache": true,
                        "dependsOn": ["^build", "prepare"]
                    }
                }
            }"#,
        );

        let projects = NodeWorkspaceProvider.discover(temp.path()).unwrap();
        let suggestions = &projects[0].tasks["build"].suggestions;

        assert_eq!(suggestions.inputs, ["src/**", "package.json"]);
        assert_eq!(
            suggestions
                .outputs
                .as_ref()
                .map(|outputs| outputs.iter().map(String::as_str).collect::<Vec<_>>()),
            Some(vec!["dist/**"])
        );
        assert_eq!(suggestions.cache, Some(true));
        assert_eq!(
            suggestions
                .depends
                .as_ref()
                .map(|depends| { depends.iter().map(String::as_str).collect::<Vec<_>>() }),
            Some(vec!["^build", "prepare"])
        );
        assert_eq!(suggestions.config_sources, [temp.path().join(TURBO_JSON)]);
        assert_eq!(
            suggestions.provenance.inputs,
            Some(WorkspaceProvenance {
                provider: Some("node".to_string()),
                source: Some(PathBuf::from(TURBO_JSON)),
            })
        );
        assert_eq!(
            suggestions.provenance.outputs,
            suggestions.provenance.inputs
        );
        assert_eq!(suggestions.provenance.cache, suggestions.provenance.inputs);
        assert_eq!(
            suggestions.provenance.depends,
            suggestions.provenance.inputs
        );
    }

    #[test]
    fn invalid_turbo_json_does_not_remove_package_scripts() {
        for contents in ["{", r#"{"tasks":{"build":{"cache":"yes"}}}"#] {
            let temp = tempdir().unwrap();
            write(
                &temp.path().join(PACKAGE_JSON),
                r#"{"packageManager":"pnpm@10.0.0","workspaces":["packages/*"]}"#,
            );
            write(
                &temp.path().join("packages/app/package.json"),
                r#"{"name":"app","scripts":{"build":"vite build"}}"#,
            );
            write(&temp.path().join(TURBO_JSON), contents);

            let projects = NodeWorkspaceProvider.discover(temp.path()).unwrap();
            let task = &projects[0].tasks["build"];

            assert_eq!(task.command, "pnpm run build --");
            assert_eq!(task.suggestions, WorkspaceTaskSuggestions::default());
        }
    }

    #[test]
    fn leaves_unsupported_turbo_suggestion_fields_unset() {
        let task: TurboTask = serde_json::from_str(
            r#"{
                "inputs": ["$TURBO_DEFAULT$", "src/**"],
                "outputs": ["$TURBO_ROOT$/dist/**"],
                "dependsOn": ["other-package#build"],
                "cache": false
            }"#,
        )
        .unwrap();

        let suggestions = task.suggestions(Path::new("/workspace"));

        assert!(suggestions.inputs.is_empty());
        assert!(suggestions.outputs.is_none());
        assert!(suggestions.depends.is_none());
        assert_eq!(suggestions.cache, Some(false));
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
        assert_eq!(project.provenance, WorkspaceProvenance::default());
        assert!(!project.tasks.contains_key("old"));
        assert_eq!(
            project.tasks.get("new"),
            Some(&WorkspaceTask {
                command: "pnpm run new --".to_string(),
                description: "new command".to_string(),
                source: PathBuf::from("overrides/app/package.json"),
                provenance: WorkspaceProvenance {
                    provider: Some("node".to_string()),
                    source: Some(PathBuf::from("overrides/app/package.json")),
                },
                suggestions: WorkspaceTaskSuggestions::default(),
            })
        );
    }

    #[test]
    fn root_overrides_without_valid_manifests_have_no_scripts() {
        let temp = tempdir().unwrap();
        write(
            &temp.path().join(PACKAGE_JSON),
            r#"{"workspaces":["packages/*"]}"#,
        );
        write(
            &temp.path().join("packages/app/package.json"),
            r#"{"name":"app","scripts":{"old":"old command"}}"#,
        );
        let overrides = BTreeMap::from([(
            "node:app".to_string(),
            super::super::WorkspaceProjectOverride {
                root: Some("overrides/app".into()),
                ..Default::default()
            },
        )]);

        let discover = || {
            super::super::WorkspaceProjectGraph::discover_all_with_overrides(
                &[&NodeWorkspaceProvider],
                temp.path(),
                &overrides,
            )
            .unwrap()
        };

        assert!(
            discover()
                .get(&ProjectId::new("node", "app").unwrap())
                .unwrap()
                .tasks
                .is_empty()
        );

        write(&temp.path().join("overrides/app/package.json"), "{");
        assert!(
            discover()
                .get(&ProjectId::new("node", "app").unwrap())
                .unwrap()
                .tasks
                .is_empty()
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
    fn pnpm_root_package_parse_errors_do_not_panic() {
        let temp = tempdir().unwrap();
        write(&temp.path().join(PACKAGE_JSON), "{");
        write(&temp.path().join(PNPM_WORKSPACE), "packages:\n  - .\n");

        let error = NodeWorkspaceProvider.discover(temp.path()).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("failed to parse Node package manifest")
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
        assert_eq!(
            app.provenance,
            WorkspaceProvenance {
                provider: Some("node".to_string()),
                source: Some(PathBuf::from("packages/app/package.json")),
            }
        );
        assert!(app.dependencies.iter().all(|dependency| {
            app.dependency_provenance.get(dependency)
                == Some(&WorkspaceProvenance {
                    provider: Some("node".to_string()),
                    source: Some(PathBuf::from("packages/app/package.json")),
                })
        }));
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
