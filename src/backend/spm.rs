use crate::backend::Backend;
use crate::backend::backend_type::BackendType;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::git::{CloneOptions, Git};
use crate::install_context::InstallContext;
use crate::toolset::ToolVersion;
use crate::{dirs, file, github, gitlab};
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
use strum::{AsRefStr, EnumString, VariantNames};
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
        let provider = GitProvider::from_ba(&self.ba);
        let repo = SwiftPackageRepo::new(&self.tool_name(), &provider)?;
        let releases = match provider.kind {
            GitProviderKind::GitLab => {
                gitlab::list_releases_from_url(&provider.api_url, repo.shorthand.as_str())
                    .await?
                    .into_iter()
                    .map(|r| r.tag_name)
                    .rev()
                    .collect()
            }
            _ => github::list_releases_from_url(&provider.api_url, repo.shorthand.as_str())
                .await?
                .into_iter()
                .map(|r| r.tag_name)
                .rev()
                .collect(),
        };

        Ok(releases)
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        tv: ToolVersion,
    ) -> eyre::Result<ToolVersion> {
        let settings = Settings::get();
        settings.ensure_experimental("spm backend")?;

        // Check if swift is available
        self.warn_if_dependency_missing(
            &ctx.config,
            "swift",
            "To use Swift Package Manager (spm) tools with mise, you need to install Swift first:\n\
              mise use swift@latest\n\n\
            Or install Swift via https://swift.org/download/",
        )
        .await;
        let provider = GitProvider::from_ba(&self.ba);
        let repo = SwiftPackageRepo::new(&self.tool_name(), &provider)?;
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
                CloneOptions::default().pr(ctx.pr.as_ref()),
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
            .with_pr(ctx.pr.as_ref())
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitProvider {
    pub api_url: String,
    pub kind: GitProviderKind,
}

impl Default for GitProvider {
    fn default() -> Self {
        Self {
            api_url: github::API_URL.to_string(),
            kind: GitProviderKind::GitHub,
        }
    }
}

#[derive(AsRefStr, Clone, Debug, Eq, PartialEq, EnumString, VariantNames)]
pub enum GitProviderKind {
    #[strum(serialize = "github")]
    GitHub,
    #[strum(serialize = "gitlab")]
    GitLab,
}

impl GitProvider {
    fn from_ba(ba: &BackendArg) -> Self {
        let opts = ba.opts();

        let default_provider = GitProviderKind::GitHub.as_ref().to_string();
        let provider = opts.get("provider").unwrap_or(&default_provider);
        let kind = if ba.tool_name.contains("gitlab.com") {
            GitProviderKind::GitLab
        } else {
            match provider.to_lowercase().as_str() {
                "gitlab" => GitProviderKind::GitLab,
                _ => GitProviderKind::GitHub,
            }
        };

        let api_url = match opts.get("api_url") {
            Some(api_url) => api_url.trim_end_matches('/').to_string(),
            None => match kind {
                GitProviderKind::GitHub => github::API_URL.to_string(),
                GitProviderKind::GitLab => gitlab::API_URL.to_string(),
            },
        };

        Self { api_url, kind }
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
    fn new(name: &str, provider: &GitProvider) -> Result<Self, eyre::Error> {
        let name = name.strip_prefix("spm:").unwrap_or(name);
        let shorthand_regex = regex!(r"^(?:[a-zA-Z0-9_-]+/)+[a-zA-Z0-9._-]+$");
        let shorthand_in_url_regex = regex!(
            r"^https://(?P<domain>[^/]+)/(?P<shorthand>(?:[a-zA-Z0-9_-]+/)+[a-zA-Z0-9._-]+)\.git"
        );

        let (shorthand, url) = if let Some(caps) = shorthand_in_url_regex.captures(name) {
            let shorthand = caps.name("shorthand").unwrap().as_str();
            let url = Url::parse(name)?;
            (shorthand, url)
        } else if shorthand_regex.is_match(name) {
            let host = match provider.kind {
                GitProviderKind::GitHub => "github.com",
                GitProviderKind::GitLab => "gitlab.com",
            };
            let url_str = format!("https://{}/{}.git", host, name);
            let url = Url::parse(&url_str)?;
            (name, url)
        } else {
            Err(eyre::eyre!(
                "Invalid Swift package repository: {}. The repository should either be a repository slug (owner/name), or the complete URL (e.g. https://github.com/owner/name.git).",
                name
            ))?
        };

        Ok(Self {
            url,
            shorthand: shorthand.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{config::Config, toolset::ToolVersionOptions};

    use super::*;
    use indexmap::indexmap;
    use pretty_assertions::assert_str_eq;

    #[tokio::test]
    async fn test_git_provider_from_ba() {
        // Example of defining a capture (closure) in Rust:
        let get_ba = |tool: String, opts: Option<ToolVersionOptions>| {
            BackendArg::new_raw("spm".to_string(), Some(tool.clone()), tool, opts)
        };

        assert_eq!(
            GitProvider::from_ba(&get_ba("tool".to_string(), None)),
            GitProvider {
                api_url: github::API_URL.to_string(),
                kind: GitProviderKind::GitHub
            }
        );

        assert_eq!(
            GitProvider::from_ba(&get_ba(
                "tool".to_string(),
                Some(ToolVersionOptions {
                    opts: indexmap![
                        "provider".to_string() => "gitlab".to_string()
                    ],
                    ..Default::default()
                })
            )),
            GitProvider {
                api_url: gitlab::API_URL.to_string(),
                kind: GitProviderKind::GitLab
            }
        );

        assert_eq!(
            GitProvider::from_ba(&get_ba(
                "tool".to_string(),
                Some(ToolVersionOptions {
                    opts: indexmap![
                        "api_url".to_string() => "https://gitlab.acme.com/api/v4".to_string(),
                        "provider".to_string() => "gitlab".to_string(),
                    ],
                    ..Default::default()
                })
            )),
            GitProvider {
                api_url: "https://gitlab.acme.com/api/v4".to_string(),
                kind: GitProviderKind::GitLab
            }
        );
    }

    #[tokio::test]
    async fn test_spm_repo_init_by_shorthand() {
        let _config = Config::get().await.unwrap();
        let package_repo =
            SwiftPackageRepo::new("nicklockwood/SwiftFormat", &GitProvider::default()).unwrap();
        assert_str_eq!(
            package_repo.url.as_str(),
            "https://github.com/nicklockwood/SwiftFormat.git"
        );
        assert_str_eq!(package_repo.shorthand, "nicklockwood/SwiftFormat");

        let package_repo = SwiftPackageRepo::new(
            "acme/nicklockwood/SwiftFormat",
            &GitProvider {
                api_url: gitlab::API_URL.to_string(),
                kind: GitProviderKind::GitLab,
            },
        )
        .unwrap();
        assert_str_eq!(
            package_repo.url.as_str(),
            "https://gitlab.com/acme/nicklockwood/SwiftFormat.git"
        );
        assert_str_eq!(package_repo.shorthand, "acme/nicklockwood/SwiftFormat");
    }

    #[tokio::test]
    async fn test_spm_repo_init_name() {
        let _config = Config::get().await.unwrap();
        assert!(
            SwiftPackageRepo::new("owner/name.swift", &GitProvider::default()).is_ok(),
            "name part can contain ."
        );
        assert!(
            SwiftPackageRepo::new("owner/name_swift", &GitProvider::default()).is_ok(),
            "name part can contain _"
        );
        assert!(
            SwiftPackageRepo::new("owner/name-swift", &GitProvider::default()).is_ok(),
            "name part can contain -"
        );
        assert!(
            SwiftPackageRepo::new("owner/name$swift", &GitProvider::default()).is_err(),
            "name part cannot contain characters other than a-zA-Z0-9._-"
        );
    }

    #[tokio::test]
    async fn test_spm_repo_init_by_url() {
        let package_repo = SwiftPackageRepo::new(
            "https://github.com/nicklockwood/SwiftFormat.git",
            &GitProvider::default(),
        )
        .unwrap();
        assert_str_eq!(
            package_repo.url.as_str(),
            "https://github.com/nicklockwood/SwiftFormat.git"
        );
        assert_str_eq!(package_repo.shorthand, "nicklockwood/SwiftFormat");

        let package_repo = SwiftPackageRepo::new(
            "https://gitlab.acme.com/acme/someuser/SwiftTool.git",
            &GitProvider {
                api_url: "https://api.gitlab.acme.com/api/v4".to_string(),
                kind: GitProviderKind::GitLab,
            },
        )
        .unwrap();
        assert_str_eq!(
            package_repo.url.as_str(),
            "https://gitlab.acme.com/acme/someuser/SwiftTool.git"
        );
        assert_str_eq!(package_repo.shorthand, "acme/someuser/SwiftTool");
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
