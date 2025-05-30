use crate::backend::backend_type::BackendType;
use crate::cli::args::BackendArg;
use crate::cli::version::{ARCH, OS};
use crate::cmd::CmdLineRunner;
use crate::config::Settings;
use crate::file::TarOptions;
use crate::http::HTTP;
use crate::install_context::InstallContext;
use crate::path::{Path, PathBuf, PathExt};
use crate::plugins::VERSION_REGEX;
use crate::registry::REGISTRY;
use crate::toolset::ToolVersion;
use crate::{
    aqua::aqua_registry::{
        AQUA_REGISTRY, AquaChecksumType, AquaMinisignType, AquaPackage, AquaPackageType,
    },
    cache::{CacheManager, CacheManagerBuilder},
};
use crate::{backend::Backend, config::Config};
use crate::{file, github, minisign};
use async_trait::async_trait;
use dashmap::DashMap;
use eyre::{ContextCompat, Result, bail};
use indexmap::IndexSet;
use itertools::Itertools;
use regex::Regex;
use std::fmt::Debug;
use std::{collections::HashSet, sync::Arc};

#[derive(Debug)]
pub struct AquaBackend {
    ba: Arc<BackendArg>,
    id: String,
    bin_path_caches: DashMap<String, CacheManager<Vec<PathBuf>>>,
}

#[async_trait]
impl Backend for AquaBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Aqua
    }

    async fn description(&self) -> Option<String> {
        AQUA_REGISTRY
            .package(&self.ba.tool_name)
            .await
            .ok()
            .and_then(|p| p.description.clone())
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    fn get_optional_dependencies(&self) -> Result<Vec<&str>> {
        Ok(vec!["cosign", "slsa-verifier"])
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<String>> {
        let pkg = AQUA_REGISTRY.package(&self.id).await?;
        if !pkg.repo_owner.is_empty() && !pkg.repo_name.is_empty() {
            let versions = get_versions(&pkg).await?;
            Ok(versions
                .into_iter()
                .filter_map(|v| {
                    let mut v = v.as_str();
                    match pkg.version_filter_ok(v) {
                        Ok(true) => {}
                        Ok(false) => return None,
                        Err(e) => {
                            warn!("[{}] aqua version filter error: {e}", self.ba);
                        }
                    }
                    let pkg = pkg.clone().with_version(v);
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
                .collect())
        } else {
            warn!("no aqua registry found for {}", self.ba);
            Ok(vec![])
        }
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> Result<ToolVersion> {
        let mut v = format!("v{}", tv.version);
        let pkg = AQUA_REGISTRY.package_with_version(&self.id, &v).await?;
        if let Some(prefix) = &pkg.version_prefix {
            v = format!("{prefix}{v}");
        }
        validate(&pkg)?;
        let url = match self.fetch_url(&pkg, &v).await {
            Ok(url) => url,
            Err(err) => {
                if let Some(prefix) = &pkg.version_prefix {
                    v = format!("{}{}", prefix, tv.version);
                } else {
                    v = tv.version.to_string();
                }
                self.fetch_url(&pkg, &v)
                    .await
                    .map_err(|e| err.wrap_err(e))?
            }
        };
        let filename = url.split('/').next_back().unwrap();
        self.download(ctx, &tv, &url, filename).await?;
        self.verify(ctx, &mut tv, &pkg, &v, filename).await?;
        self.install(ctx, &tv, &pkg, &v, filename)?;

        Ok(tv)
    }

    async fn list_bin_paths(
        &self,
        _config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> Result<Vec<PathBuf>> {
        // TODO: instead of caching it would probably be better to create this as part of installation
        let cache = self
            .bin_path_caches
            .entry(tv.version.clone())
            .or_insert_with(|| {
                CacheManagerBuilder::new(tv.cache_path().join("bin_paths.msgpack.z"))
                    .with_fresh_duration(Settings::get().fetch_remote_versions_cache())
                    .build()
            });
        let install_path = tv.install_path();
        let paths = cache
            .get_or_try_init_async(async || {
                let pkg = AQUA_REGISTRY
                    .package_with_version(&self.id, &tv.version)
                    .await?;

                let srcs = self.srcs(&pkg, tv)?;
                let paths = if srcs.is_empty() {
                    vec![install_path.clone()]
                } else {
                    srcs.iter()
                        .map(|(_, dst)| dst.parent().unwrap().to_path_buf())
                        .collect()
                };
                Ok(paths
                    .into_iter()
                    .unique()
                    .filter(|p| p.exists())
                    .map(|p| p.strip_prefix(&install_path).unwrap().to_path_buf())
                    .collect())
            })
            .await?
            .iter()
            .map(|p| p.mount(&install_path))
            .collect();
        Ok(paths)
    }

    fn fuzzy_match_filter(&self, versions: Vec<String>, query: &str) -> Vec<String> {
        let escaped_query = regex::escape(query);
        let query = if query == "latest" {
            "\\D*[0-9].*"
        } else {
            &escaped_query
        };
        let query_regex = Regex::new(&format!("^{query}([-.].+)?$")).unwrap();
        versions
            .into_iter()
            .filter(|v| {
                if query == v {
                    return true;
                }
                if VERSION_REGEX.is_match(v) {
                    return false;
                }
                query_regex.is_match(v)
            })
            .collect()
    }
}

impl AquaBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        let full = ba.full();
        let mut id = full.split_once(":").unwrap_or(("", &full)).1;
        if !id.contains("/") {
            id = REGISTRY
                .get(id)
                .and_then(|t| t.backends.iter().find_map(|s| s.full.strip_prefix("aqua:")))
                .unwrap_or_else(|| {
                    warn!("invalid aqua tool: {}", id);
                    id
                });
        }
        Self {
            id: id.to_string(),
            ba: Arc::new(ba),
            bin_path_caches: Default::default(),
        }
    }

    async fn fetch_url(&self, pkg: &AquaPackage, v: &str) -> Result<String> {
        match pkg.r#type {
            AquaPackageType::GithubRelease => self.github_release_url(pkg, v).await,
            AquaPackageType::GithubArchive | AquaPackageType::GithubContent => {
                self.github_archive_url(pkg, v).await
            }
            AquaPackageType::Http => {
                let url = pkg.url(v)?;
                HTTP.head(&url).await?;
                Ok(url)
            }
            ref t => bail!("unsupported aqua package type: {t}"),
        }
    }

    async fn github_release_url(&self, pkg: &AquaPackage, v: &str) -> Result<String> {
        let asset_strs = pkg.asset_strs(v)?;
        self.github_release_asset(pkg, v, asset_strs).await
    }

    async fn github_release_asset(
        &self,
        pkg: &AquaPackage,
        v: &str,
        asset_strs: IndexSet<String>,
    ) -> Result<String> {
        let gh_id = format!("{}/{}", pkg.repo_owner, pkg.repo_name);
        let gh_release = github::get_release(&gh_id, v).await?;
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

    async fn github_archive_url(&self, pkg: &AquaPackage, v: &str) -> Result<String> {
        let gh_id = format!("{}/{}", pkg.repo_owner, pkg.repo_name);
        let url = format!("https://github.com/{gh_id}/archive/refs/tags/{v}.tar.gz");
        HTTP.head(&url).await?;
        Ok(url)
    }

    async fn download(
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
        ctx.pr.set_message(format!("download {filename}"));
        HTTP.download_file(url, &tarball_path, Some(&ctx.pr))
            .await?;
        Ok(())
    }

    async fn verify(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
        pkg: &AquaPackage,
        v: &str,
        filename: &str,
    ) -> Result<()> {
        self.verify_slsa(ctx, tv, pkg, v, filename).await?;
        self.verify_minisign(ctx, tv, pkg, v, filename).await?;
        if !tv.checksums.contains_key(filename) {
            if let Some(checksum) = &pkg.checksum {
                if checksum.enabled() {
                    let url = match checksum._type() {
                        AquaChecksumType::GithubRelease => {
                            let asset_strs = checksum.asset_strs(pkg, v)?;
                            self.github_release_asset(pkg, v, asset_strs).await?
                        }
                        AquaChecksumType::Http => checksum.url(pkg, v)?,
                    };
                    let checksum_path = tv.download_path().join(format!("{filename}.checksum"));
                    HTTP.download_file(&url, &checksum_path, Some(&ctx.pr))
                        .await?;
                    self.cosign_checksums(ctx, pkg, v, tv, &checksum_path)
                        .await?;
                    let mut checksum_file = file::read_to_string(&checksum_path)?;
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
                                        .trim_matches('*')
                                        .to_string(),
                                ))
                            } else {
                                None
                            }
                        })
                        .find(|(_, f)| f == filename)
                        .map(|(c, _)| c)
                        .unwrap_or(checksum_file);
                    let checksum_str = checksum_str.split_whitespace().next().unwrap();
                    let checksum = format!("{}:{}", checksum.algorithm(), checksum_str);
                    tv.checksums.insert(filename.to_string(), checksum);
                }
            }
        }
        let tarball_path = tv.download_path().join(filename);
        self.verify_checksum(ctx, tv, &tarball_path)?;
        Ok(())
    }

    async fn verify_minisign(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
        pkg: &AquaPackage,
        v: &str,
        filename: &str,
    ) -> Result<()> {
        if !Settings::get().aqua.slsa {
            return Ok(());
        }
        if let Some(minisign) = &pkg.minisign {
            if minisign.enabled == Some(false) {
                debug!("minisign is disabled for {tv}");
                return Ok(());
            }
            ctx.pr.set_message("verify minisign".to_string());
            debug!("minisign: {:?}", minisign);
            let sig_path = match minisign._type() {
                AquaMinisignType::GithubRelease => {
                    let asset = minisign.asset(pkg, v)?;
                    let repo_owner = minisign
                        .repo_owner
                        .clone()
                        .unwrap_or_else(|| pkg.repo_owner.clone());
                    let repo_name = minisign
                        .repo_name
                        .clone()
                        .unwrap_or_else(|| pkg.repo_name.clone());
                    let url = github::get_release(&format!("{repo_owner}/{repo_name}"), v)
                        .await?
                        .assets
                        .into_iter()
                        .find(|a| a.name == asset)
                        .map(|a| a.browser_download_url);
                    if let Some(url) = url {
                        let path = tv.download_path().join(asset);
                        HTTP.download_file(&url, &path, Some(&ctx.pr)).await?;
                        path
                    } else {
                        warn!("no asset found for minisign of {tv}: {asset}");
                        return Ok(());
                    }
                }
                AquaMinisignType::Http => {
                    let url = minisign.url(pkg, v)?;
                    let path = tv.download_path().join(filename).with_extension(".minisig");
                    HTTP.download_file(&url, &path, Some(&ctx.pr)).await?;
                    path
                }
            };
            let data = file::read(tv.download_path().join(filename))?;
            let sig = file::read_to_string(sig_path)?;
            minisign::verify(&minisign.public_key(pkg, v)?, &data, &sig)?;
        }
        Ok(())
    }

    async fn verify_slsa(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
        pkg: &AquaPackage,
        v: &str,
        filename: &str,
    ) -> Result<()> {
        if !Settings::get().aqua.slsa {
            return Ok(());
        }
        if let Some(slsa) = &pkg.slsa_provenance {
            if slsa.enabled == Some(false) {
                debug!("slsa is disabled for {tv}");
                return Ok(());
            }
            if let Some(slsa_bin) = self.dependency_which(&ctx.config, "slsa-verifier").await {
                ctx.pr.set_message("verify slsa".to_string());
                let repo_owner = slsa
                    .repo_owner
                    .clone()
                    .unwrap_or_else(|| pkg.repo_owner.clone());
                let repo_name = slsa
                    .repo_name
                    .clone()
                    .unwrap_or_else(|| pkg.repo_name.clone());
                let repo = format!("{repo_owner}/{repo_name}");
                let provenance_path = match slsa.r#type.as_deref().unwrap_or_default() {
                    "github_release" => {
                        let asset = slsa.asset(pkg, v)?;
                        let url = github::get_release(&repo, v)
                            .await?
                            .assets
                            .into_iter()
                            .find(|a| a.name == asset)
                            .map(|a| a.browser_download_url);
                        if let Some(url) = url {
                            let path = tv.download_path().join(asset);
                            HTTP.download_file(&url, &path, Some(&ctx.pr)).await?;
                            path.to_string_lossy().to_string()
                        } else {
                            warn!("no asset found for slsa verification of {tv}: {asset}");
                            return Ok(());
                        }
                    }
                    "http" => {
                        let url = slsa.url(pkg, v)?;
                        let path = tv.download_path().join(filename);
                        HTTP.download_file(&url, &path, Some(&ctx.pr)).await?;
                        path.to_string_lossy().to_string()
                    }
                    t => {
                        warn!("unsupported slsa type: {t}");
                        return Ok(());
                    }
                };
                let source_uri = slsa
                    .source_uri
                    .clone()
                    .unwrap_or_else(|| format!("github.com/{repo}"));
                let mut cmd = CmdLineRunner::new(slsa_bin)
                    .arg("verify-artifact")
                    .arg(tv.download_path().join(filename))
                    .arg("--provenance-repository")
                    .arg(&repo)
                    .arg("--source-uri")
                    .arg(source_uri)
                    .arg("--provenance-path")
                    .arg(provenance_path);
                let source_tag = slsa.source_tag.clone().unwrap_or_else(|| v.to_string());
                if source_tag != "-" {
                    cmd = cmd.arg("--source-tag").arg(source_tag);
                }
                cmd = cmd.with_pr(&ctx.pr);
                cmd.execute()?;
            } else {
                warn!("{tv} can be verified with slsa-verifier but slsa-verifier is not installed");
            }
        }
        Ok(())
    }

    async fn cosign_checksums(
        &self,
        ctx: &InstallContext,
        pkg: &AquaPackage,
        v: &str,
        tv: &ToolVersion,
        checksum_path: &Path,
    ) -> Result<()> {
        if !Settings::get().aqua.cosign {
            return Ok(());
        }
        if let Some(cosign) = pkg.checksum.as_ref().and_then(|c| c.cosign.as_ref()) {
            if cosign.enabled == Some(false) {
                debug!("cosign is disabled for {tv}");
                return Ok(());
            }
            if let Some(cosign_bin) = self.dependency_which(&ctx.config, "cosign").await {
                ctx.pr
                    .set_message("verify checksums with cosign".to_string());
                let mut cmd = CmdLineRunner::new(cosign_bin)
                    .arg("verify-blob")
                    .arg(checksum_path);
                if log::log_enabled!(log::Level::Debug) {
                    cmd = cmd.arg("--verbose");
                }
                if cosign.experimental == Some(true) {
                    cmd = cmd.env("COSIGN_EXPERIMENTAL", "1");
                }
                if let Some(signature) = &cosign.signature {
                    let arg = signature.arg(pkg, v)?;
                    if !arg.is_empty() {
                        cmd = cmd.arg("--signature").arg(arg);
                    }
                }
                if let Some(key) = &cosign.key {
                    let arg = key.arg(pkg, v)?;
                    if !arg.is_empty() {
                        cmd = cmd.arg("--key").arg(arg);
                    }
                }
                if let Some(certificate) = &cosign.certificate {
                    let arg = certificate.arg(pkg, v)?;
                    if !arg.is_empty() {
                        cmd = cmd.arg("--certificate").arg(arg);
                    }
                }
                for opt in cosign.opts(pkg, v)? {
                    cmd = cmd.arg(opt);
                }
                for arg in Settings::get()
                    .aqua
                    .cosign_extra_args
                    .clone()
                    .unwrap_or_default()
                {
                    cmd = cmd.arg(arg);
                }
                cmd = cmd.with_pr(&ctx.pr);
                cmd.execute()?;
            } else {
                warn!("{tv} can be verified with cosign but cosign is not installed");
            }
        }
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
        ctx.pr.set_message(format!("extract {filename}"));
        let install_path = tv.install_path();
        file::remove_all(&install_path)?;
        let format = pkg.format(v)?;
        let mut bin_path = install_path.join(
            pkg.files
                .first()
                .map(|f| f.name.as_str())
                .or_else(|| pkg.name.as_ref().and_then(|n| n.split('/').next_back()))
                .unwrap_or(&pkg.repo_name),
        );
        if cfg!(windows) && pkg.complete_windows_ext {
            bin_path = bin_path.with_extension("exe");
        }
        let mut tar_opts = TarOptions {
            format: format.parse().unwrap_or_default(),
            pr: Some(&ctx.pr),
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
        } else if format == "pkg" {
            file::un_pkg(&tarball_path, &install_path)?;
        } else {
            bail!("unsupported format: {}", format);
        }

        for (src, dst) in self.srcs(pkg, tv)? {
            if src != dst && src.exists() && !dst.exists() {
                if cfg!(windows) {
                    file::copy(&src, &dst)?;
                } else {
                    let src = PathBuf::from(".").join(src.file_name().unwrap().to_str().unwrap());
                    file::make_symlink(&src, &dst)?;
                }
            }
        }

        Ok(())
    }

    fn srcs(&self, pkg: &AquaPackage, tv: &ToolVersion) -> Result<Vec<(PathBuf, PathBuf)>> {
        let files: Vec<(PathBuf, PathBuf)> = pkg
            .files
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
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .unique_by(|(src, _)| src.to_path_buf())
            .collect();
        Ok(files)
    }
}

async fn get_versions(pkg: &AquaPackage) -> Result<Vec<String>> {
    if let Some("github_tag") = pkg.version_source.as_deref() {
        let versions = github::list_tags(&format!("{}/{}", pkg.repo_owner, pkg.repo_name)).await?;
        return Ok(versions);
    }
    let mut versions = github::list_releases(&format!("{}/{}", pkg.repo_owner, pkg.repo_name))
        .await?
        .into_iter()
        .map(|r| r.tag_name)
        .collect_vec();
    if versions.is_empty() {
        versions = github::list_tags(&format!("{}/{}", pkg.repo_owner, pkg.repo_name)).await?;
    }
    Ok(versions)
}

fn validate(pkg: &AquaPackage) -> Result<()> {
    let envs: HashSet<&str> = pkg.supported_envs.iter().map(|s| s.as_str()).collect();
    let os = os();
    let arch = arch();
    let os_arch = format!("{os}/{arch}");
    let mut myself: HashSet<&str> = ["all", os, arch, os_arch.as_str()].into();
    if os == "windows" && arch == "arm64" {
        // assume windows/arm64 is supported
        myself.insert("windows/amd64");
        myself.insert("amd64");
    }
    if !envs.is_empty() && envs.is_disjoint(&myself) {
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
