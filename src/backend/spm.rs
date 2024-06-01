use std::env::temp_dir;
use std::fmt::{self, Debug};
use std::path::Path;
use std::str::FromStr;

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
        self.remote_version_cache
            .get_or_try_init(|| {
                Ok(github::list_releases(self.name())?
                    .into_iter()
                    .map(|r| r.tag_name)
                    .rev()
                    .collect())
            })
            .cloned()
    }

    fn install_version_impl(
        &self,
        ctx: &crate::install_context::InstallContext,
    ) -> eyre::Result<()> {
        let settings = Settings::get();
        settings.ensure_experimental("spm backend")?;

        //
        // 1. Checkout the swift package repo:
        // - name could be github repo shorthand or full url
        // - if full url, clone it
        // - if shorthand, convert to full url and clone it
        //
        // - version is a release tag
        // - if version not specified ("latest"), get last release tag
        // - if there are no release tags, get error
        //
        let repo_url = SwiftPackageRepo::from_str(self.name())?.0;
        let version = if ctx.tv.version == "latest" {
            self.latest_stable_version()?
                .ok_or_else(|| eyre::eyre!("No stable versions found"))?
        } else {
            ctx.tv.version.clone()
        };

        let tmp_repo_dir = temp_dir()
            .join("spm")
            .join(self.filename_safe_url(&repo_url) + "@" + &version);
        file::remove_all(&tmp_repo_dir)?;
        file::create_dir_all(tmp_repo_dir.parent().unwrap())?;

        debug!(
            "Cloning swift package repo: {}, tag: {}, path: {}",
            repo_url,
            version,
            tmp_repo_dir.display()
        );
        // TODO: use project git module (now it doesn't support checkout by tag)
        let repo = Repository::clone(&repo_url, &tmp_repo_dir)?;
        let (object, reference) = repo.revparse_ext(&version)?;
        repo.checkout_tree(&object, None)?;
        repo.set_head(reference.unwrap().name().unwrap())?;

        //
        // 2. Build the swift package
        // - parse Package.swift dump to get executables list
        // - build each executable
        //
        let package_json = cmd!(
            "swift",
            "package",
            "dump-package",
            "--package-path",
            &tmp_repo_dir
        )
        .read()?;

        let executables = serde_json::from_str::<PackageDescription>(&package_json)
            .map_err(|err| eyre::eyre!("Failed to parse package description. Details: {}", err))?
            .products
            .iter()
            .filter(|p| p.r#type.is_executable())
            .map(|p| p.name.clone())
            .collect::<Vec<String>>();
        if executables.is_empty() {
            return Err(eyre::eyre!("No executables found in the package"));
        }
        debug!("Found executables: {:?}", executables);

        for executable in executables {
            debug!("Building swift package");
            let build_cmd = CmdLineRunner::new("swift")
                .arg("build")
                .arg("--configuration")
                .arg("release")
                .arg("--product")
                .arg(&executable)
                .arg("--package-path")
                .arg(&tmp_repo_dir)
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
                &tmp_repo_dir,
                "--show-bin-path"
            )
            .read()?;

            //
            // 3. Copy binary to the install path
            // - copy resources and other related files
            //
            let install_bin_path = ctx.tv.install_path().join("bin");
            debug!(
                "Copying binaries to install path: {}",
                install_bin_path.display()
            );
            file::create_dir_all(&install_bin_path)?;
            file::copy(
                Path::new(&bin_path).join(&executable),
                &install_bin_path.join(&executable),
            )?;

            // find and copy resources with save relative to the binary path
            // dynamic libraries: .dylib, .so, .dll
            // resource bundles: .bundle, .resources
            WalkDir::new(&bin_path)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| {
                    let ext = e.path().extension().unwrap_or_default();
                    ext == "dylib"
                        || ext == "so"
                        || ext == "dll"
                        || ext == "bundle"
                        || ext == "resources"
                })
                .try_for_each(|e| -> Result<(), eyre::Error> {
                    let rel_path = e.path().strip_prefix(&bin_path)?;
                    let install_path = install_bin_path.join(rel_path);
                    file::create_dir_all(&install_path.parent().unwrap())?;
                    if e.path().is_dir() {
                        file::copy_dir_all(e.path(), &install_path)?;
                    } else {
                        file::copy(e.path(), &install_path)?;
                    }
                    Ok(())
                })?;
        }

        debug!("Cleaning up temporary files");
        file::remove_all(&tmp_repo_dir)?;

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

    fn filename_safe_url(&self, url: &str) -> String {
        url.replace("://", "_")
            .replace("/", "_")
            .replace("?", "_")
            .replace("&", "_")
            .replace(":", "_")
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

/// https://github.com/owner/repo.git
struct SwiftPackageRepo(String);

impl FromStr for SwiftPackageRepo {
    type Err = eyre::Error;

    // swift package github repo shorthand:
    // - owner/repo
    //
    // swift package github repo full url:
    // - https://github.com/owner/repo.git
    // TODO: support more type of git urls
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let url = Url::parse(s);
        if url.is_ok()
            && url.as_ref().unwrap().host_str() == Some("github.com")
            && url.as_ref().unwrap().path().ends_with(".git")
        {
            Ok(Self(s.to_string()))
        } else if regex!(r"^[a-zA-Z0-9_-]+/[a-zA-Z0-9_-]+$").is_match(s) {
            Ok(Self(format!("https://github.com/{}.git", s.to_string())))
        } else {
            Err(eyre::eyre!("Invalid swift package repo: {}", s))
        }
    }
}
