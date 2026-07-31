use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use eyre::{Context, Result, bail};
use glob::{MatchOptions, Pattern};
use serde::Deserialize;

use super::{ProjectId, WorkspaceProject, WorkspaceProvenance, WorkspaceProvider};

const PYPROJECT_TOML: &str = "pyproject.toml";

/// Discovers Python projects from uv workspace and local source metadata.
#[derive(Debug, Default)]
pub struct UvWorkspaceProvider;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct PyProject {
    project: Option<PythonProject>,
    #[serde(rename = "dependency-groups")]
    dependency_groups: BTreeMap<String, Vec<toml::Value>>,
    tool: ToolTable,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct PythonProject {
    name: String,
    dependencies: Vec<String>,
    #[serde(rename = "optional-dependencies")]
    optional_dependencies: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ToolTable {
    uv: UvTable,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct UvTable {
    workspace: Option<UvWorkspace>,
    sources: BTreeMap<String, toml::Value>,
    #[serde(rename = "dev-dependencies")]
    dev_dependencies: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct UvWorkspace {
    members: Vec<String>,
    exclude: Vec<String>,
}

struct ProjectManifest {
    root: PathBuf,
    canonical_root: PathBuf,
    pyproject: PyProject,
    id: ProjectId,
    workspace_member: bool,
}

impl WorkspaceProvider for UvWorkspaceProvider {
    fn id(&self) -> &str {
        "uv"
    }

    fn discover(&self, workspace_root: &Path) -> Result<Vec<WorkspaceProject>> {
        let root_pyproject_path = workspace_root.join(PYPROJECT_TOML);
        if !root_pyproject_path.is_file() {
            return Ok(Vec::new());
        }
        let root_pyproject = read_pyproject(&root_pyproject_path)?;
        let Some(workspace) = root_pyproject.tool.uv.workspace.as_ref() else {
            return Ok(Vec::new());
        };
        let canonical_root = workspace_root.canonicalize().wrap_err_with(|| {
            format!(
                "failed to resolve uv workspace root {}",
                workspace_root.display()
            )
        })?;
        let excludes = compile_patterns(&workspace.exclude, "exclude")?;
        let mut workspace_roots = discover_members(
            workspace_root,
            &canonical_root,
            &workspace.members,
            &excludes,
        )?;
        if root_pyproject.project.is_some() {
            workspace_roots.insert(canonical_root.clone());
        }

        let mut known_roots = workspace_roots.clone();
        let mut queue = workspace_roots.iter().cloned().collect::<VecDeque<_>>();
        let mut manifests = BTreeMap::new();
        while let Some(project_root) = queue.pop_front() {
            if manifests.contains_key(&project_root) {
                continue;
            }
            let relative = relative_root(&canonical_root, &project_root)?;
            let pyproject_path = project_root.join(PYPROJECT_TOML);
            let pyproject = read_pyproject(&pyproject_path)?;
            let project = pyproject.project.as_ref().ok_or_else(|| {
                eyre::eyre!("uv project at {} is missing [project]", relative.display())
            })?;
            if project.name.is_empty() {
                bail!(
                    "uv project at {} has an empty project.name",
                    relative.display()
                );
            }
            let id = ProjectId::new(self.id(), &normalize_package_name(&project.name))?;
            let workspace_member = workspace_roots.contains(&project_root);

            for dependency in dependency_names(&pyproject) {
                let Some((source, source_base)) = effective_source(
                    &pyproject,
                    &root_pyproject,
                    &dependency,
                    workspace_member,
                    &project_root,
                    &canonical_root,
                ) else {
                    continue;
                };
                for source in source_tables(source) {
                    let Some(path) = source.get("path").and_then(toml::Value::as_str) else {
                        continue;
                    };
                    let candidate = source_base.join(path);
                    if !candidate.join(PYPROJECT_TOML).is_file() {
                        continue;
                    }
                    let candidate = candidate.canonicalize().wrap_err_with(|| {
                        format!(
                            "failed to resolve uv path dependency {}",
                            candidate.display()
                        )
                    })?;
                    if candidate.strip_prefix(&canonical_root).is_ok()
                        && known_roots.insert(candidate.clone())
                    {
                        queue.push_back(candidate);
                    }
                }
            }

            manifests.insert(
                project_root.clone(),
                ProjectManifest {
                    root: relative,
                    canonical_root: project_root,
                    pyproject,
                    id,
                    workspace_member,
                },
            );
        }

        let ids_by_root = manifests
            .values()
            .map(|project| (project.canonical_root.clone(), project.id.clone()))
            .collect::<BTreeMap<_, _>>();
        let workspace_ids_by_name = manifests
            .values()
            .filter(|project| project.workspace_member)
            .map(|project| {
                let name = project
                    .pyproject
                    .project
                    .as_ref()
                    .expect("validated uv project")
                    .name
                    .as_str();
                (normalize_package_name(name), project.id.clone())
            })
            .collect::<BTreeMap<_, _>>();

        manifests
            .into_values()
            .map(|project| {
                let source_path = project.root.join(PYPROJECT_TOML);
                let mut dependencies = BTreeSet::new();
                for dependency in dependency_names(&project.pyproject) {
                    let Some((source, source_base)) = effective_source(
                        &project.pyproject,
                        &root_pyproject,
                        &dependency,
                        project.workspace_member,
                        &project.canonical_root,
                        &canonical_root,
                    ) else {
                        continue;
                    };
                    for source in source_tables(source) {
                        if source.get("workspace").and_then(toml::Value::as_bool) == Some(true)
                            && let Some(id) = workspace_ids_by_name.get(&dependency)
                            && id != &project.id
                        {
                            dependencies.insert(id.clone());
                        }
                        let Some(path) = source.get("path").and_then(toml::Value::as_str) else {
                            continue;
                        };
                        let candidate = source_base.join(path);
                        if !candidate.join(PYPROJECT_TOML).is_file() {
                            continue;
                        }
                        let candidate = candidate.canonicalize().wrap_err_with(|| {
                            format!(
                                "failed to resolve uv path dependency {}",
                                candidate.display()
                            )
                        })?;
                        if let Some(id) = ids_by_root.get(&candidate)
                            && id != &project.id
                        {
                            dependencies.insert(id.clone());
                        }
                    }
                }

                let provenance = WorkspaceProvenance {
                    provider: Some(self.id().to_string()),
                    source: Some(source_path.clone()),
                };
                let mut workspace_project = WorkspaceProject::new(project.id, project.root);
                workspace_project.dependency_provenance = dependencies
                    .iter()
                    .cloned()
                    .map(|dependency| (dependency, provenance.clone()))
                    .collect();
                workspace_project.dependencies = dependencies;
                workspace_project.provenance = provenance;
                workspace_project
                    .metadata
                    .insert("workspace_source".to_string(), PYPROJECT_TOML.to_string());
                Ok(workspace_project)
            })
            .collect()
    }
}

fn read_pyproject(path: &Path) -> Result<PyProject> {
    let contents = fs::read_to_string(path)
        .wrap_err_with(|| format!("failed to read uv project metadata {}", path.display()))?;
    toml::from_str(&contents)
        .wrap_err_with(|| format!("failed to parse uv project metadata {}", path.display()))
}

fn compile_patterns(patterns: &[String], kind: &str) -> Result<Vec<Pattern>> {
    patterns
        .iter()
        .map(|pattern| {
            Pattern::new(pattern)
                .wrap_err_with(|| format!("invalid uv workspace {kind} pattern {pattern:?}"))
        })
        .collect()
}

fn discover_members(
    pattern_root: &Path,
    canonical_root: &Path,
    members: &[String],
    excludes: &[Pattern],
) -> Result<BTreeSet<PathBuf>> {
    let options = match_options();
    // Avoid canonical Windows roots here because their verbatim `\\?\` prefix
    // contains a character that glob interprets as a wildcard.
    let escaped_root = Pattern::escape(&pattern_root.to_string_lossy());
    let mut roots = BTreeSet::new();
    for member in members {
        if Path::new(member).is_absolute() {
            bail!("uv workspace member pattern {member:?} must be relative");
        }
        let pattern = format!("{escaped_root}/{member}");
        for candidate in glob::glob_with(&pattern, options)
            .wrap_err_with(|| format!("invalid uv workspace member pattern {member:?}"))?
        {
            let candidate = candidate.wrap_err_with(|| {
                format!("failed to evaluate uv workspace member pattern {member:?}")
            })?;
            if !candidate.is_dir() {
                continue;
            }
            let candidate = candidate.canonicalize().wrap_err_with(|| {
                format!(
                    "failed to resolve uv workspace member {}",
                    candidate.display()
                )
            })?;
            let relative = relative_root(canonical_root, &candidate)?;
            if is_excluded(&relative, excludes) {
                continue;
            }
            if !candidate.join(PYPROJECT_TOML).is_file() {
                bail!(
                    "uv workspace member at {} is missing {PYPROJECT_TOML}",
                    relative.display()
                );
            }
            roots.insert(candidate);
        }
    }
    Ok(roots)
}

fn match_options() -> MatchOptions {
    MatchOptions {
        case_sensitive: true,
        require_literal_separator: true,
        require_literal_leading_dot: true,
    }
}

fn relative_root(workspace_root: &Path, project_root: &Path) -> Result<PathBuf> {
    let relative = project_root.strip_prefix(workspace_root).map_err(|_| {
        eyre::eyre!(
            "uv workspace member {} is outside workspace root {}",
            project_root.display(),
            workspace_root.display()
        )
    })?;
    Ok(if relative.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        relative.to_path_buf()
    })
}

fn is_excluded(relative: &Path, excludes: &[Pattern]) -> bool {
    excludes
        .iter()
        .any(|exclude| exclude.matches_path_with(relative, match_options()))
}

fn dependency_names(pyproject: &PyProject) -> BTreeSet<String> {
    let Some(project) = pyproject.project.as_ref() else {
        return BTreeSet::new();
    };
    project
        .dependencies
        .iter()
        .map(String::as_str)
        .chain(
            project
                .optional_dependencies
                .values()
                .flatten()
                .map(String::as_str),
        )
        .chain(
            pyproject
                .tool
                .uv
                .dev_dependencies
                .iter()
                .map(String::as_str),
        )
        .chain(
            pyproject
                .dependency_groups
                .values()
                .flatten()
                .filter_map(toml::Value::as_str),
        )
        .filter_map(requirement_name)
        .collect()
}

fn requirement_name(requirement: &str) -> Option<String> {
    let name = requirement
        .trim_start()
        .chars()
        .take_while(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
        .collect::<String>();
    (!name.is_empty()).then(|| normalize_package_name(&name))
}

fn normalize_package_name(name: &str) -> String {
    let mut normalized = String::new();
    let mut separator = false;
    for character in name.chars() {
        if matches!(character, '-' | '_' | '.') {
            separator = true;
        } else {
            if separator && !normalized.is_empty() {
                normalized.push('-');
            }
            separator = false;
            normalized.extend(character.to_lowercase());
        }
    }
    normalized
}

fn effective_source<'a>(
    pyproject: &'a PyProject,
    root_pyproject: &'a PyProject,
    dependency: &str,
    workspace_member: bool,
    project_root: &'a Path,
    workspace_root: &'a Path,
) -> Option<(&'a toml::Value, &'a Path)> {
    if let Some(source) = find_source(&pyproject.tool.uv.sources, dependency) {
        return Some((source, project_root));
    }
    (workspace_member && project_root != workspace_root)
        .then(|| find_source(&root_pyproject.tool.uv.sources, dependency))
        .flatten()
        .map(|source| (source, workspace_root))
}

fn find_source<'a>(
    sources: &'a BTreeMap<String, toml::Value>,
    dependency: &str,
) -> Option<&'a toml::Value> {
    sources
        .iter()
        .find(|(name, _)| normalize_package_name(name) == dependency)
        .map(|(_, source)| source)
}

fn source_tables(source: &toml::Value) -> Vec<&toml::Table> {
    if let Some(table) = source.as_table() {
        vec![table]
    } else {
        source
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(toml::Value::as_table)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    fn write(path: &Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn project(path: &Path, name: &str, extra: &str) {
        write(
            &path.join(PYPROJECT_TOML),
            &format!("[project]\nname = {name:?}\nversion = \"0.1.0\"\n{extra}"),
        );
    }

    #[test]
    fn discovers_root_members_and_excludes_with_normalized_ids() {
        let temp = tempdir().unwrap();
        project(
            temp.path(),
            "Root_Project",
            "[tool.uv.workspace]\nmembers = [\"packages/*\"]\nexclude = [\"packages/skip\"]\n",
        );
        project(&temp.path().join("packages/api"), "My.API", "");
        project(&temp.path().join("packages/skip"), "skip", "");

        let projects = UvWorkspaceProvider.discover(temp.path()).unwrap();
        let summary = projects
            .iter()
            .map(|project| (project.id.as_str(), project.root.as_path()))
            .collect::<Vec<_>>();

        assert_eq!(
            summary,
            vec![
                ("uv:root-project", Path::new(".")),
                ("uv:my-api", Path::new("packages/api"))
            ]
        );
    }

    #[test]
    fn supports_non_package_workspace_roots() {
        let temp = tempdir().unwrap();
        project(
            temp.path(),
            "root",
            "[tool.uv]\npackage = false\n[tool.uv.workspace]\nmembers = []\n",
        );

        let projects = UvWorkspaceProvider.discover(temp.path()).unwrap();

        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].id.as_str(), "uv:root");
        assert_eq!(projects[0].root, Path::new("."));
    }

    #[test]
    fn supports_virtual_workspace_roots_without_a_project_table() {
        let temp = tempdir().unwrap();
        write(
            &temp.path().join(PYPROJECT_TOML),
            "[tool.uv.workspace]\nmembers = [\"packages/*\"]\n",
        );
        project(&temp.path().join("packages/api"), "api", "");

        let projects = UvWorkspaceProvider.discover(temp.path()).unwrap();

        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].id.as_str(), "uv:api");
        assert_eq!(projects[0].root, Path::new("packages/api"));
    }

    #[test]
    fn member_globs_ignore_regular_files() {
        let temp = tempdir().unwrap();
        project(
            temp.path(),
            "root",
            "[tool.uv.workspace]\nmembers = [\"packages/*\"]\n",
        );
        project(&temp.path().join("packages/api"), "api", "");
        write(&temp.path().join("packages/README.md"), "not a project");

        let projects = UvWorkspaceProvider.discover(temp.path()).unwrap();
        let ids = projects
            .iter()
            .map(|project| project.id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(ids, vec!["uv:root", "uv:api"]);
    }

    #[test]
    fn infers_workspace_inherited_and_path_dependencies() {
        let temp = tempdir().unwrap();
        project(
            temp.path(),
            "root",
            "dependencies = [\"Shared.Lib\"]\n\
             [tool.uv.sources]\nshared-lib = { workspace = true }\ninherited = { path = \"packages/inherited\" }\n\
             [tool.uv.workspace]\nmembers = [\"packages/*\"]\nexclude = [\"packages/local\", \"packages/inherited\"]\n",
        );
        project(
            &temp.path().join("packages/app"),
            "app",
            "dependencies = [\"shared_lib[fast]>=1\", \"inherited\"]\n\
             [project.optional-dependencies]\ntest = [\"local-helper\"]\n\
             [dependency-groups]\ndev = [\"build.helper\", { include-group = \"lint\" }]\nlint = [\"lint-helper\"]\n\
             [tool.uv.sources]\nlocal-helper = { path = \"../local\" }\nlint-helper = { path = \"../lint\" }\n\
             build-helper = [{ path = \"../build\", marker = \"sys_platform == 'linux'\" }]\n",
        );
        project(&temp.path().join("packages/shared"), "shared-lib", "");
        project(&temp.path().join("packages/local"), "local-helper", "");
        project(&temp.path().join("packages/lint"), "lint-helper", "");
        project(&temp.path().join("packages/build"), "build-helper", "");
        project(&temp.path().join("packages/inherited"), "inherited", "");

        let projects = UvWorkspaceProvider.discover(temp.path()).unwrap();
        let app = projects
            .iter()
            .find(|project| project.id.as_str() == "uv:app")
            .unwrap();

        assert_eq!(
            app.dependencies,
            [
                "build-helper",
                "inherited",
                "lint-helper",
                "local-helper",
                "shared-lib"
            ]
            .into_iter()
            .map(|name| ProjectId::new("uv", name).unwrap())
            .collect()
        );
        assert_eq!(projects.len(), 7);
    }

    #[test]
    fn member_sources_override_root_sources() {
        let temp = tempdir().unwrap();
        project(
            temp.path(),
            "root",
            "[tool.uv.sources]\nhelper = { workspace = true }\n\
             [tool.uv.workspace]\nmembers = [\"packages/*\"]\n",
        );
        project(
            &temp.path().join("packages/app"),
            "app",
            "dependencies = [\"helper\"]\n\
             [tool.uv.sources]\nhelper = { index = \"private\" }\n",
        );
        project(&temp.path().join("packages/helper"), "helper", "");

        let projects = UvWorkspaceProvider.discover(temp.path()).unwrap();
        let app = projects
            .iter()
            .find(|project| project.id.as_str() == "uv:app")
            .unwrap();

        assert!(app.dependencies.is_empty());
    }

    #[test]
    fn ignores_projects_without_a_uv_workspace() {
        let temp = tempdir().unwrap();
        project(temp.path(), "standalone", "");

        assert!(
            UvWorkspaceProvider
                .discover(temp.path())
                .unwrap()
                .is_empty()
        );
    }
}
