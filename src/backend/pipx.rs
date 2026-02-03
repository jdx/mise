use crate::backend::backend_type::BackendType;
use crate::backend::platform_target::PlatformTarget;
use crate::backend::{Backend, VersionInfo};
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::env;
use crate::file;
use crate::github;
use crate::http::HTTP_FETCH;
use crate::install_context::InstallContext;
use crate::timeout;
use crate::toolset::{ToolRequest, ToolVersion, ToolVersionOptions, Toolset, ToolsetBuilder};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::SingleReport;
use async_trait::async_trait;
use eyre::{Result, eyre};
use indexmap::IndexMap;
use itertools::Itertools;
use regex::Regex;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::{fmt::Debug, sync::Arc};
use versions::Versioning;
use xx::regex;

#[derive(Debug)]
pub struct PIPXBackend {
    ba: Arc<BackendArg>,
    latest_version_cache: CacheManager<Option<String>>,
}

#[async_trait]
impl Backend for PIPXBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Pipx
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    fn get_dependencies(&self) -> eyre::Result<Vec<&str>> {
        Ok(vec!["pipx"])
    }

    fn get_optional_dependencies(&self) -> eyre::Result<Vec<&str>> {
        Ok(vec!["uv"])
    }

    /// Pipx installs packages from PyPI or Git using version specs (e.g., black==24.3.0).
    /// It doesn't support installing from direct URLs, so lockfile URLs are not applicable.
    fn supports_lockfile_url(&self) -> bool {
        false
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> eyre::Result<Vec<VersionInfo>> {
        match self.tool_name().parse()? {
            PipxRequest::Pypi(package) => {
                let registry_url = Self::get_registry_url()?;
                if registry_url.contains("/json") {
                    debug!("Fetching JSON for {}", package);
                    let url = registry_url.replace("{}", &package);
                    let data: PypiPackage = HTTP_FETCH.json(url).await?;

                    // Get versions sorted and attach timestamps from the first file in each release
                    let versions = data
                        .releases
                        .into_iter()
                        .sorted_by_cached_key(|(v, _)| Versioning::new(v))
                        .map(|(version, files)| {
                            // Get the earliest upload_time from the release files
                            let created_at = files
                                .iter()
                                .filter_map(|f| f.upload_time.as_ref())
                                .min()
                                .cloned();
                            VersionInfo {
                                version,
                                created_at,
                                ..Default::default()
                            }
                        })
                        .collect();

                    Ok(versions)
                } else {
                    debug!("Fetching HTML for {}", package);
                    let url = registry_url.replace("{}", &package);
                    let html = HTTP_FETCH.get_html(url).await?;

                    // PEP-0503 (HTML format doesn't include timestamps)
                    let version_re = regex!(
                        r#"href=["'][^"']*/([^/]+)\.tar\.gz(?:#(md5|sha1|sha224|sha256|sha384|sha512)=[0-9A-Fa-f]+)?["']"#
                    );

                    let versions: Vec<VersionInfo> = version_re
                        .captures_iter(&html)
                        .filter_map(|cap| {
                            let filename = cap.get(1)?.as_str();
                            let escaped_package = regex::escape(&package);
                            // PEP-503: normalize package names by replacing hyphens with character class that allows -, _, .
                            let re_str = escaped_package.replace(r"\-", r"[\-_.]");
                            let re_str = format!("^{re_str}-(.+)$");
                            let pkg_re = regex::Regex::new(&re_str).ok()?;
                            let pkg_version = pkg_re.captures(filename)?.get(1)?.as_str();
                            Some(VersionInfo {
                                version: pkg_version.to_string(),
                                ..Default::default()
                            })
                        })
                        .sorted_by_cached_key(|v| Versioning::new(&v.version))
                        .collect();

                    Ok(versions)
                }
            }
            PipxRequest::Git(url) if url.starts_with("https://github.com/") => {
                let repo = url.strip_prefix("https://github.com/").unwrap();
                let data = github::list_releases(repo).await?;
                Ok(data
                    .into_iter()
                    .rev()
                    .map(|r| VersionInfo {
                        version: r.tag_name,
                        created_at: Some(r.created_at),
                        ..Default::default()
                    })
                    .collect())
            }
            PipxRequest::Git { .. } => Ok(vec![VersionInfo {
                version: "latest".to_string(),
                ..Default::default()
            }]),
        }
    }

    async fn latest_stable_version(&self, config: &Arc<Config>) -> eyre::Result<Option<String>> {
        let this = self;
        timeout::run_with_timeout_async(
            async || {
                this.latest_version_cache
                    .get_or_try_init_async(async || match this.tool_name().parse()? {
                        PipxRequest::Pypi(package) => {
                            let registry_url = Self::get_registry_url()?;
                            if registry_url.contains("/json") {
                                debug!("Fetching JSON for {}", package);
                                let url = registry_url.replace("{}", &package);
                                let pkg: PypiPackage = HTTP_FETCH.json(url).await?;
                                Ok(Some(pkg.info.version))
                            } else {
                                debug!("Fetching HTML for {}", package);
                                let url = registry_url.replace("{}", &package);
                                let html = HTTP_FETCH.get_html(url).await?;

                                 // PEP-0503
                                let version_re = regex!(r#"href=["'][^"']*/([^/]+)\.tar\.gz(?:#(md5|sha1|sha224|sha256|sha384|sha512)=[0-9A-Fa-f]+)?["']"#);

                                let version = version_re
                                    .captures_iter(&html)
                                    .filter_map(|cap| {
                                        let filename = cap.get(1)?.as_str();
                                        let escaped_package = regex::escape(&package);
                                        // PEP-503: normalize package names by replacing hyphens with character class that allows -, _, .
                                        let re_str = escaped_package.replace(r"\-", r"[\-_.]");
                                        let re_str = format!("^{re_str}-(.+)$");
                                        let pkg_re = regex::Regex::new(&re_str).ok()?;
                                        let pkg_version =
                                            pkg_re.captures(filename)?.get(1)?.as_str();
                                        Some(pkg_version.to_string())
                                    })
                                    .filter(|v| {
                                        !v.contains("dev")
                                            && !v.contains("a")
                                            && !v.contains("b")
                                            && !v.contains("rc")
                                    })
                                    .sorted_by_cached_key(|v| Versioning::new(v))
                                    .next_back();

                                Ok(version)
                            }
                        }
                        _ => this.latest_version(config, Some("latest".into())).await,
                    })
                    .await
            },
            Settings::get().fetch_remote_versions_timeout(),
        )
        .await
        .cloned()
    }

    async fn install_version_(&self, ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion> {
        // Check if pipx is available (unless uvx is being used)
        let use_uvx = self.uv_is_installed(&ctx.config).await
            && Settings::get().pipx.uvx != Some(false)
            && tv.request.options().get("uvx") != Some(&"false".to_string());

        if !use_uvx {
            self.warn_if_dependency_missing(
                &ctx.config,
                "pipx",
                "To use pipx packages with mise, you need to install pipx first:\n\
                  mise use pipx@latest\n\n\
                Alternatively, you can use uv/uvx by installing uv:\n\
                  mise use uv@latest",
            )
            .await;
        }

        let pipx_request = self
            .tool_name()
            .parse::<PipxRequest>()?
            .pipx_request(&tv.version, &tv.request.options());

        if use_uvx {
            ctx.pr
                .set_message(format!("uv tool install {pipx_request}"));
            let mut cmd = Self::uvx_cmd(
                &ctx.config,
                &["tool", "install", &pipx_request],
                self,
                &tv,
                &ctx.ts,
                ctx.pr.as_ref(),
            )
            .await?;
            if let Some(args) = tv.request.options().get("uvx_args") {
                cmd = cmd.args(shell_words::split(args)?);
            }
            cmd.execute()?;
        } else {
            ctx.pr.set_message(format!("pipx install {pipx_request}"));
            let mut cmd = Self::pipx_cmd(
                &ctx.config,
                &["install", &pipx_request],
                self,
                &tv,
                &ctx.ts,
                ctx.pr.as_ref(),
            )
            .await?;
            if let Some(args) = tv.request.options().get("pipx_args") {
                cmd = cmd.args(shell_words::split(args)?);
            }
            cmd.execute()?;
        }

        // Fix venv Python symlink to use minor version path
        // This allows patch upgrades (3.12.1 → 3.12.2) to work without reinstalling
        let pkg_name = self.tool_name();
        fix_venv_python_symlink(&tv.install_path(), &pkg_name)?;

        Ok(tv)
    }

    fn resolve_lockfile_options(
        &self,
        request: &ToolRequest,
        _target: &PlatformTarget,
    ) -> BTreeMap<String, String> {
        let opts = request.options();
        let mut result = BTreeMap::new();

        // These options affect what gets installed
        for key in ["extras", "pipx_args", "uvx_args", "uvx"] {
            if let Some(value) = opts.get(key) {
                result.insert(key.to_string(), value.clone());
            }
        }

        result
    }
}

/// Returns install-time-only option keys for PIPX backend.
pub fn install_time_option_keys() -> Vec<String> {
    vec![
        "extras".into(),
        "pipx_args".into(),
        "uvx_args".into(),
        "uvx".into(),
    ]
}

impl PIPXBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self {
            latest_version_cache: CacheManagerBuilder::new(
                ba.cache_path.join("latest_version.msgpack.z"),
            )
            .with_fresh_duration(Settings::get().fetch_remote_versions_cache())
            .build(),
            ba: Arc::new(ba),
        }
    }

    fn get_index_url() -> eyre::Result<String> {
        let registry_url = Settings::get().pipx.registry_url.clone();

        // Remove {} placeholders and trailing slashes
        let mut url = registry_url
            .replace("{}", "")
            .trim_end_matches('/')
            .to_string();

        // Handle different URL formats and convert to simple format
        if url.contains("pypi.org") {
            // For pypi.org, convert any format to simple format
            if url.contains("/pypi/") {
                // Replace /pypi/*/json or /pypi/*/simple with /simple
                let re = Regex::new(r"/pypi/[^/]*/(?:json|simple)$").unwrap();
                url = re.replace(&url, "/simple").to_string();
            } else if !url.ends_with("/simple") {
                // If it's pypi.org but doesn't already end with /simple, make it /simple
                let base_url = url.split("/simple").next().unwrap_or(&url);
                url = format!("{}/simple", base_url.trim_end_matches('/'));
            }
        } else {
            // For custom registries, ensure they end with /simple
            if url.ends_with("/json") {
                // Replace /json with /simple
                url = url.replace("/json", "/simple");
            } else if !url.ends_with("/simple") {
                // If it doesn't end with /simple, append it
                url = format!("{url}/simple");
            }
        }

        debug!("Converted registry URL to index URL: {}", url);
        Ok(url)
    }

    fn get_registry_url() -> eyre::Result<String> {
        let registry_url = Settings::get().pipx.registry_url.clone();

        debug!("Pipx registry URL: {}", registry_url);

        let re = Regex::new(r"^(http|https)://.*\{\}.*$").unwrap();

        if !re.is_match(&registry_url) {
            return Err(eyre!(
                "Registry URL must be a valid URL and contain a {{}} placeholder"
            ));
        }

        Ok(registry_url)
    }

    pub async fn reinstall_all(config: &Arc<Config>) -> Result<()> {
        let ts = ToolsetBuilder::new().build(config).await?;
        let pipx_tools = ts
            .list_installed_versions(config)
            .await?
            .into_iter()
            .filter(|(b, _tv)| b.ba().backend_type() == BackendType::Pipx)
            .collect_vec();
        if Settings::get().pipx.uvx != Some(false) {
            let pr = MultiProgressReport::get().add("reinstalling pipx tools with uvx");
            for (b, tv) in pipx_tools {
                for (cmd, tool) in &[
                    ("uninstall", tv.ba().tool_name.to_string()),
                    ("install", format!("{}=={}", tv.ba().tool_name, tv.version)),
                ] {
                    let args = &["tool", cmd, tool];
                    Self::uvx_cmd(config, args, &*b, &tv, &ts, pr.as_ref())
                        .await?
                        .execute()?;
                }
            }
        } else {
            let pr = MultiProgressReport::get().add("reinstalling pipx tools");
            for (b, tv) in pipx_tools {
                let args = &["reinstall", &tv.ba().tool_name];
                Self::pipx_cmd(config, args, &*b, &tv, &ts, pr.as_ref())
                    .await?
                    .execute()?;
            }
        }
        Ok(())
    }

    async fn uvx_cmd<'a>(
        config: &Arc<Config>,
        args: &[&str],
        b: &dyn Backend,
        tv: &ToolVersion,
        ts: &Toolset,
        pr: &'a dyn SingleReport,
    ) -> Result<CmdLineRunner<'a>> {
        let mut cmd = CmdLineRunner::new("uv");
        for arg in args {
            cmd = cmd.arg(arg);
        }
        cmd.with_pr(pr)
            .env("UV_TOOL_DIR", tv.install_path())
            .env("UV_TOOL_BIN_DIR", tv.install_path().join("bin"))
            .env("UV_INDEX", Self::get_index_url()?)
            .envs(ts.env_with_path(config).await?)
            .prepend_path(ts.list_paths(config).await)?
            .prepend_path(vec![tv.install_path().join("bin")])?
            .prepend_path(b.dependency_toolset(config).await?.list_paths(config).await)
    }

    async fn pipx_cmd<'a>(
        config: &Arc<Config>,
        args: &[&str],
        b: &dyn Backend,
        tv: &ToolVersion,
        ts: &Toolset,
        pr: &'a dyn SingleReport,
    ) -> Result<CmdLineRunner<'a>> {
        let mut cmd = CmdLineRunner::new("pipx");
        for arg in args {
            cmd = cmd.arg(arg);
        }
        cmd.with_pr(pr)
            .env("PIPX_HOME", tv.install_path())
            .env("PIPX_BIN_DIR", tv.install_path().join("bin"))
            .env("PIP_INDEX_URL", Self::get_index_url()?)
            .envs(ts.env_with_path(config).await?)
            .prepend_path(ts.list_paths(config).await)?
            .prepend_path(vec![tv.install_path().join("bin")])?
            .prepend_path(b.dependency_toolset(config).await?.list_paths(config).await)
    }

    async fn uv_is_installed(&self, config: &Arc<Config>) -> bool {
        self.dependency_which(config, "uv").await.is_some()
    }
}

enum PipxRequest {
    /// git+https://github.com/psf/black.git@24.2.0
    /// psf/black@24.2.0
    Git(String),
    /// black@24.2.0
    Pypi(String),
}

impl PipxRequest {
    fn extras_from_opts(&self, opts: &ToolVersionOptions) -> String {
        match opts.get("extras") {
            Some(extras) => format!("[{extras}]"),
            None => String::new(),
        }
    }

    fn pipx_request(&self, v: &str, opts: &ToolVersionOptions) -> String {
        let extras = self.extras_from_opts(opts);

        if v == "latest" {
            match self {
                PipxRequest::Git(url) => format!("git+{url}.git"),
                PipxRequest::Pypi(package) => format!("{package}{extras}"),
            }
        } else {
            match self {
                PipxRequest::Git(url) => format!("git+{url}.git@{v}"),
                PipxRequest::Pypi(package) => format!("{package}{extras}=={v}"),
            }
        }
    }
}

#[derive(serde::Deserialize)]
struct PypiPackage {
    releases: IndexMap<String, Vec<PypiRelease>>,
    info: PypiInfo,
}

#[derive(serde::Deserialize)]
struct PypiInfo {
    version: String,
}

#[derive(serde::Deserialize)]
struct PypiRelease {
    upload_time: Option<String>,
}

impl FromStr for PipxRequest {
    type Err = eyre::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(cap) = regex!(r"(git\+)(.*)(\.git)").captures(s) {
            Ok(PipxRequest::Git(cap.get(2).unwrap().as_str().to_string()))
        } else if s.contains('/') {
            Ok(PipxRequest::Git(format!("https://github.com/{s}")))
        } else {
            Ok(PipxRequest::Pypi(s.to_string()))
        }
    }
}

/// Check if a path is within mise's Python installs directory
#[cfg(unix)]
fn is_mise_managed_python(path: &Path) -> bool {
    let installs_dir = &*env::MISE_INSTALLS_DIR;
    path.starts_with(installs_dir.join("python"))
}

/// Convert a Python path with full version to use minor version
/// e.g., .../python/3.12.1/bin/python → .../python/3.12/bin/python
#[cfg(unix)]
fn path_with_minor_version(path: &Path) -> Option<PathBuf> {
    let path_str = path.to_str()?;

    // Match pattern: /python/X.Y.Z/ and replace with /python/X.Y/
    let re = regex!(r"/python/(\d+)\.(\d+)\.\d+/");
    if re.is_match(path_str) {
        let result = re.replace(path_str, "/python/$1.$2/");
        Some(PathBuf::from(result.to_string()))
    } else {
        None
    }
}

/// Ensure the minor version symlink exists for a Python installation path.
/// For example, if the path is `.../python/3.12.1/bin/python3`, this ensures
/// that `.../python/3.12` exists as a symlink to `./3.12.1`.
///
/// This is normally done by `runtime_symlinks::rebuild()`, but that runs after
/// postinstall hooks. We need to create it early so that venv symlinks work
/// immediately for postinstall hooks.
#[cfg(unix)]
fn ensure_minor_version_symlink(full_version_path: &Path) -> Result<()> {
    // Extract version components from path like .../python/3.12.1/bin/python3
    // Use same regex pattern as path_with_minor_version for consistency
    let re = regex!(r"/python/(\d+)\.(\d+)\.(\d+)/");
    let path_str = match full_version_path.to_str() {
        Some(s) => s,
        None => return Ok(()),
    };

    let caps = match re.captures(path_str) {
        Some(c) => c,
        None => return Ok(()),
    };

    let minor_version = format!("{}.{}", &caps[1], &caps[2]); // e.g., "3.12"
    let full_version = format!("{}.{}.{}", &caps[1], &caps[2], &caps[3]); // e.g., "3.12.1"

    let installs_dir = &*env::MISE_INSTALLS_DIR;
    let python_installs = installs_dir.join("python");
    let minor_version_dir = python_installs.join(&minor_version);
    let full_version_dir = python_installs.join(&full_version);

    // Only create if the minor version symlink doesn't exist but the full version does
    if !minor_version_dir.exists() && full_version_dir.exists() {
        trace!(
            "Creating early minor version symlink: {:?} -> ./{:?}",
            minor_version_dir, full_version
        );
        // Use relative symlink with "./" prefix like runtime_symlinks does
        // This allows is_runtime_symlink() to identify it for cleanup/updates
        file::make_symlink(&PathBuf::from(".").join(&full_version), &minor_version_dir)?;
    }

    Ok(())
}

/// Fix the venv Python symlinks to use mise's minor version path
/// This allows patch upgrades (3.12.1 → 3.12.2) to work without reinstalling
///
/// The venv structure typically has:
/// - python -> python3 (relative symlink)
/// - python3 -> /path/to/mise/installs/python/3.12.1/bin/python3 (absolute symlink)
///
/// We need to fix the absolute symlink to use minor version path (3.12 instead of 3.12.1)
#[cfg(unix)]
fn fix_venv_python_symlink(install_path: &Path, pkg_name: &str) -> Result<()> {
    // For Git-based packages like "psf/black", the venv directory is just "black"
    // Extract the actual package name (last component after any '/')
    let actual_pkg_name = pkg_name.rsplit('/').next().unwrap_or(pkg_name);

    // Check both possible venv locations: {pkg}/ for uvx, venvs/{pkg}/ for pipx
    let venv_dirs = [
        install_path.join(actual_pkg_name),
        install_path.join("venvs").join(actual_pkg_name),
    ];

    trace!(
        "fix_venv_python_symlink: checking venv dirs: {:?}",
        venv_dirs
    );

    for venv_dir in &venv_dirs {
        let bin_dir = venv_dir.join("bin");
        if !bin_dir.exists() {
            continue;
        }

        // Check python, python3, and python3.X symlinks for the one with absolute mise path
        for name in &["python", "python3"] {
            let symlink_path = bin_dir.join(name);
            if !symlink_path.is_symlink() {
                continue;
            }

            let target = match file::resolve_symlink(&symlink_path)? {
                Some(t) => t,
                None => continue,
            };

            // Skip relative symlinks (like python -> python3)
            if !target.is_absolute() {
                continue;
            }

            if !is_mise_managed_python(&target) {
                continue; // Leave non-mise Python alone (homebrew, uv, etc.)
            }

            if let Some(minor_path) = path_with_minor_version(&target)
                && target.exists()
            {
                // Create the minor version symlink (e.g., python/3.12 -> python/3.12.1)
                // if it doesn't exist yet. This is normally done by runtime_symlinks::rebuild,
                // but that runs after postinstall hooks, so we need to create it now
                // to ensure the venv symlink works immediately for postinstall hooks.
                ensure_minor_version_symlink(&target)?;

                trace!(
                    "Updating venv Python symlink {:?} to use minor version: {:?}",
                    symlink_path, minor_path
                );
                file::make_symlink(&minor_path, &symlink_path)?;
            }
        }
    }
    Ok(())
}

/// No-op on non-Unix platforms
#[cfg(not(unix))]
fn fix_venv_python_symlink(_install_path: &Path, _pkg_name: &str) -> Result<()> {
    Ok(())
}
