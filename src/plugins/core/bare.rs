use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use async_trait::async_trait;
use eyre::{Result, eyre};
use tempfile::tempdir_in;

use crate::backend::platform_target::PlatformTarget;
use crate::backend::{Backend, SecurityFeature, VersionInfo};
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::Config;
use crate::file::{self, ExtractOptions, ExtractionFormat};
use crate::github::{self, GithubAsset};
use crate::http::HTTP;
use crate::install_context::InstallContext;
use crate::lockfile::PlatformInfo;
use crate::plugins;
use crate::toolset::ToolVersion;
use crate::ui::progress_report::SingleReport;

const REPO: &str = "holepunchto/bare-runtime";

#[derive(Debug)]
pub(super) struct BarePlugin {
    ba: Arc<BackendArg>,
}

impl BarePlugin {
    pub(super) fn new() -> Self {
        Self {
            ba: Arc::new(plugins::core::new_backend_arg("bare")),
        }
    }

    fn bin_path(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin").join(bin_name())
    }

    async fn release_asset(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<GithubAsset> {
        let filename = asset_filename(&tv.version, target)?;
        github::get_release(REPO, &format!("v{}", tv.version))
            .await?
            .assets
            .into_iter()
            .find(|asset| asset.name == filename)
            .ok_or_else(|| {
                eyre!(
                    "Bare {version} has no asset for {target}: {filename}",
                    version = tv.version,
                    target = target.to_key()
                )
            })
    }

    async fn download(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
        pr: &dyn SingleReport,
    ) -> Result<PathBuf> {
        let target = PlatformTarget::from_current();
        let platform_key = self.get_platform_key();
        let (name, url, checksum) = if ctx.locked {
            let url = tv
                .lock_platforms
                .get(&platform_key)
                .and_then(|info| info.url.clone())
                .ok_or_else(|| eyre!("no locked Bare artifact for {}", target.to_key()))?;
            (asset_filename(&tv.version, &target)?, url, None)
        } else {
            let asset = self.release_asset(tv, &target).await?;
            (asset.name, asset.browser_download_url, asset.digest)
        };
        let tarball_path = tv.download_path().join(&name);
        pr.set_message(format!("download {name}"));
        HTTP.download_file(&url, &tarball_path, Some(pr)).await?;

        if !ctx.locked {
            let platform_info = tv.lock_platforms.entry(platform_key).or_default();
            platform_info.url = Some(url);
            platform_info.checksum = checksum;
        }

        Ok(tarball_path)
    }

    fn install(&self, tv: &ToolVersion, pr: &dyn SingleReport, tarball_path: &Path) -> Result<()> {
        let filename = tarball_path.file_name().unwrap().to_string_lossy();
        pr.set_message(format!("extract {filename}"));
        let extract_path = tempdir_in(tv.install_path().parent().unwrap())?;
        file::untar(
            tarball_path,
            extract_path.path(),
            ExtractionFormat::TarGz,
            &ExtractOptions {
                pr: Some(pr),
                ..Default::default()
            },
        )?;
        file::remove_all(tv.install_path())?;
        file::rename(extract_path.path().join("package"), tv.install_path())?;
        file::make_executable(self.bin_path(tv))?;
        Ok(())
    }

    fn verify(&self, tv: &ToolVersion, pr: &dyn SingleReport) -> Result<()> {
        pr.set_message("bare --version".into());
        CmdLineRunner::new(self.bin_path(tv))
            .with_pr(pr)
            .arg("--version")
            .env_values(tv.install_env())
            .execute()
    }
}

#[async_trait]
impl Backend for BarePlugin {
    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    async fn security_info(&self) -> Vec<SecurityFeature> {
        vec![SecurityFeature::Checksum {
            algorithm: Some("sha256".to_string()),
        }]
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<VersionInfo>> {
        let target = PlatformTarget::from_current();
        let mut versions = Vec::new();
        for release in github::list_releases(REPO).await?.into_iter().rev() {
            let Some(version) = release.tag_name.strip_prefix('v') else {
                continue;
            };
            let filename = asset_filename(version, &target)?;
            if release.assets.iter().any(|asset| asset.name == filename) {
                versions.push(VersionInfo {
                    version: version.to_string(),
                    created_at: Some(release.released_at().to_string()),
                    ..Default::default()
                });
            }
        }
        Ok(versions)
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> Result<ToolVersion> {
        let tarball_path = self.download(ctx, &mut tv, ctx.pr.as_ref()).await?;
        ctx.pr.next_operation();
        self.verify_checksum(ctx, &mut tv, &tarball_path)?;
        ctx.pr.next_operation();
        self.install(&tv, ctx.pr.as_ref(), &tarball_path)?;
        self.verify(&tv, ctx.pr.as_ref())?;
        Ok(tv)
    }

    async fn resolve_lock_info(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        let asset = self.release_asset(tv, target).await?;
        Ok(PlatformInfo {
            checksum: asset.digest,
            url: Some(asset.browser_download_url),
            ..Default::default()
        })
    }
}

fn asset_filename(version: &str, target: &PlatformTarget) -> Result<String> {
    if target.os_name() == "linux" && target.libc() == Some("musl") {
        return Err(eyre!("Bare does not publish musl Linux binaries"));
    }
    let os = match target.os_name() {
        "macos" => "darwin",
        "linux" => "linux",
        "windows" => "win32",
        os => return Err(eyre!("Bare does not support {os}")),
    };
    let arch = match target.arch_name() {
        "x64" => "x64",
        "arm64" => "arm64",
        arch => return Err(eyre!("Bare does not support {arch} on {os}")),
    };
    Ok(format!("bare-runtime-{os}-{arch}-{version}.tgz"))
}

fn bin_name() -> &'static str {
    if cfg!(windows) { "bare.exe" } else { "bare" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::Platform;

    fn target(platform: &str) -> PlatformTarget {
        PlatformTarget::new(Platform::parse(platform).unwrap())
    }

    #[test]
    fn maps_release_assets() {
        for (platform, expected) in [
            ("macos-x64", "bare-runtime-darwin-x64-1.33.4.tgz"),
            ("macos-arm64", "bare-runtime-darwin-arm64-1.33.4.tgz"),
            ("linux-x64", "bare-runtime-linux-x64-1.33.4.tgz"),
            ("linux-arm64", "bare-runtime-linux-arm64-1.33.4.tgz"),
            ("windows-x64", "bare-runtime-win32-x64-1.33.4.tgz"),
            ("windows-arm64", "bare-runtime-win32-arm64-1.33.4.tgz"),
        ] {
            assert_eq!(
                asset_filename("1.33.4", &target(platform)).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn rejects_unsupported_release_assets() {
        assert!(asset_filename("1.33.4", &target("freebsd-x64")).is_err());
        assert!(asset_filename("1.33.4", &target("linux-x86")).is_err());
        assert!(asset_filename("1.33.4", &target("linux-x64-musl")).is_err());
    }
}
