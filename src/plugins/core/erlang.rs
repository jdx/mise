use std::collections::BTreeMap;
use std::{path::PathBuf, sync::Arc};

use crate::backend::Backend;
use crate::backend::VersionInfo;
use crate::backend::platform_target::PlatformTarget;
use crate::cli::args::BackendArg;
use crate::config::{Config, Settings};
#[cfg(unix)]
use crate::file::ExtractOptions;
use crate::file::display_path;
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::lock_file::LockFile;
use crate::lockfile::PlatformInfo;
use crate::platform::{Platform, linux_os_release};
use crate::toolset::{ToolRequest, ToolVersion};
use crate::{file, github, plugins};
use async_trait::async_trait;
use eyre::{Result, bail};
use xx::regex;

#[cfg(linux)]
use crate::cmd::CmdLineRunner;
#[cfg(linux)]
use std::fs;

#[derive(Debug)]
pub struct ErlangPlugin {
    ba: Arc<BackendArg>,
}

const KERL_VERSION: &str = "4.4.0";
const ERLANG_PRECOMPILED_OPTION: &str = "precompiled";
const ERLANG_PRECOMPILED_OS_OPTION: &str = "precompiled_os";

impl ErlangPlugin {
    pub fn new() -> Self {
        Self {
            ba: Arc::new(plugins::core::new_backend_arg("erlang")),
        }
    }

    fn kerl_path(&self) -> PathBuf {
        self.ba.cache_path.join(format!("kerl-{KERL_VERSION}"))
    }

    fn kerl_base_dir(&self) -> PathBuf {
        self.ba.cache_path.join("kerl")
    }

    fn lock_build_tool(&self) -> Result<fslock::LockFile> {
        LockFile::new(&self.kerl_path())
            .with_callback(|l| {
                trace!("install_or_update_kerl {}", l.display());
            })
            .lock()
    }

    async fn update_kerl(&self) -> Result<()> {
        let _lock = self.lock_build_tool();
        if self.kerl_path().exists() {
            // TODO: find a way to not have to do this #1209
            file::remove_all(self.kerl_base_dir())?;
            return Ok(());
        }
        self.install_kerl().await?;
        let output = cmd!(self.kerl_path(), "update", "releases")
            .env("KERL_BASE_DIR", self.kerl_base_dir())
            .stdout_capture()
            .stderr_capture()
            .run()?;
        trace!("kerl stdout: {}", String::from_utf8_lossy(&output.stdout));
        trace!("kerl stderr: {}", String::from_utf8_lossy(&output.stderr));
        Ok(())
    }

    async fn install_kerl(&self) -> Result<()> {
        debug!("Installing kerl to {}", display_path(self.kerl_path()));
        HTTP_FETCH
            .download_file(
                format!("https://raw.githubusercontent.com/kerl/kerl/{KERL_VERSION}/kerl"),
                &self.kerl_path(),
                None,
            )
            .await?;
        file::make_executable(self.kerl_path())?;
        Ok(())
    }

    fn precompiled_unavailable(&self, reason: impl Into<String>) -> Result<Option<ToolVersion>> {
        let reason = reason.into();
        if Settings::get().erlang.compile == Some(false) {
            bail!("precompiled erlang is not available: {reason}");
        }
        debug!("{reason}");
        Ok(None)
    }

    fn release_tag(version: &str) -> String {
        format!("OTP-{version}")
    }

    fn lockfile_url(&self, tv: &ToolVersion) -> Option<String> {
        tv.lock_platforms
            .get(&self.get_platform_key())
            .and_then(|p| p.url.clone())
    }

    fn set_lockfile_info(&self, tv: &mut ToolVersion, url: &str, checksum: Option<String>) {
        let platform_info = tv
            .lock_platforms
            .entry(self.get_platform_key())
            .or_default();
        platform_info.url = Some(url.to_string());
        if let Some(checksum) = checksum {
            platform_info.checksum.get_or_insert(checksum);
        }
    }

    fn linux_precompiled_url(version: &str, target: &PlatformTarget) -> Result<String> {
        if target.libc() == Some("musl") {
            bail!("precompiled erlang is not supported on musl linux");
        }
        let arch = match target.arch_name() {
            "x64" => "amd64",
            "arm64" => "arm64",
            other => bail!("unsupported architecture: {other}"),
        };
        let os_ver = Self::linux_precompiled_os_version()?;
        Ok(format!(
            "https://builds.hex.pm/builds/otp/{arch}/{os_ver}/{}.tar.gz",
            Self::release_tag(version)
        ))
    }

    #[cfg(linux)]
    fn linux_precompiled_cache_name(url: &str) -> String {
        url.strip_prefix("https://builds.hex.pm/builds/otp/")
            .unwrap_or(url)
            .replace('/', "__")
            .replace(':', "_")
    }

    fn lockfile_precompiled_os_option(target: &PlatformTarget) -> Option<String> {
        if target.os_name() == "linux" && target.libc() != Some("musl") {
            Self::linux_precompiled_os_version().ok()
        } else {
            None
        }
    }

    fn linux_precompiled_os_version() -> Result<String> {
        let os_ver = if Platform::current().is_linux() {
            if let Ok(os) = std::env::var("ImageOS") {
                match os.as_str() {
                    "ubuntu24" => "ubuntu-24.04".to_string(),
                    "ubuntu22" => "ubuntu-22.04".to_string(),
                    "ubuntu20" => "ubuntu-20.04".to_string(),
                    _ => os,
                }
            } else if let Some(os_release) = linux_os_release() {
                format!("{}-{}", os_release.id, os_release.version_id)
            } else {
                bail!("could not determine OS release");
            }
        } else {
            // Cross-platform Linux lock resolution cannot inspect the target
            // distro, so use Bob's newest supported Ubuntu build.
            "ubuntu-24.04".to_string()
        };

        // Currently, Bob only builds for Ubuntu, so we have to check that we're on Ubuntu,
        // and on a supported version.
        if !["ubuntu-20.04", "ubuntu-22.04", "ubuntu-24.04"].contains(&os_ver.as_str()) {
            bail!("unsupported OS version: {os_ver}");
        }
        Ok(os_ver)
    }

    fn macos_asset_name(target: &PlatformTarget) -> Result<String> {
        let arch = match target.arch_name() {
            "x64" => "x86_64",
            "arm64" => "aarch64",
            other => bail!("unsupported architecture: {other}"),
        };
        Ok(format!("otp-{arch}-apple-darwin.tar.gz"))
    }

    fn windows_asset_name(version: &str, target: &PlatformTarget) -> Result<String> {
        let os = match target.arch_name() {
            "x64" => "win64",
            "x86" => "win32",
            other => bail!("unsupported architecture: {other}"),
        };
        Ok(format!("otp_{os}_{version}.zip"))
    }

    async fn github_asset_lock_info(repo: &str, tag: &str, name: &str) -> Result<PlatformInfo> {
        let release = github::get_release(repo, tag).await?;
        let asset = release
            .assets
            .iter()
            .find(|a| a.name == name)
            .ok_or_else(|| eyre::eyre!("no asset found for {name} in {tag}"))?;
        Ok(PlatformInfo {
            checksum: asset.digest.clone(),
            url: Some(asset.browser_download_url.clone()),
            url_api: Some(asset.url.clone()),
            ..Default::default()
        })
    }

    #[cfg(linux)]
    async fn install_precompiled(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> Result<Option<ToolVersion>> {
        if Settings::get().erlang.compile == Some(true) {
            return Ok(None);
        }
        let url = if let Some(url) = self.lockfile_url(&tv) {
            url
        } else {
            match Self::linux_precompiled_url(&tv.version, &PlatformTarget::from_current()) {
                Ok(url) => url,
                Err(e) => {
                    return self.precompiled_unavailable(e.to_string());
                }
            }
        };

        let filename = url.split('/').next_back().unwrap();
        let tarball_path = tv
            .download_path()
            .join(Self::linux_precompiled_cache_name(&url));

        ctx.pr.set_message(format!("Downloading {filename}"));
        if !tarball_path.exists() {
            HTTP.download_file(&url, &tarball_path, Some(ctx.pr.as_ref()))
                .await?;
        }
        self.set_lockfile_info(&mut tv, &url, None);
        ctx.pr.set_message(format!("Extracting {filename}"));
        file::untar(
            &tarball_path,
            &tv.download_path(),
            file::ExtractionFormat::TarGz,
            &ExtractOptions {
                pr: Some(ctx.pr.as_ref()),
                ..Default::default()
            },
        )?;

        self.move_to_install_path(&tv)?;

        CmdLineRunner::new(tv.install_path().join("Install"))
            .with_pr(ctx.pr.as_ref())
            .arg("-minimal")
            .arg(tv.install_path())
            .envs(tv.install_env())
            .execute()?;

        Ok(Some(tv))
    }

    #[cfg(linux)]
    fn move_to_install_path(&self, tv: &ToolVersion) -> Result<()> {
        let base_dir = tv
            .download_path()
            .read_dir()?
            .find(|e| e.as_ref().unwrap().file_type().unwrap().is_dir())
            .unwrap()?
            .path();
        file::remove_all(tv.install_path())?;
        file::create_dir_all(tv.install_path())?;
        for entry in fs::read_dir(base_dir)? {
            let entry = entry?;
            let dest = tv.install_path().join(entry.file_name());
            trace!("moving {:?} to {:?}", entry.path(), &dest);
            file::move_file(entry.path(), dest)?;
        }

        Ok(())
    }

    #[cfg(macos)]
    async fn install_precompiled(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> Result<Option<ToolVersion>> {
        if Settings::get().erlang.compile == Some(true) {
            return Ok(None);
        }
        let release_tag = Self::release_tag(&tv.version);
        let (url, checksum) = if let Some(url) = self.lockfile_url(&tv) {
            (url, None)
        } else {
            let tarball_name = match Self::macos_asset_name(&PlatformTarget::from_current()) {
                Ok(tarball_name) => tarball_name,
                Err(e) => return self.precompiled_unavailable(e.to_string()),
            };
            let gh_release = match github::get_release("erlef/otp_builds", &release_tag).await {
                Ok(release) => release,
                Err(e) => {
                    return self.precompiled_unavailable(format!(
                        "failed to get release {release_tag}: {e}"
                    ));
                }
            };
            let asset = match gh_release.assets.iter().find(|a| a.name == tarball_name) {
                Some(asset) => asset,
                None => {
                    return self.precompiled_unavailable(format!(
                        "no asset found for {tarball_name} in {release_tag}"
                    ));
                }
            };
            (asset.browser_download_url.clone(), asset.digest.clone())
        };
        let tarball_name = url.split('/').next_back().unwrap();
        ctx.pr.set_message(format!("Downloading {tarball_name}"));
        let tarball_path = tv.download_path().join(tarball_name);
        if !tarball_path.exists() {
            HTTP.download_file(&url, &tarball_path, Some(ctx.pr.as_ref()))
                .await?;
        }
        self.set_lockfile_info(&mut tv, &url, checksum);
        self.verify_checksum(ctx, &mut tv, &tarball_path)?;
        ctx.pr.set_message(format!("Extracting {tarball_name}"));
        file::untar(
            &tarball_path,
            &tv.install_path(),
            file::ExtractionFormat::TarGz,
            &ExtractOptions {
                pr: Some(ctx.pr.as_ref()),
                ..Default::default()
            },
        )?;
        Ok(Some(tv))
    }

    #[cfg(windows)]
    async fn install_precompiled(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> Result<Option<ToolVersion>> {
        if Settings::get().erlang.compile == Some(true) {
            return Ok(None);
        }
        let release_tag = Self::release_tag(&tv.version);
        let (url, checksum) = if let Some(url) = self.lockfile_url(&tv) {
            (url, None)
        } else {
            let zip_name =
                match Self::windows_asset_name(&tv.version, &PlatformTarget::from_current()) {
                    Ok(zip_name) => zip_name,
                    Err(e) => return self.precompiled_unavailable(e.to_string()),
                };
            let gh_release = match github::get_release("erlang/otp", &release_tag).await {
                Ok(release) => release,
                Err(e) => {
                    return self.precompiled_unavailable(format!(
                        "failed to get release {release_tag}: {e}"
                    ));
                }
            };
            let asset = match gh_release.assets.iter().find(|a| a.name == zip_name) {
                Some(asset) => asset,
                None => {
                    return self.precompiled_unavailable(format!(
                        "no asset found for {zip_name} in {release_tag}"
                    ));
                }
            };
            (asset.browser_download_url.clone(), asset.digest.clone())
        };
        let zip_name = url.split('/').next_back().unwrap();
        ctx.pr.set_message(format!("Downloading {}", zip_name));
        let zip_path = tv.download_path().join(zip_name);
        if !zip_path.exists() {
            HTTP.download_file(&url, &zip_path, Some(ctx.pr.as_ref()))
                .await?;
        }
        self.set_lockfile_info(&mut tv, &url, checksum);
        self.verify_checksum(ctx, &mut tv, &zip_path)?;
        ctx.pr.set_message(format!("Extracting {}", zip_name));
        file::unzip(&zip_path, &tv.install_path(), &Default::default())?;
        Ok(Some(tv))
    }

    #[cfg(not(any(linux, macos, windows)))]
    async fn install_precompiled(
        &self,
        _ctx: &InstallContext,
        _tv: ToolVersion,
    ) -> Result<Option<ToolVersion>> {
        if Settings::get().erlang.compile == Some(true) {
            Ok(None)
        } else {
            self.precompiled_unavailable("precompiled erlang is not supported on this platform")
        }
    }

    async fn install_via_kerl(
        &self,
        _ctx: &InstallContext,
        tv: ToolVersion,
    ) -> Result<ToolVersion> {
        self.update_kerl().await?;

        file::remove_all(tv.install_path())?;
        match &tv.request {
            ToolRequest::Ref { .. } => {
                unimplemented!("erlang does not yet support refs");
            }
            _ => {
                let mut cmd = cmd!(
                    self.kerl_path(),
                    "build-install",
                    &tv.version,
                    &tv.version,
                    tv.install_path()
                )
                .env("MAKEFLAGS", format!("-j{}", num_cpus::get()));
                for (key, value) in tv.install_env() {
                    cmd = cmd.env(key, value);
                }
                cmd.env("KERL_BASE_DIR", self.ba.cache_path.join("kerl"))
                    .run()?;
            }
        }

        Ok(tv)
    }
}

#[async_trait]
impl Backend for ErlangPlugin {
    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<VersionInfo>> {
        let versions = if Settings::get().erlang.compile == Some(false) {
            github::list_releases("erlef/otp_builds")
                .await?
                .into_iter()
                .filter_map(|r| {
                    r.tag_name
                        .strip_prefix("OTP-")
                        .map(|s| (s.to_string(), Some(r.created_at)))
                })
                .map(|(version, created_at)| VersionInfo {
                    version,
                    created_at,
                    ..Default::default()
                })
                .collect()
        } else {
            self.update_kerl().await?;
            let kerl_path = self.kerl_path().to_string_lossy().to_string();
            let kerl_base_dir = self.ba.cache_path.join("kerl");
            plugins::core::run_fetch_task_with_timeout_async(async move || {
                let output = crate::cmd::cmd_read_async_inherited_env(
                    &kerl_path,
                    &["list", "releases", "all"],
                    [("KERL_BASE_DIR", kerl_base_dir.as_os_str())],
                )
                .await?;
                let versions = output
                    .split('\n')
                    .filter(|s| regex!(r"^[0-9].+$").is_match(s))
                    .map(|s| VersionInfo {
                        version: s.to_string(),
                        ..Default::default()
                    })
                    .collect();
                Ok(versions)
            })
            .await?
        };
        Ok(versions)
    }

    async fn install_version_(&self, ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion> {
        if let Some(tv) = self.install_precompiled(ctx, tv.clone()).await? {
            return Ok(tv);
        }
        self.install_via_kerl(ctx, tv).await
    }

    fn supports_lockfile_url(&self) -> bool {
        // In default mode, precompiled Erlang is opportunistic and may fall
        // back to kerl, so locked installs cannot always require a URL.
        Settings::get().erlang.compile == Some(false)
    }

    fn resolve_lockfile_options(
        &self,
        _request: &ToolRequest,
        target: &PlatformTarget,
    ) -> Result<BTreeMap<String, String>> {
        let mut opts = BTreeMap::new();
        let settings = Settings::get();

        match settings.erlang.compile {
            Some(true) => {
                opts.insert("compile".to_string(), "true".to_string());
            }
            Some(false) => {
                opts.insert("compile".to_string(), "false".to_string());
                if let Some(os_version) = Self::lockfile_precompiled_os_option(target) {
                    opts.insert(ERLANG_PRECOMPILED_OS_OPTION.to_string(), os_version);
                }
            }
            None => {
                opts.insert(
                    ERLANG_PRECOMPILED_OPTION.to_string(),
                    "if_available".to_string(),
                );
                if let Some(os_version) = Self::lockfile_precompiled_os_option(target) {
                    opts.insert(ERLANG_PRECOMPILED_OS_OPTION.to_string(), os_version);
                }
            }
        }

        Ok(opts)
    }

    async fn resolve_lock_info(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        let compile = Settings::get().erlang.compile;
        if compile == Some(true) {
            return Ok(PlatformInfo::default());
        }

        let release_tag = Self::release_tag(&tv.version);
        match target.os_name() {
            "linux" => match Self::linux_precompiled_url(&tv.version, target) {
                Ok(url) => Ok(PlatformInfo {
                    url: Some(url),
                    ..Default::default()
                }),
                Err(err) if compile == Some(false) => Err(err),
                Err(_) => Ok(PlatformInfo::default()),
            },
            "macos" => {
                let info = match Self::macos_asset_name(target) {
                    Ok(asset_name) => {
                        Self::github_asset_lock_info("erlef/otp_builds", &release_tag, &asset_name)
                            .await
                    }
                    Err(err) => Err(err),
                };
                match info {
                    Ok(info) => Ok(info),
                    Err(err) if compile == Some(false) => Err(err),
                    Err(_) => Ok(PlatformInfo::default()),
                }
            }
            "windows" => {
                let info = match Self::windows_asset_name(&tv.version, target) {
                    Ok(asset_name) => {
                        Self::github_asset_lock_info("erlang/otp", &release_tag, &asset_name).await
                    }
                    Err(err) => Err(err),
                };
                match info {
                    Ok(info) => Ok(info),
                    Err(err) if compile == Some(false) => Err(err),
                    Err(_) => Ok(PlatformInfo::default()),
                }
            }
            _ => Ok(PlatformInfo::default()),
        }
    }
}
