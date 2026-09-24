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
use crate::packslip_requirements::glibc_version;
use crate::platform::Platform;
use crate::plugins;
use crate::toolset::ToolVersion;
use crate::ui::progress_report::SingleReport;

const REPO: &str = "holepunchto/bare-runtime";

#[derive(Debug)]
pub(super) struct BarePlugin {
    ba: Arc<BackendArg>,
}

impl BarePlugin {
    /// Creates a new BarePlugin instance.
    pub(super) fn new() -> Self {
        Self {
            ba: Arc::new(plugins::core::new_backend_arg("bare")),
        }
    }

    /// Returns the path to the bare binary for the given tool version.
    fn bin_path(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin").join(bin_name())
    }

    /// Finds the GitHub release asset matching the current platform for the given version.
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

    /// Validates that the current system's glibc version meets Bare's minimum requirement (2.35+).
    /// Returns an error if glibc is too old or detection fails on Linux.
    async fn validate_glibc_version(target: &PlatformTarget) -> Result<()> {
        if target.os_name() != "linux" {
            return Ok(());
        }
        if target.libc() == Some("musl") {
            return Ok(());
        }
        let version_str = glibc_version().await.ok_or_else(|| {
            eyre!("Failed to detect glibc version. Bare requires glibc 2.35 or newer on Linux.")
        })?;
        let version = version_str.as_str();
        let parts: Vec<u32> = version.split('.').map(|s| s.parse().unwrap_or(0)).collect();
        let major = parts.first().copied().unwrap_or(0);
        let minor = parts.get(1).copied().unwrap_or(0);
        if major < 2 || (major == 2 && minor < 35) {
            return Err(eyre!(
                "Bare requires glibc 2.35 or newer, found {version}. \
                See https://github.com/holepunchto/bare#platform-support"
            ));
        }
        Ok(())
    }

    /// Downloads the Bare runtime artifact for the current platform.
    /// If locked, uses the URL and checksum from the lockfile.
    /// Otherwise, fetches the latest release asset from GitHub and requires a SHA-256 digest.
    async fn download(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
        pr: &dyn SingleReport,
    ) -> Result<PathBuf> {
        let target = PlatformTarget::from_current();
        Self::validate_glibc_version(&target).await?;
        let platform_key = self.get_platform_key();
        let (name, url, checksum) = if ctx.locked {
            let platform_info = tv
                .lock_platforms
                .get(&platform_key)
                .ok_or_else(|| eyre!("no locked Bare artifact for {}", target.to_key()))?;
            let url = platform_info
                .url
                .clone()
                .ok_or_else(|| eyre!("no locked Bare artifact for {}", target.to_key()))?;
            platform_info
                .checksum
                .as_ref()
                .ok_or_else(|| eyre!("no locked Bare checksum for {}", target.to_key()))?;
            (asset_filename(&tv.version, &target)?, url, None)
        } else {
            let asset = self.release_asset(tv, &target).await?;
            let digest = asset.digest.ok_or_else(|| {
                eyre!(
                    "Bare release asset {} has no SHA-256 digest; refusing to install without integrity check",
                    asset.name
                )
            })?;
            (asset.name, asset.browser_download_url, Some(digest))
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

    /// Extracts and installs the Bare binary from the downloaded tarball.
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

    /// Verifies the installed Bare binary works by running `bare --version`.
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
    /// Returns the backend argument configuration for this plugin.
    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    /// Returns the security features supported by this backend.
    /// Bare provides SHA-256 checksum verification for downloaded artifacts.
    async fn security_info(&self) -> Vec<SecurityFeature> {
        vec![SecurityFeature::Checksum {
            algorithm: Some("sha256".to_string()),
        }]
    }

    /// Lists all available remote versions of Bare from GitHub releases.
    /// Filters to only include versions that have a matching asset for the current platform.
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

    /// Installs a specific version of Bare.
    /// Downloads the platform-specific artifact, verifies its checksum, extracts it, and verifies the binary works.
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

    /// Resolves lockfile information for a specific version and platform.
    /// Fetches the GitHub release asset and returns its checksum and download URL.
    async fn resolve_lock_info(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        let asset = self.release_asset(tv, target).await?;
        let checksum = asset.digest.ok_or_else(|| {
            eyre!(
                "Bare release asset {} has no SHA-256 digest; refusing to install without integrity check",
                asset.name
            )
        })?;
        Ok(PlatformInfo {
            checksum: Some(checksum),
            url: Some(asset.browser_download_url),
            ..Default::default()
        })
    }

    /// Returns platform variants for lockfile generation.
    /// Bare provides both macOS architectures (x64 and ARM64) to ensure
    /// lockfiles are complete for cross-platform teams.
    fn platform_variants(&self, platform: &Platform) -> Vec<Platform> {
        if platform.qualifier.is_some() {
            return vec![platform.clone()];
        }

        let mut variants = vec![platform.clone()];

        match (platform.os.as_str(), platform.arch.as_str()) {
            ("macos", "arm64") => {
                // Add macOS x64 variant for cross-platform lockfile completeness
                variants.push(Platform {
                    os: "macos".to_string(),
                    arch: "x64".to_string(),
                    qualifier: None,
                });
            }
            ("macos", "x64") => {
                // Add macOS ARM64 variant for cross-platform lockfile completeness
                variants.push(Platform {
                    os: "macos".to_string(),
                    arch: "arm64".to_string(),
                    qualifier: None,
                });
            }
            _ => {}
        }

        variants
    }
}

/// Returns the release asset filename for the given version and platform target.
/// Validates that the platform is supported (Linux glibc, macOS, Windows on x64/arm64).
/// Returns an error for unsupported platforms or architectures.
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

/// Returns the bare binary name for the current platform.
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
