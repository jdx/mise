use std::path::PathBuf;

use crate::backend::Backend;
use crate::cli::args::BackendArg;
use crate::config::SETTINGS;
use crate::file::{display_path, TarOptions};
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::lock_file::LockFile;
use crate::toolset::{ToolRequest, ToolVersion};
use crate::{cmd, file, github, plugins};
use eyre::Result;
use xx::regex;

#[derive(Debug)]
pub struct ErlangPlugin {
    ba: BackendArg,
}

const KERL_VERSION: &str = "4.1.1";

impl ErlangPlugin {
    pub fn new() -> Self {
        Self {
            ba: plugins::core::new_backend_arg("erlang"),
        }
    }

    fn kerl_path(&self) -> PathBuf {
        self.ba.cache_path.join(format!("kerl-{}", KERL_VERSION))
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

    fn update_kerl(&self) -> Result<()> {
        let _lock = self.lock_build_tool();
        if self.kerl_path().exists() {
            // TODO: find a way to not have to do this #1209
            file::remove_all(self.kerl_base_dir())?;
            return Ok(());
        }
        self.install_kerl()?;
        cmd!(self.kerl_path(), "update", "releases")
            .env("KERL_BASE_DIR", self.kerl_base_dir())
            .stdout_to_stderr()
            .run()?;
        Ok(())
    }

    fn install_kerl(&self) -> Result<()> {
        debug!("Installing kerl to {}", display_path(self.kerl_path()));
        HTTP_FETCH.download_file(
            format!("https://raw.githubusercontent.com/kerl/kerl/{KERL_VERSION}/kerl"),
            &self.kerl_path(),
            None,
        )?;
        file::make_executable(self.kerl_path())?;
        Ok(())
    }

    fn install_precompiled(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> Result<Option<ToolVersion>> {
        if SETTINGS.erlang.compile == Some(false) {
            return Ok(None);
        }
        let release_tag = format!("OTP-{}", tv.version);
        let gh_release = match github::get_release("erlef/otp_builds", &release_tag) {
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
        ctx.pr.set_message(format!("Downloading {}", tarball_name));
        let tarball_path = tv.download_path().join(&tarball_name);
        HTTP.download_file(
            &asset.browser_download_url,
            &tarball_path,
            Some(ctx.pr.as_ref()),
        )?;
        self.verify_checksum(ctx, &mut tv, &tarball_path)?;
        ctx.pr.set_message(format!("Extracting {}", tarball_name));
        file::untar(
            &tarball_path,
            &tv.install_path(),
            &TarOptions {
                strip_components: 0,
                pr: Some(ctx.pr.as_ref()),
                format: file::TarFormat::TarGz,
            },
        )?;
        Ok(Some(tv))
    }

    fn install_via_kerl(&self, _ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion> {
        self.update_kerl()?;

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
                .stdout_to_stderr()
                .run()?;
            }
        }

        Ok(tv)
    }
}

impl Backend for ErlangPlugin {
    fn ba(&self) -> &BackendArg {
        &self.ba
    }
    fn _list_remote_versions(&self) -> Result<Vec<String>> {
        let versions = if SETTINGS.erlang.compile == Some(false) {
            github::list_releases("erlef/otp_builds")?
                .into_iter()
                .filter_map(|r| r.tag_name.strip_prefix("OTP-").map(|s| s.to_string()))
                .collect()
        } else {
            self.update_kerl()?;
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

    fn install_version_(&self, ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion> {
        if let Some(tv) = self.install_precompiled(ctx, tv.clone())? {
            return Ok(tv);
        }
        self.install_via_kerl(ctx, tv)
    }
}

#[cfg(target_arch = "x86_64")]
pub const ARCH: &str = "x86_64";

#[cfg(target_arch = "aarch64")]
const ARCH: &str = "aarch64";

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
const ARCH: &str = "unknown";

#[cfg(macos)]
const OS: &str = "apple-darwin";

#[cfg(target_os = "freebsd")]
const OS: &str = "unknown";

#[cfg(not(macos))]
const OS: &str = "unknown";
