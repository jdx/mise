use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use eyre::{Context, Result, bail};
use glob::{MatchOptions, Pattern};
use serde::Deserialize;

use super::{ProjectId, WorkspaceProject, WorkspaceProvenance, WorkspaceProvider};

const CARGO_TOML: &str = "Cargo.toml";

/// Discovers Cargo packages from a workspace manifest.
#[derive(Debug, Default)]
pub struct CargoWorkspaceProvider;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct CargoManifest {
    package: Option<CargoPackage>,
    workspace: Option<CargoWorkspace>,
    dependencies: DependencyMap,
    #[serde(rename = "dev-dependencies")]
    dev_dependencies: DependencyMap,
    #[serde(rename = "build-dependencies")]
    build_dependencies: DependencyMap,
    target: BTreeMap<String, TargetDependencies>,
}

#[derive(Debug, Deserialize)]
struct CargoPackage {
    name: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct CargoWorkspace {
    members: Vec<String>,
    exclude: Vec<String>,
    dependencies: DependencyMap,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct TargetDependencies {
    dependencies: DependencyMap,
    #[serde(rename = "dev-dependencies")]
    dev_dependencies: DependencyMap,
    #[serde(rename = "build-dependencies")]
    build_dependencies: DependencyMap,
}

type DependencyMap = BTreeMap<String, toml::Value>;

struct PackageManifest {
    root: PathBuf,
    canonical_root: PathBuf,
    manifest: CargoManifest,
    id: ProjectId,
}

impl WorkspaceProvider for CargoWorkspaceProvider {
    fn id(&self) -> &str {
        "cargo"
    }

    fn discover(&self, workspace_root: &Path) -> Result<Vec<WorkspaceProject>> {
        let root_manifest_path = workspace_root.join(CARGO_TOML);
        if !root_manifest_path.is_file() {
            return Ok(Vec::new());
        }
        let root_manifest = read_manifest(&root_manifest_path)?;
        let Some(workspace) = root_manifest.workspace.as_ref() else {
            return Ok(Vec::new());
        };
        let canonical_root = workspace_root.canonicalize().wrap_err_with(|| {
            format!(
                "failed to resolve Cargo workspace root {}",
                workspace_root.display()
            )
        })?;
        let excludes = workspace
            .exclude
            .iter()
            .map(|exclude| {
                Pattern::new(exclude).wrap_err_with(|| {
                    format!("invalid Cargo workspace exclude pattern {exclude:?}")
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let mut roots = discover_members(
            workspace_root,
            &canonical_root,
            &workspace.members,
            &excludes,
        )?;
        if root_manifest.package.is_some() && !is_excluded(Path::new("."), &excludes) {
            roots.insert(canonical_root.clone());
        }

        // Cargo implicitly treats path dependencies below the workspace root as members.
        let mut queue = roots.iter().cloned().collect::<VecDeque<_>>();
        let mut manifests = BTreeMap::new();
        while let Some(package_root) = queue.pop_front() {
            if manifests.contains_key(&package_root) {
                continue;
            }
            let relative = relative_root(&canonical_root, &package_root)?;
            let manifest_path = package_root.join(CARGO_TOML);
            let manifest = if package_root == canonical_root {
                read_manifest(&root_manifest_path)?
            } else {
                read_manifest(&manifest_path)?
            };
            let package = manifest.package.as_ref().ok_or_else(|| {
                eyre::eyre!(
                    "Cargo workspace member at {} is missing [package]",
                    relative.display()
                )
            })?;
            let id = ProjectId::new(self.id(), &package.name)?;

            for (dependency, base) in
                dependency_values(&manifest, workspace, &package_root, &canonical_root)
            {
                let Some(path) = dependency_path(dependency) else {
                    continue;
                };
                let candidate = base.join(path);
                if !candidate.join(CARGO_TOML).is_file() {
                    continue;
                }
                let candidate = candidate.canonicalize().wrap_err_with(|| {
                    format!(
                        "failed to resolve Cargo path dependency {}",
                        candidate.display()
                    )
                })?;
                let Ok(relative) = candidate.strip_prefix(&canonical_root) else {
                    continue;
                };
                let relative = normalize_relative(relative);
                if !is_excluded(&relative, &excludes) && roots.insert(candidate.clone()) {
                    queue.push_back(candidate);
                }
            }

            manifests.insert(
                package_root.clone(),
                PackageManifest {
                    root: relative,
                    canonical_root: package_root,
                    manifest,
                    id,
                },
            );
        }

        let ids_by_root = manifests
            .values()
            .map(|package| (package.canonical_root.clone(), package.id.clone()))
            .collect::<BTreeMap<_, _>>();

        manifests
            .into_values()
            .map(|package| {
                let source = package.root.join(CARGO_TOML);
                let mut dependencies = BTreeSet::new();
                for (dependency, base) in dependency_values(
                    &package.manifest,
                    workspace,
                    &package.canonical_root,
                    &canonical_root,
                ) {
                    let Some(path) = dependency_path(dependency) else {
                        continue;
                    };
                    let candidate = base.join(path);
                    if !candidate.join(CARGO_TOML).is_file() {
                        continue;
                    }
                    let candidate = candidate.canonicalize().wrap_err_with(|| {
                        format!(
                            "failed to resolve Cargo path dependency {}",
                            candidate.display()
                        )
                    })?;
                    if let Some(dependency_id) = ids_by_root.get(&candidate)
                        && dependency_id != &package.id
                    {
                        dependencies.insert(dependency_id.clone());
                    }
                }

                let provenance = WorkspaceProvenance {
                    provider: Some(self.id().to_string()),
                    source: Some(source.clone()),
                };
                let mut project = WorkspaceProject::new(package.id, package.root);
                project.dependency_provenance = dependencies
                    .iter()
                    .cloned()
                    .map(|dependency| (dependency, provenance.clone()))
                    .collect();
                project.dependencies = dependencies;
                project.provenance = provenance;
                project
                    .metadata
                    .insert("workspace_source".to_string(), CARGO_TOML.to_string());
                Ok(project)
            })
            .collect()
    }
}

fn read_manifest(path: &Path) -> Result<CargoManifest> {
    let contents = fs::read_to_string(path)
        .wrap_err_with(|| format!("failed to read Cargo manifest {}", path.display()))?;
    toml::from_str(&contents)
        .wrap_err_with(|| format!("failed to parse Cargo manifest {}", path.display()))
}

fn discover_members(
    pattern_root: &Path,
    canonical_root: &Path,
    members: &[String],
    excludes: &[Pattern],
) -> Result<BTreeSet<PathBuf>> {
    let options = MatchOptions {
        case_sensitive: true,
        require_literal_separator: true,
        require_literal_leading_dot: true,
    };
    // Use the caller's non-canonical path for globbing. Windows canonical paths
    // use a verbatim `\\?\` prefix whose `?` is interpreted as a glob token.
    let escaped_root = Pattern::escape(&pattern_root.to_string_lossy());
    let mut roots = BTreeSet::new();
    for member in members {
        if Path::new(member).is_absolute() {
            bail!("Cargo workspace member pattern {member:?} must be relative");
        }
        let pattern = format!("{escaped_root}/{member}");
        for candidate in glob::glob_with(&pattern, options)
            .wrap_err_with(|| format!("invalid Cargo workspace member pattern {member:?}"))?
        {
            let candidate = candidate.wrap_err_with(|| {
                format!("failed to evaluate Cargo workspace member pattern {member:?}")
            })?;
            if !candidate.join(CARGO_TOML).is_file() {
                continue;
            }
            let candidate = candidate.canonicalize().wrap_err_with(|| {
                format!(
                    "failed to resolve Cargo workspace member {}",
                    candidate.display()
                )
            })?;
            let relative = relative_root(canonical_root, &candidate)?;
            if !is_excluded(&relative, excludes) {
                roots.insert(candidate);
            }
        }
    }
    Ok(roots)
}

fn relative_root(workspace_root: &Path, package_root: &Path) -> Result<PathBuf> {
    let relative = package_root.strip_prefix(workspace_root).map_err(|_| {
        eyre::eyre!(
            "Cargo workspace member {} is outside workspace root {}",
            package_root.display(),
            workspace_root.display()
        )
    })?;
    Ok(normalize_relative(relative))
}

fn normalize_relative(path: &Path) -> PathBuf {
    if path.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        path.to_path_buf()
    }
}

fn is_excluded(relative: &Path, excludes: &[Pattern]) -> bool {
    let options = MatchOptions {
        case_sensitive: true,
        require_literal_separator: true,
        require_literal_leading_dot: true,
    };
    relative.ancestors().any(|candidate| {
        excludes
            .iter()
            .any(|exclude| exclude.matches_path_with(candidate, options))
    })
}

fn dependency_values<'a>(
    manifest: &'a CargoManifest,
    workspace: &'a CargoWorkspace,
    package_root: &'a Path,
    workspace_root: &'a Path,
) -> Vec<(&'a toml::Value, &'a Path)> {
    manifest
        .dependencies
        .iter()
        .chain(&manifest.dev_dependencies)
        .chain(&manifest.build_dependencies)
        .chain(manifest.target.values().flat_map(|target| {
            target
                .dependencies
                .iter()
                .chain(&target.dev_dependencies)
                .chain(&target.build_dependencies)
        }))
        .filter_map(|(name, dependency)| {
            if dependency
                .as_table()
                .and_then(|table| table.get("workspace"))
                .and_then(toml::Value::as_bool)
                == Some(true)
            {
                workspace
                    .dependencies
                    .get(name)
                    .map(|dependency| (dependency, workspace_root))
            } else {
                Some((dependency, package_root))
            }
        })
        .collect()
}

fn dependency_path(dependency: &toml::Value) -> Option<&Path> {
    dependency.as_table()?.get("path")?.as_str().map(Path::new)
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

    fn package(path: &Path, name: &str, extra: &str) {
        write(
            &path.join(CARGO_TOML),
            &format!("[package]\nname = {name:?}\nversion = \"0.1.0\"\n{extra}"),
        );
    }

    #[test]
    fn discovers_members_root_package_and_excludes() {
        let temp = tempdir().unwrap();
        write(
            &temp.path().join(CARGO_TOML),
            "[package]\nname = \"root\"\nversion = \"0.1.0\"\n\
             [workspace]\nmembers = [\"crates/*\", \"crates/*/*\"]\nexclude = [\"crates/skip\"]\n",
        );
        package(&temp.path().join("crates/app"), "app", "");
        package(&temp.path().join("crates/skip"), "skip", "");
        package(&temp.path().join("crates/skip/nested"), "nested", "");

        let projects = CargoWorkspaceProvider.discover(temp.path()).unwrap();
        let summary = projects
            .iter()
            .map(|project| (project.id.as_str(), project.root.as_path()))
            .collect::<Vec<_>>();

        assert_eq!(
            summary,
            vec![
                ("cargo:root", Path::new(".")),
                ("cargo:app", Path::new("crates/app"))
            ]
        );
        assert!(projects.iter().all(|project| {
            project.metadata.get("workspace_source").map(String::as_str) == Some(CARGO_TOML)
        }));
    }

    #[test]
    fn infers_all_path_dependency_kinds_and_implicit_members() {
        let temp = tempdir().unwrap();
        write(
            &temp.path().join(CARGO_TOML),
            "[workspace]\nmembers = [\"crates/app\", \"crates/core\"]\n\
             [workspace.dependencies]\nshared = { path = \"crates/shared\" }\n",
        );
        package(
            &temp.path().join("crates/app"),
            "app",
            "[dependencies]\nrenamed = { package = \"core\", path = \"../core\" }\n\
             [dev-dependencies]\nshared = { workspace = true }\n\
             [build-dependencies]\nbuild-helper = { path = \"../build-helper\" }\n\
             [target.'cfg(unix)'.dependencies]\ntarget-helper = { path = \"../target-helper\" }\n\
             [target.'cfg(windows)'.dev-dependencies]\nexternal = \"1\"\n",
        );
        for name in ["core", "shared", "build-helper", "target-helper"] {
            package(&temp.path().join(format!("crates/{name}")), name, "");
        }

        let projects = CargoWorkspaceProvider.discover(temp.path()).unwrap();
        let app = projects
            .iter()
            .find(|project| project.id.as_str() == "cargo:app")
            .unwrap();

        assert_eq!(
            app.dependencies,
            ["build-helper", "core", "shared", "target-helper"]
                .into_iter()
                .map(|name| ProjectId::new("cargo", name).unwrap())
                .collect()
        );
        assert_eq!(projects.len(), 5);
        assert!(app.dependencies.iter().all(|dependency| {
            app.dependency_provenance[dependency].source
                == Some(PathBuf::from("crates/app/Cargo.toml"))
        }));
    }

    #[test]
    fn ignores_manifests_without_a_workspace() {
        let temp = tempdir().unwrap();
        package(temp.path(), "standalone", "");

        assert!(
            CargoWorkspaceProvider
                .discover(temp.path())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn rejects_members_outside_the_workspace() {
        let temp = tempdir().unwrap();
        let outside = tempdir().unwrap();
        package(outside.path(), "outside", "");
        write(
            &temp.path().join(CARGO_TOML),
            &format!(
                "[workspace]\nmembers = [{:?}]\n",
                outside.path().display().to_string()
            ),
        );

        let error = CargoWorkspaceProvider.discover(temp.path()).unwrap_err();
        assert!(error.to_string().contains("must be relative"));
    }
}
