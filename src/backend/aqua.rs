use crate::aqua::aqua_registry::{AquaPackage, AquaPackageType, AQUA_REGISTRY};
use crate::aqua::aqua_template;
use crate::backend::{Backend, BackendType};
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cli::args::BackendArg;
use crate::cli::version::{ARCH, OS};
use crate::config::SETTINGS;
use crate::http::HTTP;
use crate::install_context::InstallContext;
use crate::registry::REGISTRY;
use crate::toolset::ToolVersion;
use crate::{dirs, file, github, hashmap};
use eyre::{bail, ContextCompat, Result};
use itertools::Itertools;
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::path::PathBuf;

#[derive(Debug)]
pub struct AquaBackend {
    ba: BackendArg,
    id: String,
    remote_version_cache: CacheManager<Vec<String>>,
}

impl Backend for AquaBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Aqua
    }

    fn fa(&self) -> &BackendArg {
        &self.ba
    }

    fn _list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        self.remote_version_cache
            .get_or_try_init(|| {
                if let Some(pkg) = AQUA_REGISTRY.package(&self.id)? {
                    if !pkg.repo_owner.is_empty() && !pkg.repo_name.is_empty() {
                        Ok(
                            github::list_releases(&format!(
                                "{}/{}",
                                pkg.repo_owner, pkg.repo_name
                            ))?
                            .into_iter()
                            .map(|r| {
                                r.tag_name
                                    .strip_prefix('v')
                                    .unwrap_or(&r.tag_name)
                                    .to_string()
                            })
                            .rev()
                            .collect_vec(),
                        )
                    } else {
                        warn!("no aqua registry found for {}", self.ba);
                        Ok(vec![])
                    }
                } else {
                    warn!("no aqua registry found for {}", self.ba);
                    Ok(vec![])
                }
            })
            .cloned()
    }

    fn install_version_impl(&self, ctx: &InstallContext) -> eyre::Result<()> {
        if !cfg!(windows) {
            SETTINGS.ensure_experimental("aqua")?;
        }
        let v = &ctx.tv.version;
        let pkg = AQUA_REGISTRY
            .package_with_version(&self.id, v)?
            .wrap_err_with(|| format!("no aqua registry found for {}", self.id))?;
        match pkg.r#type {
            AquaPackageType::GithubRelease => {
                Self::install_version_github_release(ctx, v, &pkg)?;
            }
            AquaPackageType::Http => {
                unimplemented!("http aqua packages not yet supported")
            }
        }

        Ok(())
    }

    fn list_bin_paths(&self, tv: &ToolVersion) -> Result<Vec<PathBuf>> {
        let pkg = AQUA_REGISTRY
            .package_with_version(&self.id, &tv.version)?
            .wrap_err_with(|| format!("no aqua registry found for {}", self.ba))?;

        if pkg.files.is_empty() {
            return Ok(vec![tv.install_path()]);
        }

        Ok(pkg
            .files
            .iter()
            .flat_map(|f| {
                f.src
                    .as_ref()
                    .map(|s| parse_aqua_str(&pkg, s, &tv.version, &Default::default()))
            })
            .map(|f| {
                PathBuf::from(f)
                    .parent()
                    .map(|p| tv.install_path().join(p))
                    .unwrap_or_else(|| tv.install_path())
            })
            .unique()
            .collect())
    }
}

impl AquaBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        let mut id = ba.full.strip_prefix("aqua:").unwrap_or(&ba.full);
        if !id.contains("/") {
            id = REGISTRY
                .get(id)
                .and_then(|b| b.iter().find_map(|s| s.strip_prefix("aqua:")))
                .unwrap_or_else(|| {
                    panic!("invalid aqua tool: {}", id);
                });
        }
        Self {
            remote_version_cache: CacheManagerBuilder::new(
                ba.cache_path.join("remote_versions.msgpack.z"),
            )
            .with_fresh_duration(SETTINGS.fetch_remote_versions_cache())
            .with_fresh_file(dirs::DATA.to_path_buf())
            .with_fresh_file(ba.installs_path.to_path_buf())
            .build(),
            id: id.to_string(),
            ba,
        }
    }

    fn install_version_github_release(
        ctx: &InstallContext,
        v: &str,
        pkg: &AquaPackage,
    ) -> Result<()> {
        validate(pkg)?;
        let gh_id = format!("{}/{}", pkg.repo_owner, pkg.repo_name);
        let mut v = format!("v{}", v);
        let gh_release = match github::get_release(&gh_id, &v) {
            Ok(r) => r,
            Err(_) => {
                v = v.strip_prefix('v').unwrap().to_string();
                github::get_release(&gh_id, &v)?
            }
        };
        let asset_strs = asset_strs(pkg, &v);
        let asset = gh_release
            .assets
            .iter()
            .find(|a| asset_strs.contains(&a.name))
            .wrap_err_with(|| {
                format!(
                    "no asset found: {}\nAvailable assets:\n{}",
                    asset_strs.iter().join(", "),
                    gh_release.assets.iter().map(|a| &a.name).join("\n")
                )
            })?;

        let url = &asset.browser_download_url;
        let filename = url.split('/').last().unwrap();
        let tarball_path = ctx.tv.download_path().join(filename);
        ctx.pr.set_message(format!("downloading {filename}"));
        HTTP.download_file(url, &tarball_path, Some(ctx.pr.as_ref()))?;

        ctx.pr.set_message(format!("installing {filename}"));
        let install_path = ctx.tv.install_path();
        file::remove_all(&install_path)?;
        if pkg.format == "raw" {
            file::create_dir_all(&install_path)?;
            let bin_path = install_path.join(&pkg.repo_name);
            file::copy(&tarball_path, &bin_path)?;
            file::make_executable(&bin_path)?;
        } else if pkg.format == "tar.gz" {
            file::untar_gz(&tarball_path, &install_path)?;
        } else if pkg.format == "tar.xz" {
            file::untar_xz(&tarball_path, &install_path)?;
        } else if pkg.format == "zip" {
            file::unzip(&tarball_path, &install_path)?;
        } else {
            bail!("unsupported format: {}", pkg.format);
        }

        Ok(())
    }
}

fn validate(pkg: &AquaPackage) -> Result<()> {
    let envs: HashSet<&str> = pkg.supported_envs.iter().map(|s| s.as_str()).collect();
    let os = os();
    let os_arch = format!("{}-{}", os, arch());
    if !(envs.is_empty()
        || envs.contains("all")
        || envs.contains(os)
        || envs.contains(os_arch.as_str()))
    {
        bail!("unsupported env: {os_arch}");
    }
    Ok(())
}

pub fn os() -> &'static str {
    if cfg!(target_os = "macos") {
        "darwin"
    } else {
        &OS
    }
}

pub fn arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "amd64"
    } else if cfg!(target_arch = "arm") {
        "armv6l"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        &ARCH
    }
}

fn asset_strs(pkg: &AquaPackage, v: &str) -> HashSet<String> {
    let mut ctx = Default::default();
    let mut strs = HashSet::from([parse_aqua_str(pkg, &pkg.asset, v, &ctx)]);
    if cfg!(macos) {
        ctx.insert("ARCH".to_string(), "arm64".to_string());
        strs.insert(parse_aqua_str(pkg, &pkg.asset, v, &ctx));
    }
    strs
}

fn parse_aqua_str(
    pkg: &AquaPackage,
    s: &str,
    v: &str,
    overrides: &HashMap<String, String>,
) -> String {
    let os = os();
    let mut arch = arch();
    if os == "darwin" && arch == "arm64" && pkg.rosetta2 {
        arch = "amd64";
    }
    if os == "windows" && arch == "arm64" && pkg.windows_arm_emulation {
        arch = "amd64";
    }
    let replace = |s: &str| {
        pkg.replacements
            .get(s)
            .map(|s| s.to_string())
            .unwrap_or_else(|| s.to_string())
    };
    let mut ctx = hashmap! {
        "Version".to_string() => replace(v),
        "OS".to_string() => replace(os),
        "Arch".to_string() => replace(arch),
        "Format".to_string() => replace(&pkg.format),
    };
    ctx.extend(overrides.clone());
    aqua_template::render(s, &ctx)
}
