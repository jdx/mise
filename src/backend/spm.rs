use crate::backend::Backend;
use crate::backend::backend_type::BackendType;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::git::{CloneOptions, Git};
use crate::install_context::InstallContext;
use crate::toolset::ToolVersion;
use crate::{dirs, file, github};
use async_trait::async_trait;
use eyre::WrapErr;
use serde::Deserializer;
use serde::de::{MapAccess, Visitor};
use serde_derive::Deserialize;
use std::path::PathBuf;
use std::{
    fmt::{self, Debug},
    sync::Arc,
};
use url::Url;
use xx::regex;

#[derive(Debug)]
pub struct SPMBackend {
    ba: Arc<BackendArg>,
}

#[async_trait]
impl Backend for SPMBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Spm
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    fn get_dependencies(&self) -> eyre::Result<Vec<&str>> {
        Ok(vec!["swift"])
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> eyre::Result<Vec<String>> {
        let repo = SwiftPackageRepo::new(&self.tool_name())?;
        Ok(github::list_releases(repo.shorthand.as_str())
            .await?
            .into_iter()
            .map(|r| r.tag_name)
            .rev()
            .collect())
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        tv: ToolVersion,
    ) -> eyre::Result<ToolVersion> {
        let settings = Settings::get();
        settings.ensure_experimental("spm backend")?;

        let repo = SwiftPackageRepo::new(&self.tool_name())?;
        let revision = if tv.version == "latest" {
            self.latest_stable_version(&ctx.config)
                .await?
                .ok_or_else(|| eyre::eyre!("No stable versions found"))?
        } else {
            tv.version.clone()
        };
        let repo_dir = self.clone_package_repo(ctx, &tv, &repo, &revision)?;

        let executables = self.get_executable_names(ctx, &repo_dir, &tv).await?;
        if executables.is_empty() {
            return Err(eyre::eyre!("No executables found in the package"));
        }
        let bin_path = tv.install_path().join("bin");
        file::create_dir_all(&bin_path)?;
        for executable in executables {
            let exe_path = self
                .build_executable(&executable, &repo_dir, ctx, &tv)
                .await?;
            file::make_symlink(&exe_path, &bin_path.join(executable))?;
        }

        // delete (huge) intermediate artifacts
        file::remove_all(tv.install_path().join("repositories"))?;
        file::remove_all(tv.cache_path())?;

        Ok(tv)
    }
}

impl SPMBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self { ba: Arc::new(ba) }
    }

    fn clone_package_repo(
        &self,
        ctx: &InstallContext,
        tv: &ToolVersion,
        package_repo: &SwiftPackageRepo,
        revision: &str,
    ) -> Result<PathBuf, eyre::Error> {
        let repo = Git::new(tv.cache_path().join("repo"));
        if !repo.exists() {
            debug!(
                "Cloning swift package repo {} to {}",
                package_repo.url.as_str(),
                repo.dir.display(),
            );
            repo.clone(
                package_repo.url.as_str(),
                CloneOptions::default().pr(&ctx.pr),
            )?;
        }
        debug!("Checking out revision: {revision}");
        repo.update_tag(revision.to_string())?;

        Ok(repo.dir)
    }

    async fn get_executable_names(
        &self,
        ctx: &InstallContext,
        repo_dir: &PathBuf,
        tv: &ToolVersion,
    ) -> Result<Vec<String>, eyre::Error> {
        let package_json = cmd!(
            "swift",
            "package",
            "dump-package",
            "--package-path",
            &repo_dir,
            "--scratch-path",
            tv.install_path(),
            "--cache-path",
            dirs::CACHE.join("spm"),
        )
        .full_env(self.dependency_env(&ctx.config).await?)
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

    async fn build_executable(
        &self,
        executable: &str,
        repo_dir: &PathBuf,
        ctx: &InstallContext,
        tv: &ToolVersion,
    ) -> Result<PathBuf, eyre::Error> {
        debug!("Building swift package");
        CmdLineRunner::new("swift")
            .arg("build")
            .arg("--configuration")
            .arg("release")
            .arg("--product")
            .arg(executable)
            .arg("--scratch-path")
            .arg(tv.install_path())
            .arg("--package-path")
            .arg(repo_dir)
            .arg("--cache-path")
            .arg(dirs::CACHE.join("spm"))
            .with_pr(&ctx.pr)
            .prepend_path(
                self.dependency_toolset(&ctx.config)
                    .await?
                    .list_paths(&ctx.config)
                    .await,
            )?
            .execute()?;

        let bin_path = cmd!(
            "swift",
            "build",
            "--configuration",
            "release",
            "--product",
            &executable,
            "--package-path",
            &repo_dir,
            "--scratch-path",
            tv.install_path(),
            "--cache-path",
            dirs::CACHE.join("spm"),
            "--show-bin-path"
        )
        .full_env(self.dependency_env(&ctx.config).await?)
        .read()?;
        Ok(PathBuf::from(bin_path.trim().to_string()).join(executable))
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
        let name = name.strip_prefix("spm:").unwrap_or(name);
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
            Err(eyre::eyre!(
                "Invalid Swift package repository: {}. The repository should either be a GitHub repository slug, owner/name, or the complete URL, https://github.com/owner/name.",
                name
            ))?
        };
        let url_str = format!("https://github.com/{shorthand}.git");
        let url = Url::parse(&url_str)?;

        Ok(Self {
            url,
            shorthand: shorthand.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::config::Config;

    use super::*;
    use pretty_assertions::assert_str_eq;

    #[tokio::test]
    async fn test_spm_repo_init_by_shorthand() {
        let _config = Config::get().await.unwrap();
        let package_name = "nicklockwood/SwiftFormat";
        let package_repo = SwiftPackageRepo::new(package_name).unwrap();
        assert_str_eq!(
            package_repo.url.as_str(),
            "https://github.com/nicklockwood/SwiftFormat.git"
        );
        assert_str_eq!(package_repo.shorthand, "nicklockwood/SwiftFormat");
    }

    #[tokio::test]
    async fn test_spm_repo_init_name() {
        let _config = Config::get().await.unwrap();
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

    #[tokio::test]
    async fn test_spm_repo_init_by_url() {
        let _config = Config::get().await.unwrap();
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

    /// Swift determines the toolchain to use with a given package using a comment in the Package.swift file at the top.
    /// For example:
    ///   // swift-tools-version: 6.0
    ///
    /// The version of the toolchain can be older than the Swift version used to build the package. This versioning gives
    /// Apple the flexibility to introduce and flag breaking changes in the toolchain.
    ///
    /// How to determine the product type is something that might change across different versions of Swift.
    ///
    /// ## Swift 5.x
    ///
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
    ///
    /// ## Swift 6.x
    ///
    /// The product type is directly the value under the key "type"
    ///
    /// Example:
    ///
    /// ```json
    /// "type": "executable"
    /// ```
    ///
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
                        "type" => {
                            let value: String = map.next_value()?;
                            if value == "executable" {
                                Ok(PackageDescriptionProductType::Executable)
                            } else {
                                Ok(PackageDescriptionProductType::Other)
                            }
                        }
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
