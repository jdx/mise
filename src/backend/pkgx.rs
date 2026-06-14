use crate::backend::backend_type::BackendType;
use crate::backend::platform_target::PlatformTarget;
use crate::backend::{VersionInfo, runtime_path_for_install_path};
use crate::cli::args::BackendArg;
use crate::config::{Config, Settings};
use crate::file::{ExtractOptions, ExtractionFormat};
use crate::hash;
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::lockfile::{self, Lockfile, PlatformInfo};
use crate::toolset::{ToolRequest, ToolVersion};
use crate::{backend::Backend, file};
use async_trait::async_trait;
use eyre::{Result, WrapErr, bail};
use indexmap::IndexMap;
use nodejs_semver::{Range, Version as NodeVersion};
use serde::Deserialize;
use serde_yaml::{Mapping, Value};
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const DIST_URL: &str = "https://dist.pkgx.dev";
const PANTRY_RAW_URL: &str = "https://raw.githubusercontent.com/pkgxdev/pantry/main/projects";
pub const EXPERIMENTAL: bool = true;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct PkgxPackageInfo {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pkgx_provides: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pkgx_runtime_env: Option<BTreeMap<String, String>>,
}

pub fn install_time_option_keys() -> Vec<String> {
    vec![]
}

#[derive(Debug)]
pub struct PkgxBackend {
    ba: Arc<BackendArg>,
}

#[derive(Debug, Clone)]
struct ResolvedPackage {
    name: String,
    version: String,
    manifest: PackageManifest,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
struct PackageManifest {
    provides: Vec<String>,
    dependencies: Value,
    companions: Value,
    runtime: RuntimeManifest,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
struct RuntimeManifest {
    env: Value,
}

#[async_trait]
impl Backend for PkgxBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Pkgx
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    fn supports_lockfile_url(&self) -> bool {
        true
    }

    fn mark_prereleases_from_version_pattern(&self) -> bool {
        true
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<VersionInfo>> {
        self.ensure_experimental()?;
        Ok(list_pkg_versions(&self.tool_name()).await?)
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> Result<ToolVersion> {
        self.ensure_experimental()?;
        let pkgx_root = pkgx_root(&tv);
        file::create_dir_all(&pkgx_root)?;

        let platform_key = self.get_platform_key();
        let packages = if tv
            .lock_platforms
            .get(&platform_key)
            .and_then(|p| p.url.as_ref())
            .is_some()
        {
            self.install_from_locked(ctx, &mut tv, &pkgx_root, &platform_key)
                .await?
        } else {
            let target = PlatformTarget::from_current();
            let packages = resolve_closure(&self.tool_name(), &tv.version, &target).await?;
            let mut bottles = BTreeMap::new();
            for package in &packages {
                let bottle = resolve_bottle_info(&package.name, &package.version, &target).await?;
                install_package(ctx, &tv, &pkgx_root, package, &bottle).await?;
                bottles.insert(pkgx_package_id(package), bottle);
            }
            self.populate_lock_info(&mut tv, &platform_key, &packages, &bottles)
                .await?;
            packages
        };

        write_wrappers(&tv, &packages)?;
        Ok(tv)
    }

    async fn resolve_lock_info(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        self.ensure_experimental()?;
        let packages = resolve_closure(&self.tool_name(), &tv.version, target).await?;
        let Some(main) = packages
            .iter()
            .find(|package| package.name == self.tool_name())
        else {
            return Ok(PlatformInfo::default());
        };
        let main_bottle = resolve_bottle_info(&main.name, &main.version, target).await?;
        let deps = packages
            .iter()
            .filter(|package| package.name != main.name)
            .map(pkgx_package_id)
            .collect::<Vec<_>>();

        Ok(PlatformInfo {
            checksum: main_bottle.checksum,
            url: Some(main_bottle.url),
            pkgx_deps: Some(deps),
            pkgx_provides: optional_vec(main.manifest.provided_bins()),
            pkgx_runtime_env: optional_map(
                main.manifest
                    .runtime_env_for_target(target)
                    .into_iter()
                    .collect(),
            ),
            ..Default::default()
        })
    }

    async fn list_bin_paths(
        &self,
        _config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> Result<Vec<PathBuf>> {
        Ok(vec![runtime_path_for_install_path(
            tv,
            tv.install_path().join("bin"),
        )])
    }

    fn resolve_lockfile_options(
        &self,
        _request: &ToolRequest,
        _target: &PlatformTarget,
    ) -> Result<BTreeMap<String, String>> {
        Ok(BTreeMap::new())
    }
}

impl PkgxBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self { ba: Arc::new(ba) }
    }

    fn ensure_experimental(&self) -> Result<()> {
        Settings::get().ensure_experimental("pkgx backend")
    }

    fn read_lockfile_for_tool(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<Lockfile> {
        let (lockfile_path, _) =
            lockfile::lockfile_path_for_tool_source(&ctx.config, tv.request.source())
                .ok_or_else(|| eyre::eyre!("could not determine pkgx lockfile path"))?;
        Lockfile::read(&lockfile_path)
    }

    async fn install_from_locked(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
        pkgx_root: &Path,
        platform_key: &str,
    ) -> Result<Vec<ResolvedPackage>> {
        let platform_info = tv
            .lock_platforms
            .get(platform_key)
            .ok_or_else(|| eyre::eyre!("no pkgx lock info for platform {platform_key}"))?;
        let main_url = platform_info
            .url
            .clone()
            .ok_or_else(|| eyre::eyre!("no pkgx URL in lockfile for {}", self.tool_name()))?;
        let main = ResolvedPackage {
            name: self.tool_name(),
            version: tv.version.clone(),
            manifest: PackageManifest::from_locked_metadata(
                platform_info.pkgx_provides.clone(),
                platform_info.pkgx_runtime_env.clone(),
            ),
        };

        let lockfile = self.read_lockfile_for_tool(ctx, tv)?;
        let mut packages = Vec::new();
        for dep_id in platform_info.pkgx_deps.clone().unwrap_or_default() {
            let (name, version) = parse_pkgx_package_id(&dep_id)?;
            let pkg_info = lockfile
                .get_pkgx_package(platform_key, &dep_id)
                .ok_or_else(|| {
                    eyre::eyre!("pkgx package {dep_id} not found in lockfile for {platform_key}")
                })?
                .clone();
            let package = ResolvedPackage {
                name,
                version,
                manifest: PackageManifest::from_locked_metadata(
                    pkg_info.pkgx_provides.clone(),
                    pkg_info.pkgx_runtime_env.clone(),
                ),
            };
            install_package(ctx, tv, pkgx_root, &package, &pkg_info).await?;
            tv.pkgx_packages
                .insert((platform_key.to_string(), dep_id), pkg_info);
            packages.push(package);
        }

        let main_info = PkgxPackageInfo {
            url: main_url,
            checksum: platform_info.checksum.clone(),
            pkgx_provides: platform_info.pkgx_provides.clone(),
            pkgx_runtime_env: platform_info.pkgx_runtime_env.clone(),
        };
        install_package(ctx, tv, pkgx_root, &main, &main_info).await?;
        packages.push(main);

        Ok(packages)
    }

    async fn populate_lock_info(
        &self,
        tv: &mut ToolVersion,
        platform_key: &str,
        packages: &[ResolvedPackage],
        bottles: &BTreeMap<String, PkgxPackageInfo>,
    ) -> Result<()> {
        let Some(main) = packages
            .iter()
            .find(|package| package.name == self.tool_name())
        else {
            return Ok(());
        };
        let main_bottle = bottles
            .get(&pkgx_package_id(main))
            .ok_or_else(|| eyre::eyre!("missing pkgx bottle info for {}", pkgx_package_id(main)))?;
        let deps = packages
            .iter()
            .filter(|package| package.name != main.name)
            .map(pkgx_package_id)
            .collect::<Vec<_>>();

        let platform_info = tv
            .lock_platforms
            .entry(platform_key.to_string())
            .or_default();
        platform_info.url = Some(main_bottle.url.clone());
        platform_info.checksum = main_bottle.checksum.clone();
        platform_info.pkgx_deps = Some(deps);
        platform_info.pkgx_provides = optional_vec(main.manifest.provided_bins());
        platform_info.pkgx_runtime_env = optional_map(
            main.manifest
                .runtime_env_for_current_platform()
                .into_iter()
                .collect(),
        );

        for package in packages.iter().filter(|package| package.name != main.name) {
            let id = pkgx_package_id(package);
            let bottle = bottles
                .get(&id)
                .ok_or_else(|| eyre::eyre!("missing pkgx bottle info for {id}"))?;
            tv.pkgx_packages.insert(
                (platform_key.to_string(), id),
                pkgx_package_info_with_metadata(
                    bottle,
                    package.manifest.provided_bins(),
                    package.manifest.runtime_env_for_current_platform(),
                ),
            );
        }
        Ok(())
    }

    pub async fn resolve_pkgx_packages(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<BTreeMap<String, PkgxPackageInfo>> {
        self.ensure_experimental()?;
        let packages = resolve_closure(&self.tool_name(), &tv.version, target).await?;
        let mut result = BTreeMap::new();
        for package in packages
            .iter()
            .filter(|package| package.name != self.tool_name())
        {
            let bottle = resolve_bottle_info(&package.name, &package.version, target).await?;
            result.insert(
                pkgx_package_id(package),
                pkgx_package_info_with_metadata(
                    &bottle,
                    package.manifest.provided_bins(),
                    package.manifest.runtime_env_for_target(target),
                ),
            );
        }
        Ok(result)
    }
}

async fn resolve_closure(
    root_name: &str,
    root_version: &str,
    target: &PlatformTarget,
) -> Result<Vec<ResolvedPackage>> {
    let mut resolved: IndexMap<String, ResolvedPackage> = IndexMap::new();
    let mut requirements = BTreeMap::new();
    let mut pending = vec![root_name.to_string()];
    requirements.insert(root_name.to_string(), root_version.to_string());

    while let Some(name) = pending.pop() {
        if resolved.contains_key(&name) {
            continue;
        }
        let requirement = requirements
            .get(&name)
            .cloned()
            .ok_or_else(|| eyre::eyre!("missing pkgx requirement for {name}"))?;
        let version = resolve_version(&name, &requirement, target).await?;
        let manifest = fetch_manifest(&name).await?;
        let deps = manifest.dependencies_for_target(target);
        let companions = manifest.companions_for_target(target);

        for (dep_name, dep_requirement) in deps.into_iter().chain(companions) {
            if let Some(package) = resolved.get(&dep_name) {
                ensure_resolved_requirement(&dep_name, &package.version, &dep_requirement)?;
                continue;
            }
            let next_requirement = if let Some(existing) = requirements.get(&dep_name) {
                merge_requirements(&dep_name, existing, &dep_requirement)?
            } else {
                dep_requirement
            };
            if requirements.get(&dep_name) != Some(&next_requirement) {
                requirements.insert(dep_name.clone(), next_requirement);
                pending.push(dep_name);
            }
        }

        resolved.insert(
            name.to_string(),
            ResolvedPackage {
                name: name.to_string(),
                version,
                manifest,
            },
        );
    }

    Ok(resolved.into_values().collect())
}

async fn resolve_version(name: &str, requirement: &str, target: &PlatformTarget) -> Result<String> {
    let requirement = requirement.trim();
    if requirement.is_empty() || requirement == "*" || requirement.eq_ignore_ascii_case("latest") {
        return latest_version(name, target).await;
    }

    let versions = list_pkg_versions_for_target(name, target).await?;
    let version_strings = versions
        .iter()
        .map(|v| v.version.as_str())
        .collect::<Vec<_>>();

    if version_strings.contains(&requirement) {
        return Ok(requirement.to_string());
    }

    if let Some(stripped) = requirement.strip_prefix('v')
        && version_strings.contains(&stripped)
    {
        return Ok(stripped.to_string());
    }

    let range = parse_requirement_range(name, requirement)?;

    version_strings
        .iter()
        .rev()
        .find(|version| semver_satisfies(version, &range))
        .map(|version| (*version).to_string())
        .ok_or_else(|| eyre::eyre!("no pkgx version for {name} satisfies {requirement}"))
}

async fn latest_version(name: &str, target: &PlatformTarget) -> Result<String> {
    list_pkg_versions_for_target(name, target)
        .await?
        .last()
        .map(|v| v.version.clone())
        .ok_or_else(|| eyre::eyre!("no pkgx versions found for {name}"))
}

fn ensure_resolved_requirement(name: &str, version: &str, requirement: &str) -> Result<()> {
    if version_satisfies_requirement(version, requirement)? {
        Ok(())
    } else {
        bail!("resolved pkgx package {name}@{version} does not satisfy {requirement:?}")
    }
}

fn merge_requirements(name: &str, left: &str, right: &str) -> Result<String> {
    let left = left.trim();
    let right = right.trim();
    if is_any_requirement(left) {
        return Ok(right.to_string());
    }
    if is_any_requirement(right) || left == right {
        return Ok(left.to_string());
    }
    let merged = format!("{left} {right}");
    parse_requirement_range(name, &merged)?;
    Ok(merged)
}

fn version_satisfies_requirement(version: &str, requirement: &str) -> Result<bool> {
    let requirement = requirement.trim();
    if is_any_requirement(requirement) {
        return Ok(true);
    }
    if version == requirement || version.trim_start_matches(['v', 'V']) == requirement {
        return Ok(true);
    }
    Ok(semver_satisfies(
        version,
        &parse_requirement_range("pkgx package", requirement)?,
    ))
}

fn is_any_requirement(requirement: &str) -> bool {
    requirement.is_empty() || requirement == "*" || requirement.eq_ignore_ascii_case("latest")
}

fn parse_requirement_range(name: &str, requirement: &str) -> Result<Range> {
    Range::parse(requirement)
        .or_else(|_| Range::parse(format!("{requirement}.x")))
        .wrap_err_with(|| {
            format!("unsupported pkgx version requirement {requirement:?} for {name}")
        })
}

fn semver_satisfies(version: &str, range: &Range) -> bool {
    NodeVersion::parse(version)
        .or_else(|_| NodeVersion::parse(version.trim_start_matches(['v', 'V'])))
        .is_ok_and(|version| range.satisfies(&version))
}

async fn list_pkg_versions(name: &str) -> Result<Vec<VersionInfo>> {
    list_pkg_versions_for_target(name, &PlatformTarget::from_current()).await
}

async fn list_pkg_versions_for_target(
    name: &str,
    target: &PlatformTarget,
) -> Result<Vec<VersionInfo>> {
    let url = format!(
        "{DIST_URL}/{name}/{}/{}/versions.txt",
        pkgx_os_for_target(target),
        pkgx_arch_for_target(target)
    );
    let text = HTTP_FETCH
        .get_text(url)
        .await
        .wrap_err_with(|| format!("failed to list pkgx versions for {name}"))?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|version| VersionInfo {
            version: version.trim_start_matches('v').to_string(),
            ..Default::default()
        })
        .collect())
}

async fn resolve_bottle_info(
    name: &str,
    version: &str,
    target: &PlatformTarget,
) -> Result<PkgxPackageInfo> {
    let base = format!(
        "{DIST_URL}/{}/{}/{}/v{}",
        name,
        pkgx_os_for_target(target),
        pkgx_arch_for_target(target),
        version
    );
    let mut last_err = None;
    for extension in ["tar.xz", "tar.gz"] {
        let url = format!("{base}.{extension}");
        let checksum_url = format!("{url}.sha256sum");
        match HTTP_FETCH.get_text(&checksum_url).await {
            Ok(text) => {
                let checksum = text
                    .split_whitespace()
                    .next()
                    .ok_or_else(|| eyre::eyre!("empty pkgx checksum file for {name}"))?;
                return Ok(PkgxPackageInfo {
                    url,
                    checksum: Some(format!("sha256:{checksum}")),
                    pkgx_provides: None,
                    pkgx_runtime_env: None,
                });
            }
            Err(err) => match HTTP.head(&url).await {
                Ok(_) => {
                    return Ok(PkgxPackageInfo {
                        url,
                        checksum: None,
                        pkgx_provides: None,
                        pkgx_runtime_env: None,
                    });
                }
                Err(head_err) => {
                    debug!(
                        "pkgx bottle URL {url} was not reachable after checksum miss: {head_err}"
                    );
                    last_err = Some(err);
                }
            },
        }
    }
    Err(last_err.unwrap_or_else(|| eyre::eyre!("failed to resolve pkgx bottle for {name}")))
}

async fn fetch_manifest(name: &str) -> Result<PackageManifest> {
    let url = format!("{PANTRY_RAW_URL}/{name}/package.yml");
    let text = HTTP_FETCH
        .get_text(url)
        .await
        .wrap_err_with(|| format!("failed to fetch pkgx pantry manifest for {name}"))?;
    serde_yaml::from_str(&text)
        .wrap_err_with(|| format!("failed to parse pkgx manifest for {name}"))
}

async fn install_package(
    ctx: &InstallContext,
    tv: &ToolVersion,
    pkgx_root: &Path,
    package: &ResolvedPackage,
    bottle: &PkgxPackageInfo,
) -> Result<()> {
    let prefix = package_prefix(pkgx_root, &package.name, &package.version);
    if package_is_installed(&prefix, bottle)? {
        return Ok(());
    }

    let archive_path = download_bottle(ctx, tv, package, bottle).await?;
    let format = ExtractionFormat::from_file_name(&archive_path.to_string_lossy());
    if let Some(checksum) = bottle
        .checksum
        .as_deref()
        .and_then(|c| c.strip_prefix("sha256:"))
    {
        hash::ensure_checksum(&archive_path, checksum, Some(ctx.pr.as_ref()), "sha256")?;
    }

    let parent = prefix.parent().unwrap();
    file::create_dir_all(parent)?;
    let tmp = tempfile::Builder::new()
        .prefix("mise-pkgx-")
        .tempdir_in(parent)?;
    file::untar(
        &archive_path,
        tmp.path(),
        format,
        &ExtractOptions {
            strip_components: 0,
            pr: Some(ctx.pr.as_ref()),
            preserve_mtime: false,
        },
    )?;
    if tmp
        .path()
        .join(&package.name)
        .join(format!("v{}", package.version))
        .exists()
    {
        file::copy_dir_all(tmp.path(), pkgx_root)?;
    } else {
        file::rename(tmp.path(), &prefix)?;
    }
    if !prefix.exists() {
        bail!(
            "pkgx bottle for {} did not contain {}",
            package.name,
            prefix.display()
        );
    }
    file::write(package_receipt_path(&prefix), package_receipt(bottle))?;
    Ok(())
}

fn package_is_installed(prefix: &Path, bottle: &PkgxPackageInfo) -> Result<bool> {
    if !prefix.exists() {
        return Ok(false);
    }
    if fs::read_to_string(package_receipt_path(prefix)).ok() == Some(package_receipt(bottle)) {
        return Ok(true);
    }
    file::remove_all(prefix)?;
    Ok(false)
}

fn package_receipt_path(prefix: &Path) -> PathBuf {
    prefix.join(".mise-pkgx.toml")
}

fn package_receipt(bottle: &PkgxPackageInfo) -> String {
    let checksum = bottle.checksum.as_deref().unwrap_or_default();
    format!("url = {:?}\nchecksum = {:?}\n", bottle.url, checksum)
}

async fn download_bottle(
    ctx: &InstallContext,
    tv: &ToolVersion,
    package: &ResolvedPackage,
    bottle: &PkgxPackageInfo,
) -> Result<PathBuf> {
    let filename = bottle
        .url
        .rsplit('/')
        .next()
        .unwrap_or("pkgx-bottle.tar.xz");
    let archive_path = tv.download_path().join(format!(
        "pkgx-{}-{filename}",
        package.name.replace('/', "-")
    ));
    if archive_path.exists()
        && let Some(checksum) = bottle
            .checksum
            .as_deref()
            .and_then(|c| c.strip_prefix("sha256:"))
        && hash::ensure_checksum(&archive_path, checksum, None, "sha256").is_ok()
    {
        return Ok(archive_path);
    }

    ctx.pr
        .set_message(format!("download pkgx {}", package.name));
    HTTP.download_file(&bottle.url, &archive_path, Some(ctx.pr.as_ref()))
        .await?;
    Ok(archive_path)
}

fn write_wrappers(tv: &ToolVersion, packages: &[ResolvedPackage]) -> Result<()> {
    let root = packages
        .iter()
        .find(|package| package.name == tv.ba().tool_name)
        .or_else(|| packages.first())
        .ok_or_else(|| eyre::eyre!("pkgx install did not resolve any packages"))?;

    let bin_dir = tv.install_path().join("bin");
    file::create_dir_all(&bin_dir)?;
    let env = runtime_env(&pkgx_root(tv), packages, &root.name);
    let provides = root.manifest.provided_bins();
    let bins = if provides.is_empty() {
        discover_bins(&package_prefix(&pkgx_root(tv), &root.name, &root.version))?
    } else {
        provides
    };

    for rel_bin in bins {
        let exe_name = Path::new(&rel_bin)
            .file_name()
            .ok_or_else(|| eyre::eyre!("invalid pkgx provided binary path: {rel_bin}"))?
            .to_string_lossy()
            .to_string();
        let target = package_prefix(&pkgx_root(tv), &root.name, &root.version).join(&rel_bin);
        write_wrapper(&bin_dir.join(exe_name), &target, &env)?;
    }
    Ok(())
}

fn write_wrapper(path: &Path, target: &Path, env: &BTreeMap<String, String>) -> Result<()> {
    #[cfg(unix)]
    {
        let mut script = String::from("#!/usr/bin/env bash\n");
        for (key, value) in env {
            if is_path_env_key(key) {
                script.push_str("export ");
                script.push_str(key);
                script.push('=');
                script.push_str(&shell_quote(value));
                script.push_str("${");
                script.push_str(key);
                script.push_str(":+:${");
                script.push_str(key);
                script.push_str("}}\n");
            } else {
                script.push_str("export ");
                script.push_str(key);
                script.push('=');
                script.push_str(&shell_quote(value));
                script.push('\n');
            }
        }
        script.push_str("exec ");
        script.push_str(&shell_quote(&target.to_string_lossy()));
        script.push_str(" \"$@\"\n");
        file::write(path, script)?;
        file::make_executable(path)?;
    }

    #[cfg(windows)]
    {
        let mut script = String::from("@echo off\r\n");
        for (key, value) in env {
            script.push_str("set \"");
            script.push_str(key);
            script.push('=');
            script.push_str(&cmd_escape_value(value));
            if is_path_env_key(key) {
                script.push_str(";%");
                script.push_str(key);
                script.push('%');
            }
            script.push_str("\"\r\n");
        }
        script.push('"');
        script.push_str(&cmd_escape_value(&target.to_string_lossy()));
        script.push_str("\" %*\r\n");
        file::write(path.with_extension("cmd"), script)?;
    }

    Ok(())
}

fn runtime_env(
    pkgx_root: &Path,
    packages: &[ResolvedPackage],
    root_name: &str,
) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    prepend_env_paths(&mut env, "PATH", pkg_paths(pkgx_root, packages, "bin"));
    prepend_env_paths(&mut env, "PATH", pkg_paths(pkgx_root, packages, "sbin"));
    prepend_env_paths(
        &mut env,
        "MANPATH",
        pkg_paths(pkgx_root, packages, "share/man"),
    );
    prepend_env_paths(
        &mut env,
        "PKG_CONFIG_PATH",
        pkg_paths(pkgx_root, packages, "lib/pkgconfig"),
    );
    prepend_env_paths(
        &mut env,
        "LIBRARY_PATH",
        pkg_paths(pkgx_root, packages, "lib"),
    );
    prepend_env_paths(
        &mut env,
        "LD_LIBRARY_PATH",
        pkg_paths(pkgx_root, packages, "lib"),
    );
    prepend_env_paths(
        &mut env,
        "DYLD_FALLBACK_LIBRARY_PATH",
        pkg_paths(pkgx_root, packages, "lib"),
    );
    prepend_env_paths(&mut env, "CPATH", pkg_paths(pkgx_root, packages, "include"));
    prepend_env_paths(
        &mut env,
        "XDG_DATA_DIRS",
        pkg_paths(pkgx_root, packages, "share"),
    );

    for package in packages
        .iter()
        .filter(|package| package.name != root_name)
        .chain(packages.iter().filter(|package| package.name == root_name))
    {
        let prefix = package_prefix(pkgx_root, &package.name, &package.version);
        for (key, value) in package.manifest.runtime_env_for_current_platform() {
            env.insert(key, render_pkgx_env_value(&value, &prefix, &env));
        }
    }

    if let Some(ca_certs) = packages
        .iter()
        .map(|p| package_prefix(pkgx_root, &p.name, &p.version).join("ssl/cert.pem"))
        .find(|p| p.exists())
    {
        env.entry("SSL_CERT_FILE".into())
            .or_insert_with(|| ca_certs.to_string_lossy().to_string());
    }

    env
}

fn pkg_paths(pkgx_root: &Path, packages: &[ResolvedPackage], rel: &str) -> Vec<PathBuf> {
    packages
        .iter()
        .map(|p| package_prefix(pkgx_root, &p.name, &p.version).join(rel))
        .filter(|path| path.exists())
        .collect()
}

fn prepend_env_paths(env: &mut BTreeMap<String, String>, key: &str, paths: Vec<PathBuf>) {
    if paths.is_empty() {
        return;
    }
    let joined = std::env::join_paths(paths)
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    env.entry(key.to_string())
        .and_modify(|existing| {
            let sep = if cfg!(windows) { ";" } else { ":" };
            *existing = format!("{joined}{sep}{}", existing);
        })
        .or_insert(joined);
}

fn is_path_env_key(key: &str) -> bool {
    matches!(
        key,
        "PATH"
            | "MANPATH"
            | "PKG_CONFIG_PATH"
            | "LIBRARY_PATH"
            | "LD_LIBRARY_PATH"
            | "DYLD_FALLBACK_LIBRARY_PATH"
            | "CPATH"
            | "XDG_DATA_DIRS"
    )
}

fn render_pkgx_env_value(
    value: &str,
    prefix: &Path,
    current_env: &BTreeMap<String, String>,
) -> String {
    let mut rendered = value
        .replace("{{prefix}}", &prefix.to_string_lossy())
        .replace("{{ prefix }}", &prefix.to_string_lossy());
    for (key, env_value) in current_env {
        rendered = rendered.replace(&format!("${key}"), env_value);
    }
    rendered
}

fn discover_bins(prefix: &Path) -> Result<Vec<String>> {
    let bin = prefix.join("bin");
    if !bin.exists() {
        return Ok(vec![]);
    }
    Ok(file::ls(&bin)?
        .into_iter()
        .filter(|path| path.is_file())
        .filter_map(|path| {
            path.file_name()
                .map(|name| format!("bin/{}", name.to_string_lossy()))
        })
        .collect())
}

fn pkgx_root(tv: &ToolVersion) -> PathBuf {
    tv.install_path().join("pkgx-root")
}

fn package_prefix(pkgx_root: &Path, name: &str, version: &str) -> PathBuf {
    pkgx_root.join(name).join(format!("v{version}"))
}

fn pkgx_package_id(package: &ResolvedPackage) -> String {
    format!("{}@{}", package.name, package.version)
}

fn parse_pkgx_package_id(id: &str) -> Result<(String, String)> {
    let (name, version) = id
        .rsplit_once('@')
        .ok_or_else(|| eyre::eyre!("invalid pkgx package id {id:?}"))?;
    Ok((name.to_string(), version.to_string()))
}

fn pkgx_os_for_target(target: &PlatformTarget) -> String {
    match target.os_name() {
        "macos" => "darwin".to_string(),
        "linux" => "linux".to_string(),
        "windows" => "windows".to_string(),
        os => os.to_string(),
    }
}

fn pkgx_arch_for_target(target: &PlatformTarget) -> String {
    match target.arch_name() {
        "x64" => "x86-64".to_string(),
        "arm64" => "aarch64".to_string(),
        arch => arch.to_string(),
    }
}

#[cfg(any(unix, test))]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(any(windows, test))]
fn cmd_escape_value(value: &str) -> String {
    value
        .replace('^', "^^")
        .replace('%', "%%")
        .replace('&', "^&")
        .replace('|', "^|")
        .replace('<', "^<")
        .replace('>', "^>")
        .replace('"', "^\"")
}

impl PackageManifest {
    fn from_locked_metadata(
        provides: Option<Vec<String>>,
        runtime_env: Option<BTreeMap<String, String>>,
    ) -> Self {
        PackageManifest {
            provides: provides.unwrap_or_default(),
            runtime: RuntimeManifest {
                env: value_from_string_map(runtime_env.unwrap_or_default()),
            },
            ..Default::default()
        }
    }

    fn provided_bins(&self) -> Vec<String> {
        self.provides
            .iter()
            .filter(|provide| provide.starts_with("bin/") || provide.starts_with("sbin/"))
            .cloned()
            .collect()
    }

    fn dependencies_for_target(&self, target: &PlatformTarget) -> Vec<(String, String)> {
        dependency_map_for_target(&self.dependencies, target)
    }

    fn companions_for_target(&self, target: &PlatformTarget) -> Vec<(String, String)> {
        dependency_map_for_target(&self.companions, target)
    }

    fn runtime_env_for_current_platform(&self) -> Vec<(String, String)> {
        string_map_for_current_platform(&self.runtime.env)
    }

    fn runtime_env_for_target(&self, target: &PlatformTarget) -> Vec<(String, String)> {
        string_map_for_target(&self.runtime.env, target)
    }
}

fn dependency_map_for_target(value: &Value, target: &PlatformTarget) -> Vec<(String, String)> {
    string_map_for_target(value, target)
}

fn string_map_for_current_platform(value: &Value) -> Vec<(String, String)> {
    string_map_for_target(value, &PlatformTarget::from_current())
}

fn string_map_for_target(value: &Value, target: &PlatformTarget) -> Vec<(String, String)> {
    let mut out = BTreeMap::new();
    collect_string_map(value, &mut out);
    if let Some(mapping) = value.as_mapping() {
        for key in platform_keys_for_target(target) {
            if let Some(nested) = mapping.get(Value::String(key)) {
                collect_string_map(nested, &mut out);
            }
        }
    }
    out.into_iter().collect()
}

fn collect_string_map(value: &Value, out: &mut BTreeMap<String, String>) {
    let Some(mapping) = value.as_mapping() else {
        return;
    };
    for (key, value) in mapping {
        let Some(key) = yaml_string(key) else {
            continue;
        };
        if is_platform_key(&key) {
            continue;
        }
        if let Some(value) = yaml_string(value) {
            out.insert(key, value);
        }
    }
}

fn platform_keys_for_target(target: &PlatformTarget) -> Vec<String> {
    vec![
        pkgx_os_for_target(target),
        pkgx_arch_for_target(target),
        format!(
            "{}/{}",
            pkgx_os_for_target(target),
            pkgx_arch_for_target(target)
        ),
    ]
}

fn is_platform_key(key: &str) -> bool {
    if matches!(key, "linux" | "darwin" | "windows" | "x86-64" | "aarch64") {
        return true;
    }
    let Some((os, arch)) = key.split_once('/') else {
        return false;
    };
    matches!(os, "linux" | "darwin" | "windows") && matches!(arch, "x86-64" | "aarch64")
}

fn yaml_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn pkgx_package_info_with_metadata(
    bottle: &PkgxPackageInfo,
    provides: Vec<String>,
    runtime_env: Vec<(String, String)>,
) -> PkgxPackageInfo {
    PkgxPackageInfo {
        url: bottle.url.clone(),
        checksum: bottle.checksum.clone(),
        pkgx_provides: optional_vec(provides),
        pkgx_runtime_env: optional_map(runtime_env.into_iter().collect()),
    }
}

fn optional_vec(values: Vec<String>) -> Option<Vec<String>> {
    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}

fn optional_map(values: BTreeMap<String, String>) -> Option<BTreeMap<String, String>> {
    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}

fn value_from_string_map(values: BTreeMap<String, String>) -> Value {
    let mut mapping = Mapping::new();
    for (key, value) in values {
        mapping.insert(Value::String(key), Value::String(value));
    }
    Value::Mapping(mapping)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_platform_dependencies() {
        let manifest: PackageManifest = serde_yaml::from_str(
            r#"
dependencies:
  openssl.org: 1.1
  linux:
    zlib.net: 1
  darwin/aarch64:
    apple.com/xcode/clt: '*'
  github.com/kkos/oniguruma: 6
"#,
        )
        .unwrap();

        let target = PlatformTarget::from_current();
        let deps = manifest.dependencies_for_target(&target);
        assert!(
            deps.iter()
                .any(|(name, req)| name == "openssl.org" && req == "1.1")
        );
        assert!(
            deps.iter()
                .any(|(name, req)| name == "github.com/kkos/oniguruma" && req == "6")
        );
        if target.os_name() == "linux" {
            assert!(
                deps.iter()
                    .any(|(name, req)| name == "zlib.net" && req == "1")
            );
        }
    }

    #[test]
    fn preserves_short_dependency_names() {
        let manifest: PackageManifest = serde_yaml::from_str(
            r#"
dependencies:
  zlib: 1
"#,
        )
        .unwrap();
        let target = PlatformTarget::from_current();

        assert_eq!(
            manifest.dependencies_for_target(&target),
            vec![("zlib".to_string(), "1".to_string())]
        );
    }

    #[test]
    fn resolves_dependencies_for_target_platform() {
        let manifest: PackageManifest = serde_yaml::from_str(
            r#"
dependencies:
  common.example: 1
  linux/x86-64:
    linux.example: 2
  darwin/aarch64:
    macos.example: 3
"#,
        )
        .unwrap();
        let target = PlatformTarget::new(crate::platform::Platform::parse("linux-x64").unwrap());
        let deps = manifest.dependencies_for_target(&target);

        assert!(
            deps.iter()
                .any(|(name, req)| name == "common.example" && req == "1")
        );
        assert!(
            deps.iter()
                .any(|(name, req)| name == "linux.example" && req == "2")
        );
        assert!(
            !deps
                .iter()
                .any(|(name, req)| name == "macos.example" && req == "3")
        );
    }

    #[test]
    fn quotes_shell_values() {
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn escapes_cmd_values() {
        assert_eq!(
            cmd_escape_value(r#"C:\pkgx & "bin"\100%"#),
            r#"C:\pkgx ^& ^"bin^"\100%%"#
        );
        assert_eq!(cmd_escape_value("a|b<c>d"), "a^|b^<c^>d");
        assert_eq!(cmd_escape_value("a^b"), "a^^b");
    }
}
