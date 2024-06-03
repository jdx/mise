use std::env::temp_dir;
use std::fmt::{self, Debug};
use std::path::PathBuf;

use git2::Repository;
use serde::de::{MapAccess, Visitor};
use serde::Deserializer;
use serde_derive::Deserialize;
use url::Url;
use walkdir::WalkDir;

use crate::backend::{Backend, BackendType};
use crate::cache::CacheManager;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::Settings;
use crate::install_context::InstallContext;
use crate::{file, github};

#[derive(Debug)]
pub struct SPMBackend {
    fa: BackendArg,
    remote_version_cache: CacheManager<Vec<String>>,
}

// https://github.com/apple/swift-package-manager
impl Backend for SPMBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Spm
    }

    fn fa(&self) -> &BackendArg {
        &self.fa
    }

    fn get_dependencies(
        &self,
        _tvr: &crate::toolset::ToolRequest,
    ) -> eyre::Result<Vec<BackendArg>> {
        // TODO: swift as dependencies (wait for swift core plugin: https://github.com/jdx/mise/pull/1708)
        Ok(vec![])
    }

    fn _list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        let repo = SwiftPackageRepo::new(self.name())?;
        self.remote_version_cache
            .get_or_try_init(|| {
                Ok(github::list_releases(repo.shorthand.as_str())?
                    .into_iter()
                    .map(|r| r.tag_name)
                    .rev()
                    .collect())
            })
            .cloned()
    }

    fn install_version_impl(&self, ctx: &InstallContext) -> eyre::Result<()> {
        let settings = Settings::get();
        settings.ensure_experimental("spm backend")?;

        let repo = SwiftPackageRepo::new(self.name())?;
        let revision = if ctx.tv.version == "latest" {
            self.latest_stable_version()?
                .ok_or_else(|| eyre::eyre!("No stable versions found"))?
        } else {
            ctx.tv.version.clone()
        };
        let repo_dir = self.clone_package_repo(&repo, &revision)?;

        let executables = self.get_executable_names(&repo_dir)?;
        if executables.is_empty() {
            return Err(eyre::eyre!("No executables found in the package"));
        }
        for executable in executables {
            let bin_path = self.build_executable(&executable, &repo_dir, ctx)?;
            let install_bin_path = ctx.tv.install_path().join("bin");
            self.copy_build_artifacts(&executable, &bin_path, &install_bin_path)?;
        }

        debug!("Cleaning up temporary files");
        file::remove_all(&repo_dir)?;

        Ok(())
    }
}

impl SPMBackend {
    pub fn new(name: String) -> Self {
        let fa = BackendArg::new(BackendType::Spm, &name);
        Self {
            remote_version_cache: CacheManager::new(
                fa.cache_path.join("remote_versions-$KEY.msgpack.z"),
            ),
            fa,
        }
    }

    fn clone_package_repo(
        &self,
        package_repo: &SwiftPackageRepo,
        revision: &str,
    ) -> Result<PathBuf, eyre::Error> {
        let tmp_repo_dir = temp_dir().join("spm").join(package_repo.dir_name(revision));
        file::remove_all(&tmp_repo_dir)?;
        file::create_dir_all(tmp_repo_dir.parent().unwrap())?;

        debug!(
            "Cloning swift package repo: {}, revision: {} to path: {}",
            package_repo.url.as_str(),
            revision,
            tmp_repo_dir.display()
        );
        let repo = Repository::clone(package_repo.url.as_str(), &tmp_repo_dir)?;
        let (object, reference) = repo.revparse_ext(revision)?;
        repo.checkout_tree(&object, None)?;
        repo.set_head(reference.unwrap().name().unwrap())?;
        Ok(tmp_repo_dir)
    }

    fn get_executable_names(&self, repo_dir: &PathBuf) -> Result<Vec<String>, eyre::Error> {
        let package_json = cmd!(
            "swift",
            "package",
            "dump-package",
            "--package-path",
            &repo_dir
        )
        .read()?;
        let executables = serde_json::from_str::<PackageDescription>(&package_json)
            .map_err(|err| eyre::eyre!("Failed to parse package description. Details: {}", err))?
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
            .with_pr(ctx.pr.as_ref());
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
    fn new(name: &str) -> Result<Self, eyre::Error> {
        let shorthand_regex = regex!(r"^[a-zA-Z0-9_-]+/[a-zA-Z0-9_-]+$");
        let shorthand_in_url_regex =
            regex!(r"https://github.com/([a-zA-Z0-9_-]+/[a-zA-Z0-9_-]+)\.git");

        let shorthand =
            if let Some(Some(m)) = shorthand_in_url_regex.captures(name).map(|c| c.get(1)) {
                m.as_str()
            } else if shorthand_regex.is_match(name) {
                name
            } else {
                Err(eyre::eyre!("Invalid swift package repo: {}", name))?
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
    use pretty_assertions::assert_str_eq;
    use test_log::test;

    use super::*;

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
