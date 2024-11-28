use crate::backend::backend_type::BackendType;
use crate::backend::Backend;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::Settings;
use crate::git::Git;
use crate::install_context::InstallContext;
use crate::toolset::ToolVersion;
use crate::{file, github};
use eyre::WrapErr;
use serde::de::{MapAccess, Visitor};
use serde::Deserializer;
use serde_derive::Deserialize;
use std::fmt::{self, Debug};
use std::path::PathBuf;
use url::Url;
use walkdir::WalkDir;
use xx::regex;

#[derive(Debug)]
pub struct SPMBackend {
    ba: BackendArg,
}

// https://github.com/apple/swift-package-manager
impl Backend for SPMBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Spm
    }

    fn ba(&self) -> &BackendArg {
        &self.ba
    }

    fn get_dependencies(&self) -> eyre::Result<Vec<&str>> {
        // TODO: swift as dependencies (wait for swift core plugin: https://github.com/jdx/mise/pull/1708)
        Ok(vec![])
    }

    fn _list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        let repo = SwiftPackageRepo::new(&self.tool_name())?;
        Ok(github::list_releases(repo.shorthand.as_str())?
            .into_iter()
            .map(|r| r.tag_name)
            .rev()
            .collect())
    }

    fn install_version_impl(
        &self,
        ctx: &InstallContext,
        tv: ToolVersion,
    ) -> eyre::Result<ToolVersion> {
        let settings = Settings::get();
        settings.ensure_experimental("spm backend")?;

        let repo = SwiftPackageRepo::new(&self.tool_name())?;
        let revision = if tv.version == "latest" {
            self.latest_stable_version()?
                .ok_or_else(|| eyre::eyre!("No stable versions found"))?
        } else {
            tv.version.clone()
        };
        let repo_dir = self.clone_package_repo(ctx, &tv, &repo, &revision)?;

        let executables = self.get_executable_names(&repo_dir)?;
        if executables.is_empty() {
            return Err(eyre::eyre!("No executables found in the package"));
        }
        for executable in executables {
            let bin_path = self.build_executable(&executable, &repo_dir, ctx)?;
            let install_bin_path = tv.install_path().join("bin");
            self.copy_build_artifacts(&executable, &bin_path, &install_bin_path)?;
        }

        debug!("Cleaning up temporary files");
        file::remove_all(&repo_dir)?;

        Ok(tv)
    }
}

impl SPMBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self { ba }
    }

    fn clone_package_repo(
        &self,
        ctx: &InstallContext,
        tv: &ToolVersion,
        package_repo: &SwiftPackageRepo,
        revision: &str,
    ) -> Result<PathBuf, eyre::Error> {
        let repo = Git::new(&tv.cache_path().join(package_repo.dir_name(revision)));
        if !repo.exists() {
            debug!(
                "Cloning swift package repo {} to {}",
                package_repo.url.as_str(),
                repo.dir.display(),
            );
            repo.clone(package_repo.url.as_str(), Some(ctx.pr.as_ref()))?;
        }
        debug!("Checking out revision: {revision}");
        repo.update(Some(revision.to_string()))?;

        Ok(repo.dir)
    }

    fn get_executable_names(&self, repo_dir: &PathBuf) -> Result<Vec<String>, eyre::Error> {
        let package_json = cmd!(
            "swift",
            "package",
            "dump-package",
            "--package-path",
            &repo_dir
        )
        .full_env(self.dependency_env()?)
        .read()?;
        let executables = serde_json::from_str::<PackageDescription>(&package_json)
            .wrap_err("Failed to parse package description")?
            .products
            .iter()
            .filter(|p| p.r#type.is_executable())
            .map(|p| p.name.clone())
            .collect::<Vec<String>>();
        debug!("Found executables: {:?}", executables);
        Ok(executables)
    }

    fn build_executable(
        &self,
        executable: &str,
        repo_dir: &PathBuf,
        ctx: &InstallContext<'_>,
    ) -> Result<PathBuf, eyre::Error> {
        debug!("Building swift package");
        let build_cmd = CmdLineRunner::new("swift")
            .arg("build")
            .arg("--configuration")
            .arg("release")
            .arg("--product")
            .arg(executable)
            .arg("--package-path")
            .arg(repo_dir)
            .with_pr(ctx.pr.as_ref())
            .prepend_path(self.dependency_toolset()?.list_paths())?;
        build_cmd.execute()?;
        let bin_path = cmd!(
            "swift",
            "build",
            "--configuration",
            "release",
            "--product",
            &executable,
            "--package-path",
            &repo_dir,
            "--show-bin-path"
        )
        .full_env(self.dependency_env()?)
        .read()?;
        Ok(PathBuf::from(bin_path.trim().to_string()))
    }

    fn copy_build_artifacts(
        &self,
        executable: &str,
        bin_path: &PathBuf,
        install_bin_path: &PathBuf,
    ) -> Result<(), eyre::Error> {
        debug!(
            "Copying binaries to install path: {}",
            install_bin_path.display()
        );
        file::create_dir_all(install_bin_path)?;
        file::copy(bin_path.join(executable), install_bin_path.join(executable))?;
        WalkDir::new(bin_path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| {
                let ext = e.path().extension().unwrap_or_default();
                // TODO: support other platforms extensions
                ext == "dylib" || ext == "bundle"
            })
            .try_for_each(|e| -> Result<(), eyre::Error> {
                let rel_path = e.path().strip_prefix(bin_path)?;
                let install_path = install_bin_path.join(rel_path);
                file::create_dir_all(install_path.parent().unwrap())?;
                if e.path().is_dir() {
                    file::copy_dir_all(e.path(), &install_path)?;
                } else {
                    file::copy(e.path(), &install_path)?;
                }
                Ok(())
            })?;
        Ok(())
    }
}

#[derive(Debug)]
struct SwiftPackageRepo {
    /// https://github.com/owner/repo.git
    url: Url,
    /// owner/repo_name
    shorthand: String,
}

impl SwiftPackageRepo {
    /// Parse the slug or the full URL of a GitHub package repository.
    fn new(name: &str) -> Result<Self, eyre::Error> {
        let shorthand_regex = regex!(r"^[a-zA-Z0-9_-]+/[a-zA-Z0-9._-]+$");
        let shorthand_in_url_regex =
            regex!(r"https://github.com/([a-zA-Z0-9_-]+/[a-zA-Z0-9._-]+)\.git");

        let shorthand = if let Some(Some(m)) =
            shorthand_in_url_regex.captures(name).map(|c| c.get(1))
        {
            m.as_str()
        } else if shorthand_regex.is_match(name) {
            name
        } else {
            Err(eyre::eyre!("Invalid Swift package repository: {}. The repository should either be a GitHub repository slug, owner/name, or the complete URL, https://github.com/owner/name.", name))?
        };
        let url_str = format!("https://github.com/{}.git", shorthand);
        let url = Url::parse(&url_str)?;

        Ok(Self {
            url,
            shorthand: shorthand.to_string(),
        })
    }

    fn dir_name(&self, revision: &str) -> String {
        self.shorthand.replace('/', "-") + "@" + revision
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_str_eq;
    use test_log::test;

    #[test]
    fn test_spm_repo_init_by_shorthand() {
        let package_name = "nicklockwood/SwiftFormat";
        let package_repo = SwiftPackageRepo::new(package_name).unwrap();
        assert_str_eq!(
            package_repo.url.as_str(),
            "https://github.com/nicklockwood/SwiftFormat.git"
        );
        assert_str_eq!(package_repo.shorthand, "nicklockwood/SwiftFormat");
    }

    #[test]
    fn test_spm_repo_init_name() {
        assert!(
            SwiftPackageRepo::new("owner/name.swift").is_ok(),
            "name part can contain ."
        );
        assert!(
            SwiftPackageRepo::new("owner/name_swift").is_ok(),
            "name part can contain _"
        );
        assert!(
            SwiftPackageRepo::new("owner/name-swift").is_ok(),
            "name part can contain -"
        );
        assert!(
            SwiftPackageRepo::new("owner/name$swift").is_err(),
            "name part cannot contain characters other than a-zA-Z0-9._-"
        );
    }

    #[test]
    fn test_spm_repo_init_by_url() {
        let package_name = "https://github.com/nicklockwood/SwiftFormat.git";
        let package_repo = SwiftPackageRepo::new(package_name).unwrap();
        assert_str_eq!(
            package_repo.url.as_str(),
            "https://github.com/nicklockwood/SwiftFormat.git"
        );
        assert_str_eq!(package_repo.shorthand, "nicklockwood/SwiftFormat");
    }
}

/// https://developer.apple.com/documentation/packagedescription
#[derive(Deserialize)]
struct PackageDescription {
    products: Vec<PackageDescriptionProduct>,
}

#[derive(Deserialize)]
struct PackageDescriptionProduct {
    name: String,
    #[serde(deserialize_with = "PackageDescriptionProductType::deserialize_product_type_field")]
    r#type: PackageDescriptionProductType,
}

#[derive(Deserialize)]
enum PackageDescriptionProductType {
    Executable,
    Other,
}

impl PackageDescriptionProductType {
    fn is_executable(&self) -> bool {
        matches!(self, Self::Executable)
    }

    /// Product type is a key in the map with an undocumented value that we are not interested in and can be easily skipped.
    ///
    /// Example:
    /// ```json
    /// "type" : {
    ///     "executable" : null
    /// }
    /// ```
    /// or
    /// ```json
    /// "type" : {
    ///     "library" : [
    ///       "automatic"
    ///     ]
    /// }
    /// ```
    fn deserialize_product_type_field<'de, D>(
        deserializer: D,
    ) -> Result<PackageDescriptionProductType, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct TypeFieldVisitor;

        impl<'de> Visitor<'de> for TypeFieldVisitor {
            type Value = PackageDescriptionProductType;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a map with a key 'executable' or other types")
            }

            fn visit_map<V>(self, mut map: V) -> Result<Self::Value, V::Error>
            where
                V: MapAccess<'de>,
            {
                if let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "executable" => {
                            // Skip the value by reading it into a dummy serde_json::Value
                            let _value: serde_json::Value = map.next_value()?;
                            Ok(PackageDescriptionProductType::Executable)
                        }
                        _ => {
                            let _value: serde_json::Value = map.next_value()?;
                            Ok(PackageDescriptionProductType::Other)
                        }
                    }
                } else {
                    Err(serde::de::Error::custom("missing key"))
                }
            }
        }

        deserializer.deserialize_map(TypeFieldVisitor)
    }
}
