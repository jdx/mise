use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use eyre::{Context, Result, bail};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;
use serde::Deserialize;

use crate::file;

use super::{ProjectId, WorkspaceProject, WorkspaceProvider};

const PACKAGE_JSON: &str = "package.json";
const PNPM_WORKSPACE: &str = "pnpm-workspace.yaml";

/// Discovers Node projects from npm, pnpm, Yarn, and Bun workspace definitions.
#[derive(Debug, Default)]
pub struct NodeWorkspaceProvider;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PackageJson {
    name: Option<String>,
    package_manager: Option<String>,
    workspaces: Option<PackageJsonWorkspaces>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PackageJsonWorkspaces {
    Patterns(Vec<String>),
    Config {
        #[serde(default)]
        packages: Vec<String>,
    },
}

impl PackageJsonWorkspaces {
    fn patterns(self) -> Vec<String> {
        match self {
            Self::Patterns(patterns) => patterns,
            Self::Config { packages } => packages,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct PnpmWorkspace {
    #[serde(default)]
    packages: Vec<String>,
}

struct WorkspaceDefinition {
    patterns: Vec<String>,
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
        let mut roots = expand_workspace_patterns(workspace_root, &definition.patterns)?;
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

        roots
            .into_iter()
            .map(|root| {
                let manifest_path = workspace_root.join(&root).join(PACKAGE_JSON);
                let manifest = read_package_json(&manifest_path)?;
                let name = manifest.name.ok_or_else(|| {
                    eyre::eyre!(
                        "Node workspace package at {} is missing the package.json \"name\" field",
                        root.display()
                    )
                })?;
                let mut project = WorkspaceProject::new(ProjectId::new(self.id(), &name)?, root);
                project.metadata.insert(
                    "workspace_source".to_string(),
                    definition.source.to_string(),
                );
                if let Some(package_manager) = &definition.package_manager {
                    project
                        .metadata
                        .insert("package_manager".to_string(), package_manager.clone());
                }
                Ok(project)
            })
            .collect()
    }
}

fn workspace_definition(workspace_root: &Path) -> Result<Option<WorkspaceDefinition>> {
    let pnpm_workspace_path = workspace_root.join(PNPM_WORKSPACE);
    if pnpm_workspace_path.is_file() {
        let contents = file::read_to_string(&pnpm_workspace_path)?;
        let workspace: PnpmWorkspace = serde_yaml::from_str(&contents).wrap_err_with(|| {
            format!(
                "failed to parse Node workspace definition {}",
                pnpm_workspace_path.display()
            )
        })?;
        return Ok(Some(WorkspaceDefinition {
            patterns: workspace.packages,
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
    let Some(workspaces) = root_manifest.workspaces else {
        return Ok(None);
    };
    let package_manager = detect_package_manager(workspace_root, root_manifest.package_manager);
    let include_named_root = package_manager.as_deref() == Some("yarn");
    Ok(Some(WorkspaceDefinition {
        patterns: workspaces.patterns(),
        source: PACKAGE_JSON,
        package_manager,
        include_named_root,
    }))
}

fn read_package_json(path: &Path) -> Result<PackageJson> {
    let contents = file::read_to_string(path)?;
    serde_json::from_str(&contents)
        .wrap_err_with(|| format!("failed to parse Node package manifest {}", path.display()))
}

fn read_package_json_if_valid(path: &Path) -> Result<Option<PackageJson>> {
    let contents = file::read_to_string(path)?;
    Ok(serde_json::from_str(&contents).ok())
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

fn expand_workspace_patterns(
    workspace_root: &Path,
    patterns: &[String],
) -> Result<BTreeSet<PathBuf>> {
    let mut included = GlobSetBuilder::new();
    let mut excluded = GlobSetBuilder::new();
    let mut walk_roots = BTreeSet::new();
    for raw_pattern in patterns {
        let (target, pattern) = raw_pattern
            .strip_prefix('!')
            .map_or((&mut included, raw_pattern.as_str()), |pattern| {
                (&mut excluded, pattern)
            });
        validate_workspace_pattern(pattern)?;
        let pattern = normalize_workspace_pattern(pattern);
        target.add(
            GlobBuilder::new(pattern)
                .literal_separator(true)
                .backslash_escape(false)
                .build()
                .wrap_err_with(|| format!("invalid Node workspace pattern {pattern:?}"))?,
        );
        if !raw_pattern.starts_with('!') {
            walk_roots.insert(literal_workspace_prefix(pattern));
        }
    }
    if walk_roots.is_empty() {
        return Ok(BTreeSet::new());
    }
    collect_matching_package_roots(
        workspace_root,
        &minimize_walk_roots(walk_roots),
        &included.build()?,
        &excluded.build()?,
    )
}

fn validate_workspace_pattern(pattern: &str) -> Result<()> {
    if pattern.is_empty() {
        bail!("Node workspace pattern cannot be empty");
    }
    let path = Path::new(pattern);
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        bail!(
            "Node workspace pattern {pattern:?} must be relative and cannot escape the workspace root"
        );
    }
    Ok(())
}

fn normalize_workspace_pattern(pattern: &str) -> &str {
    let pattern = pattern
        .strip_prefix("./")
        .unwrap_or(pattern)
        .trim_end_matches('/');
    if pattern.is_empty() { "." } else { pattern }
}

fn literal_workspace_prefix(pattern: &str) -> PathBuf {
    let mut prefix = PathBuf::new();
    for component in pattern.split('/') {
        if component
            .chars()
            .any(|character| matches!(character, '*' | '?' | '[' | '{'))
        {
            break;
        }
        if component != "." {
            prefix.push(component);
        }
    }
    if prefix.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        prefix
    }
}

fn minimize_walk_roots(roots: BTreeSet<PathBuf>) -> BTreeSet<PathBuf> {
    let mut minimized = BTreeSet::new();
    for root in roots {
        if minimized
            .iter()
            .any(|ancestor| ancestor == Path::new(".") || root.starts_with(ancestor))
        {
            continue;
        }
        minimized.insert(root);
    }
    minimized
}

fn collect_matching_package_roots(
    workspace_root: &Path,
    walk_roots: &BTreeSet<PathBuf>,
    included: &GlobSet,
    excluded: &GlobSet,
) -> Result<BTreeSet<PathBuf>> {
    let mut roots = BTreeSet::new();
    for walk_root in walk_roots {
        let walk_root = workspace_root.join(walk_root);
        if !walk_root.exists() {
            continue;
        }
        let mut builder = WalkBuilder::new(walk_root);
        builder
            .hidden(false)
            .git_exclude(false)
            .git_global(false)
            .git_ignore(false)
            .ignore(false)
            .filter_entry(|entry| {
                entry.depth() == 0
                    || !matches!(entry.file_name().to_str(), Some(".git" | "node_modules"))
            });
        for entry in builder.build() {
            let entry = entry.wrap_err("failed to scan Node workspace packages")?;
            if !entry
                .file_type()
                .is_some_and(|file_type| file_type.is_file())
                || entry.file_name() != PACKAGE_JSON
            {
                continue;
            }
            let root = entry
                .path()
                .parent()
                .expect("package.json has a parent")
                .strip_prefix(workspace_root)
                .expect("workspace walk stays beneath its root");
            let root = if root.as_os_str().is_empty() {
                Path::new(".")
            } else {
                root
            };
            if included.is_match(root) && !excluded.is_match(root) {
                roots.insert(root.to_path_buf());
            }
        }
    }
    Ok(roots)
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
    fn narrows_walk_roots_to_literal_pattern_prefixes() {
        let roots = ["packages/*", "packages/nested/**", "apps/**/plugins/*"]
            .into_iter()
            .map(literal_workspace_prefix)
            .collect();

        assert_eq!(
            minimize_walk_roots(roots),
            BTreeSet::from([PathBuf::from("apps"), PathBuf::from("packages")])
        );
        assert_eq!(
            literal_workspace_prefix("{apps,packages}/**"),
            PathBuf::from(".")
        );
    }

    #[test]
    fn rejects_escaping_patterns_and_unnamed_packages() {
        let temp = tempdir().unwrap();
        write(
            &temp.path().join(PACKAGE_JSON),
            r#"{"workspaces":["../outside"]}"#,
        );
        let err = NodeWorkspaceProvider.discover(temp.path()).unwrap_err();
        assert!(err.to_string().contains("cannot escape the workspace root"));

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
