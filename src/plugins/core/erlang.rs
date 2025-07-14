use std::{path::PathBuf, sync::Arc};

use crate::backend::Backend;
use crate::cli::args::BackendArg;
use crate::config::{Config, Settings};
#[cfg(unix)]
use crate::file::TarOptions;
use crate::file::display_path;
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::lock_file::LockFile;
use crate::toolset::{ToolRequest, ToolVersion};
use crate::{cmd, file, github, plugins};
use async_trait::async_trait;
use eyre::Result;
use xx::regex;

#[cfg(linux)]
use crate::cmd::CmdLineRunner;
#[cfg(linux)]
use std::fs;

#[derive(Debug)]
pub struct ErlangPlugin {
    ba: Arc<BackendArg>,
}

const KERL_VERSION: &str = "4.1.1";

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
        cmd!(self.kerl_path(), "update", "releases")
            .env("KERL_BASE_DIR", self.kerl_base_dir())
            .run()?;
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

    #[cfg(linux)]
    async fn install_precompiled(
        &self,
        ctx: &InstallContext,
        tv: ToolVersion,
    ) -> Result<Option<ToolVersion>> {
        if Settings::get().erlang.compile == Some(true) {
            return Ok(None);
        }
        let release_tag = format!("OTP-{}", tv.version);

        let arch: String = match ARCH {
            "x86_64" => "amd64".to_string(),
            "aarch64" => "arm64".to_string(),
            _ => {
                debug!("Unsupported architecture: {}", ARCH);
                return Ok(None);
            }
        };

        let os_ver: String;
        if let Ok(os) = std::env::var("ImageOS") {
            os_ver = match os.as_str() {
                "ubuntu24" => "ubuntu-24.04".to_string(),
                "ubuntu22" => "ubuntu-22.04".to_string(),
                "ubuntu20" => "ubuntu-20.04".to_string(),
                _ => os,
            };
        } else if let Ok(os_release) = &*os_release::OS_RELEASE {
            os_ver = format!("{}-{}", os_release.id, os_release.version_id);
        } else {
            return Ok(None);
        };

        // Currently, Bob only builds for Ubuntu, so we have to check that we're on ubuntu, and on a supported version
        if !["ubuntu-20.04", "ubuntu-22.04", "ubuntu-24.04"].contains(&os_ver.as_str()) {
            debug!("Unsupported OS version: {}", os_ver);
            return Ok(None);
        }

        let url: String =
            format!("https://builds.hex.pm/builds/otp/{arch}/{os_ver}/{release_tag}.tar.gz");

        let filename = url.split('/').next_back().unwrap();
        let tarball_path = tv.download_path().join(filename);

        ctx.pr.set_message(format!("Downloading {filename}"));
        if !tarball_path.exists() {
            HTTP.download_file(&url, &tarball_path, Some(&ctx.pr))
                .await?;
        }
        ctx.pr.set_message(format!("Extracting {filename}"));
        file::untar(
            &tarball_path,
            &tv.download_path(),
            &TarOptions {
                strip_components: 0,
                pr: Some(&ctx.pr),
                format: file::TarFormat::TarGz,
            },
        )?;

        self.move_to_install_path(&tv)?;

        CmdLineRunner::new(tv.install_path().join("Install"))
            .with_pr(&ctx.pr)
            .arg("-minimal")
            .arg(tv.install_path())
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
            file::rename(entry.path(), dest)?;
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
        let release_tag = format!("OTP-{}", tv.version);
        let gh_release = match github::get_release("erlef/otp_builds", &release_tag).await {
            Ok(release) => release,
            Err(e) => {
                debug!("Failed to get release: {}", e);
                return Ok(None);
            }
        };
        let tarball_name = format!("otp-{ARCH}-{OS}.tar.gz");
        let asset = match gh_release.assets.iter().find(|a| a.name == tarball_name) {
            Some(asset) => asset,
            None => {
                debug!("No asset found for {}", release_tag);
                return Ok(None);
            }
        };
        ctx.pr.set_message(format!("Downloading {tarball_name}"));
        let tarball_path = tv.download_path().join(&tarball_name);
        HTTP.download_file(&asset.browser_download_url, &tarball_path, Some(&ctx.pr))
            .await?;
        self.verify_checksum(ctx, &mut tv, &tarball_path)?;
        ctx.pr.set_message(format!("Extracting {tarball_name}"));
        file::untar(
            &tarball_path,
            &tv.install_path(),
            &TarOptions {
                strip_components: 0,
                pr: Some(&ctx.pr),
                format: file::TarFormat::TarGz,
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
        let release_tag = format!("OTP-{}", tv.version);
        let gh_release = match github::get_release("erlang/otp", &release_tag).await {
            Ok(release) => release,
            Err(e) => {
                debug!("Failed to get release: {}", e);
                return Ok(None);
            }
        };
        let zip_name = format!("otp_{OS}_{version}.zip", version = tv.version);
        let asset = match gh_release.assets.iter().find(|a| a.name == zip_name) {
            Some(asset) => asset,
            None => {
                debug!("No asset found for {}", release_tag);
                return Ok(None);
            }
        };
        ctx.pr.set_message(format!("Downloading {}", zip_name));
        let zip_path = tv.download_path().join(&zip_name);
        HTTP.download_file(&asset.browser_download_url, &zip_path, Some(&ctx.pr))
            .await?;
        self.verify_checksum(ctx, &mut tv, &zip_path)?;
        ctx.pr.set_message(format!("Extracting {}", zip_name));
        file::unzip(&zip_path, &tv.install_path(), &Default::default())?;
        Ok(Some(tv))
    }

    #[cfg(not(any(linux, macos, windows)))]
    async fn install_precompiled(
        &self,
        ctx: &InstallContext,
        tv: ToolVersion,
    ) -> Result<Option<ToolVersion>> {
        Ok(None)
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
                cmd!(
                    self.kerl_path(),
                    "build-install",
                    &tv.version,
                    &tv.version,
                    tv.install_path()
                )
                .env("KERL_BASE_DIR", self.ba.cache_path.join("kerl"))
                .env("MAKEFLAGS", format!("-j{}", num_cpus::get()))
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

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<String>> {
        let versions = if Settings::get().erlang.compile == Some(false) {
            github::list_releases("erlef/otp_builds")
                .await?
                .into_iter()
                .filter_map(|r| r.tag_name.strip_prefix("OTP-").map(|s| s.to_string()))
                .collect()
        } else {
            self.update_kerl().await?;
            plugins::core::run_fetch_task_with_timeout(move || {
                let output = cmd!(self.kerl_path(), "list", "releases", "all")
                    .env("KERL_BASE_DIR", self.ba.cache_path.join("kerl"))
                    .read()?;
                let versions = output
                    .split('\n')
                    .filter(|s| regex!(r"^[0-9].+$").is_match(s))
                    .map(|s| s.to_string())
                    .collect();
                Ok(versions)
            })?
        };
        Ok(versions)
    }

    async fn install_version_(&self, ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion> {
        if let Some(tv) = self.install_precompiled(ctx, tv.clone()).await? {
            return Ok(tv);
        }
        self.install_via_kerl(ctx, tv).await
    }
}

#[cfg(all(target_arch = "x86_64", not(target_os = "windows")))]
pub const ARCH: &str = "x86_64";

#[cfg(all(target_arch = "aarch64", not(target_os = "windows")))]
const ARCH: &str = "aarch64";

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
const ARCH: &str = "unknown";

#[cfg(windows)]
const OS: &str = "win64";

#[cfg(macos)]
const OS: &str = "apple-darwin";
