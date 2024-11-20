use crate::aqua::aqua_registry::{AquaChecksumType, AquaPackage, AquaPackageType, AQUA_REGISTRY};
use crate::backend::backend_type::BackendType;
use crate::backend::Backend;
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cli::args::BackendArg;
use crate::cli::version::{ARCH, OS};
use crate::config::SETTINGS;
use crate::file::TarOptions;
use crate::http::HTTP;
use crate::install_context::InstallContext;
use crate::registry::REGISTRY;
use crate::toolset::ToolVersion;
use crate::{dirs, file, github};
use eyre::{bail, ContextCompat, Result};
use indexmap::IndexSet;
use itertools::Itertools;
use std::collections::HashSet;
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

    fn ba(&self) -> &BackendArg {
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
                            .filter_map(|r| {
                                let mut v = r.tag_name.as_str();
                                if let Some(prefix) = &pkg.version_prefix {
                                    if let Some(_v) = v.strip_prefix(prefix) {
                                        v = _v
                                    } else {
                                        return None;
                                    }
                                }
                                v = v.strip_prefix('v').unwrap_or(v);
                                Some(v.to_string())
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

    fn install_version_impl(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> eyre::Result<ToolVersion> {
        let mut v = format!("v{}", tv.version);
        let pkg = AQUA_REGISTRY
            .package_with_version(&self.id, &v)?
            .wrap_err_with(|| format!("no aqua registry found for {}", self.id))?;
        if let Some(prefix) = &pkg.version_prefix {
            v = format!("{}{}", prefix, v);
        }
        validate(&pkg)?;
        let url = match self.fetch_url(&pkg, &v) {
            Ok(url) => url,
            Err(err) => {
                if let Some(prefix) = &pkg.version_prefix {
                    v = format!("{}{}", prefix, tv.version);
                } else {
                    v = tv.version.to_string();
                }
                self.fetch_url(&pkg, &v).map_err(|e| err.wrap_err(e))?
            }
        };
        let filename = url.split('/').last().unwrap();
        self.download(ctx, &tv, &url, filename)?;
        self.verify(ctx, &mut tv, &pkg, &v, filename)?;
        self.install(ctx, &tv, &pkg, &v, filename)?;

        Ok(tv)
    }

    fn list_bin_paths(&self, tv: &ToolVersion) -> Result<Vec<PathBuf>> {
        let pkg = AQUA_REGISTRY
            .package_with_version(&self.id, &tv.version)?
            .wrap_err_with(|| format!("no aqua registry found for {}", self.ba))?;

        let srcs = self.srcs(&pkg, tv)?;
        if srcs.is_empty() {
            return Ok(vec![tv.install_path()]);
        }

        Ok(srcs
            .iter()
            .map(|(_, dst)| dst.parent().unwrap().to_path_buf())
            .filter(|p| p.exists())
            .unique()
            .collect())
    }
}

impl AquaBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        let mut id = ba.tool_name.as_str();
        if !id.contains("/") {
            id = REGISTRY
                .get(id)
                .and_then(|t| t.backends.iter().find_map(|s| s.strip_prefix("aqua:")))
                .unwrap_or_else(|| {
                    warn!("invalid aqua tool: {}", id);
                    id
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

    fn fetch_url(&self, pkg: &AquaPackage, v: &str) -> Result<String> {
        match pkg.r#type {
            AquaPackageType::GithubRelease => self.github_release_url(pkg, v),
            AquaPackageType::GithubArchive | AquaPackageType::GithubContent => {
                self.github_archive_url(pkg, v)
            }
            AquaPackageType::Http => {
                let url = pkg.url(v)?;
                HTTP.head(&url)?;
                Ok(url)
            }
        }
    }

    fn github_release_url(&self, pkg: &AquaPackage, v: &str) -> Result<String> {
        let asset_strs = pkg.asset_strs(v)?;
        self.github_release_asset(pkg, v, asset_strs)
    }

    fn github_release_asset(
        &self,
        pkg: &AquaPackage,
        v: &str,
        asset_strs: IndexSet<String>,
    ) -> Result<String> {
        let gh_id = format!("{}/{}", pkg.repo_owner, pkg.repo_name);
        let gh_release = github::get_release(&gh_id, v)?;
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

        Ok(asset.browser_download_url.to_string())
    }

    fn github_archive_url(&self, pkg: &AquaPackage, v: &str) -> Result<String> {
        let gh_id = format!("{}/{}", pkg.repo_owner, pkg.repo_name);
        let url = format!("https://github.com/{gh_id}/archive/refs/tags/{v}.tar.gz");
        HTTP.head(&url)?;
        Ok(url)
    }

    fn download(
        &self,
        ctx: &InstallContext,
        tv: &ToolVersion,
        url: &str,
        filename: &str,
    ) -> Result<()> {
        let tarball_path = tv.download_path().join(filename);
        if tarball_path.exists() {
            return Ok(());
        }
        ctx.pr.set_message(format!("downloading {filename}"));
        HTTP.download_file(url, &tarball_path, Some(ctx.pr.as_ref()))?;
        Ok(())
    }

    fn verify(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
        pkg: &AquaPackage,
        v: &str,
        filename: &str,
    ) -> Result<()> {
        if tv.checksum.is_none() {
            tv.checksum = if let Some(checksum) = &pkg.checksum {
                if checksum.enabled() {
                    let url = match checksum._type() {
                        AquaChecksumType::GithubRelease => {
                            let asset_strs = checksum.asset_strs(pkg, v)?;
                            self.github_release_asset(pkg, v, asset_strs)?
                        }
                        AquaChecksumType::Http => checksum.url(pkg, v)?,
                    };
                    let mut checksum_file = HTTP.get_text(&url)?;
                    if checksum.file_format() == "regexp" {
                        let pattern = checksum.pattern();
                        if let Some(file) = &pattern.file {
                            let re = regex::Regex::new(file.as_str())?;
                            if let Some(line) = checksum_file.lines().find(|l| {
                                re.captures(l).is_some_and(|c| c[1].to_string() == filename)
                            }) {
                                checksum_file = line.to_string();
                            } else {
                                debug!(
                                    "no line found matching {} in {} for {}",
                                    file, checksum_file, filename
                                );
                            }
                        }
                        let re = regex::Regex::new(pattern.checksum.as_str())?;
                        if let Some(caps) = re.captures(checksum_file.as_str()) {
                            checksum_file = caps[1].to_string();
                        } else {
                            debug!(
                                "no checksum found matching {} in {}",
                                pattern.checksum, checksum_file
                            );
                        }
                    }
                    let checksum_str = checksum_file
                        .lines()
                        .filter_map(|l| {
                            let split = l.split_whitespace().collect_vec();
                            if split.len() == 2 {
                                Some((
                                    split[0].to_string(),
                                    split[1]
                                        .rsplit_once('/')
                                        .map(|(_, f)| f)
                                        .unwrap_or(split[1])
                                        .to_string(),
                                ))
                            } else {
                                None
                            }
                        })
                        .find(|(_, f)| f == filename)
                        .map(|(c, _)| c)
                        .unwrap_or(checksum_file);
                    Some(format!("{}:{}", checksum.algorithm(), checksum_str.trim()))
                } else {
                    None
                }
            } else {
                None
            };
        }
        let tarball_path = tv.download_path().join(filename);
        self.verify_checksum(ctx, tv, &tarball_path)?;
        Ok(())
    }

    fn install(
        &self,
        ctx: &InstallContext,
        tv: &ToolVersion,
        pkg: &AquaPackage,
        v: &str,
        filename: &str,
    ) -> Result<()> {
        let tarball_path = tv.download_path().join(filename);
        ctx.pr.set_message(format!("installing {filename}"));
        let install_path = tv.install_path();
        file::remove_all(&install_path)?;
        let format = pkg.format(v)?;
        let bin_path =
            install_path.join(pkg.files.first().map(|f| &f.name).unwrap_or(&pkg.repo_name));
        let mut tar_opts = TarOptions {
            format: format.parse().unwrap_or_default(),
            pr: Some(ctx.pr.as_ref()),
            strip_components: 0,
        };
        if let AquaPackageType::GithubArchive = pkg.r#type {
            file::untar(&tarball_path, &install_path, &tar_opts)?;
        } else if let AquaPackageType::GithubContent = pkg.r#type {
            tar_opts.strip_components = 1;
            file::untar(&tarball_path, &install_path, &tar_opts)?;
        } else if format == "raw" {
            file::create_dir_all(&install_path)?;
            file::copy(&tarball_path, &bin_path)?;
            file::make_executable(&bin_path)?;
        } else if format.starts_with("tar") {
            file::untar(&tarball_path, &install_path, &tar_opts)?;
        } else if format == "zip" {
            file::unzip(&tarball_path, &install_path)?;
        } else if format == "gz" {
            file::create_dir_all(&install_path)?;
            file::un_gz(&tarball_path, &bin_path)?;
            file::make_executable(&bin_path)?;
        } else if format == "xz" {
            file::create_dir_all(&install_path)?;
            file::un_xz(&tarball_path, &bin_path)?;
            file::make_executable(&bin_path)?;
        } else if format == "bz2" {
            file::create_dir_all(&install_path)?;
            file::un_bz2(&tarball_path, &bin_path)?;
            file::make_executable(&bin_path)?;
        } else if format == "dmg" {
            file::un_dmg(&tarball_path, &install_path)?;
        } else {
            bail!("unsupported format: {}", format);
        }

        for (src, dst) in self.srcs(pkg, tv)? {
            if src != dst {
                if cfg!(windows) {
                    file::copy(&src, &dst)?;
                } else {
                    file::make_symlink(&PathBuf::from(".").join(&src), &dst)?;
                }
            }
        }

        Ok(())
    }

    fn srcs(&self, pkg: &AquaPackage, tv: &ToolVersion) -> Result<Vec<(PathBuf, PathBuf)>> {
        pkg.files
            .iter()
            .map(|f| {
                let srcs = if let Some(prefix) = &pkg.version_prefix {
                    vec![f.src(pkg, &format!("{}{}", prefix, tv.version))?]
                } else {
                    vec![
                        f.src(pkg, &tv.version)?,
                        f.src(pkg, &format!("v{}", tv.version))?,
                    ]
                };
                Ok(srcs
                    .into_iter()
                    .flatten()
                    .map(|src| tv.install_path().join(src))
                    .map(|src| {
                        let dst = src.parent().unwrap().join(f.name.as_str());
                        (src, dst)
                    }))
            })
            .flatten_ok()
            .collect()
    }
}

fn validate(pkg: &AquaPackage) -> Result<()> {
    let envs: HashSet<&str> = pkg.supported_envs.iter().map(|s| s.as_str()).collect();
    let os = os();
    let arch = arch();
    let os_arch = format!("{}/{}", os, arch);
    if !(envs.is_empty()
        || envs.contains("all")
        || envs.contains(os)
        || envs.contains(arch)
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
