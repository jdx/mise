use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use async_trait::async_trait;
use eyre::Result;
use itertools::Itertools;
use versions::Versioning;

use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::http::HTTP;
use crate::install_context::InstallContext;
use crate::lockfile::PlatformInfo;
use crate::toolset::ToolVersion;
use crate::ui::progress_report::SingleReport;
use crate::{backend::static_helpers::fetch_checksum_from_shasums, platform::detect_libc};
use crate::{
    backend::{Backend, GitHubReleaseInfo, VersionInfo, platform_target::PlatformTarget},
    config::{Config, Settings},
    platform::Platform,
};
use crate::{file, github, plugins};

/// Bun started publishing native Windows ARM64 archives (`bun-windows-aarch64.zip`)
/// in this release. Older releases only ship x64 Windows builds, so Windows ARM64
/// falls back to `x64-baseline` under emulation for versions below this cutoff.
const WINDOWS_ARM64_NATIVE_ARCHIVE_VERSION: &str = "1.3.10";

#[derive(Debug)]
pub struct BunPlugin {
    ba: Arc<BackendArg>,
}

impl BunPlugin {
    pub fn new() -> Self {
        Self {
            ba: Arc::new(plugins::core::new_backend_arg("bun")),
        }
    }

    fn bun_bin(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("bin").join(bun_bin_name())
    }

    fn test_bun(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<()> {
        ctx.pr.set_message("bun -v".into());
        CmdLineRunner::new(self.bun_bin(tv))
            .with_pr(ctx.pr.as_ref())
            .arg("-v")
            .envs(tv.install_env())
            .execute()
    }

    async fn download(&self, tv: &ToolVersion, pr: &dyn SingleReport) -> Result<PathBuf> {
        let url = format!(
            "https://github.com/oven-sh/bun/releases/download/bun-v{}/bun-{}-{}.zip",
            tv.version,
            os(),
            arch(&tv.version)
        );
        let filename = url.split('/').next_back().unwrap();
        let tarball_path = tv.download_path().join(filename);

        pr.set_message(format!("download {filename}"));
        HTTP.download_file(&url, &tarball_path, Some(pr)).await?;

        Ok(tarball_path)
    }

    fn install(&self, ctx: &InstallContext, tv: &ToolVersion, tarball_path: &Path) -> Result<()> {
        let filename = tarball_path.file_name().unwrap().to_string_lossy();
        ctx.pr.set_message(format!("extract {filename}"));
        file::remove_all(tv.install_path())?;
        file::create_dir_all(tv.install_path().join("bin"))?;
        file::unzip(tarball_path, &tv.download_path(), &Default::default())?;
        file::move_file(
            tv.download_path()
                .join(format!("bun-{}-{}", os(), arch(&tv.version)))
                .join(bun_bin_name()),
            self.bun_bin(tv),
        )?;
        if cfg!(unix) {
            file::make_executable(self.bun_bin(tv))?;
            file::make_symlink(Path::new("./bun"), &tv.install_path().join("bin/bunx"))?;
        }
        #[cfg(windows)]
        {
            self.install_bunx_windows(tv)?;
        }
        Ok(())
    }

    /// Create a `bunx` entry next to `bun.exe` on Windows.
    ///
    /// Upstream `bun-windows-*.zip` ships only `bun.exe`; the `bunx` entry
    /// that exists as a symlink in the unix releases is created post-install
    /// by the bun PowerShell installer (which invokes `bun completions`,
    /// see oven-sh/bun:src/cli/install_completions_command.zig
    /// `installBunxSymlinkWindows`). Mirror that step here so users get a
    /// working `bunx` after `mise install bun`.
    #[cfg(windows)]
    fn install_bunx_windows(&self, tv: &ToolVersion) -> Result<()> {
        let bin_dir = tv.install_path().join("bin");
        let bun_exe = bin_dir.join("bun.exe");
        let bunx_exe = bin_dir.join("bunx.exe");
        let bunx_cmd = bin_dir.join("bunx.cmd");

        // Defensive cleanup: `install()` already wipes the entire install
        // path before reaching here, but be idempotent for any future caller
        // that invokes this directly. `file::remove_all` is a no-op when the
        // target is missing and propagates real errors (e.g. permission /
        // locked-file failures) so we don't silently leave a stale shim.
        file::remove_all(&bunx_exe)?;
        file::remove_all(&bunx_cmd)?;

        // Prefer a hardlink (matches upstream): bun inspects argv[0] and
        // switches into bunx mode, the same way the unix symlink does.
        match std::fs::hard_link(&bun_exe, &bunx_exe) {
            Ok(()) => Ok(()),
            Err(e) => {
                // Hardlinks can fail across volumes or on filesystems that
                // disallow them. Fall back to the cmd shim upstream uses.
                debug!(
                    "bun: hardlink {bunx_exe} -> {bun_exe} failed ({e}); writing bunx.cmd shim",
                    bunx_exe = bunx_exe.display(),
                    bun_exe = bun_exe.display(),
                );
                // Quote the expanded path so spaces in the install dir
                // (e.g. `C:\Users\First Last\...`) don't make cmd.exe
                // split the command at the first space. Upstream's
                // literal omits the quotes — that's a latent bug there
                // too, but mise install paths under `%LOCALAPPDATA%`
                // commonly contain a user name with spaces, so we cannot
                // rely on the upstream form being safe in practice.
                file::write(&bunx_cmd, b"@\"%~dp0bun.exe\" x %*\r\n")
            }
        }
    }

    fn verify(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<()> {
        self.test_bun(ctx, tv)
    }
}

#[async_trait]
impl Backend for BunPlugin {
    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    async fn security_info(&self) -> Vec<crate::backend::SecurityFeature> {
        use crate::backend::SecurityFeature;

        vec![SecurityFeature::Checksum {
            algorithm: Some("sha256".to_string()),
        }]
    }

    /// Override get_platform_key to include bun's compile-time variant (baseline, musl, etc.)
    /// This ensures lockfile lookups use the correct platform key that matches the variant
    fn get_platform_key(&self) -> String {
        let settings = Settings::get();
        let os = settings.os();
        let arch = settings.arch();

        // Get the variant suffix based on compile-time features
        let variant = Self::get_platform_variant();

        if let Some(v) = variant {
            format!("{os}-{arch}-{v}")
        } else {
            format!("{os}-{arch}")
        }
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<VersionInfo>> {
        let versions = github::list_releases("oven-sh/bun")
            .await?
            .into_iter()
            .filter_map(|r| {
                r.tag_name
                    .strip_prefix("bun-v")
                    .map(|v| (v.to_string(), r.created_at))
            })
            .unique_by(|(v, _)| v.clone())
            .sorted_by_cached_key(|(s, _)| (Versioning::new(s), s.to_string()))
            .map(|(version, created_at)| VersionInfo {
                version,
                created_at: Some(created_at),
                ..Default::default()
            })
            .collect();
        Ok(versions)
    }

    async fn _idiomatic_filenames(&self) -> Result<Vec<String>> {
        Ok(vec![".bun-version".into(), "package.json".into()])
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> Result<ToolVersion> {
        let tarball_path = self.download(&tv, ctx.pr.as_ref()).await?;
        ctx.pr.next_operation();
        self.verify_checksum(ctx, &mut tv, &tarball_path)?;
        ctx.pr.next_operation();
        self.install(ctx, &tv, &tarball_path)?;
        self.verify(ctx, &tv)?;

        Ok(tv)
    }

    // ========== Lockfile Metadata Fetching Implementation ==========

    async fn get_github_release_info(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<Option<GitHubReleaseInfo>> {
        let version = &tv.version;

        // Build the asset pattern for Bun's GitHub releases
        // Pattern: bun-{os}-{arch}.zip (where arch may include variants like -musl, -baseline)
        let asset_pattern = Self::bun_asset_filename(target, version);

        Ok(Some(GitHubReleaseInfo {
            asset_pattern: Some(asset_pattern),
            api_url: Some(format!(
                "https://github.com/oven-sh/bun/releases/download/bun-v{version}"
            )),
        }))
    }

    async fn resolve_lock_info(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        let version = &tv.version;

        // Build platform-specific filename
        let filename = Self::bun_asset_filename(target, version);

        // Build download URL
        let url =
            format!("https://github.com/oven-sh/bun/releases/download/bun-v{version}/{filename}");

        // Fetch SHASUMS256.txt to get checksum without downloading the zip
        let shasums_url = format!(
            "https://github.com/oven-sh/bun/releases/download/bun-v{version}/SHASUMS256.txt"
        );
        let checksum = fetch_checksum_from_shasums(&shasums_url, &filename).await;

        Ok(PlatformInfo {
            url: Some(url),
            checksum,
            size: None,
            url_api: None,
            conda_deps: None,
            ..Default::default()
        })
    }

    fn platform_variants(&self, platform: &Platform) -> Vec<Platform> {
        // Bun has compile-time variants that affect the download URL and checksum:
        // - baseline: for CPUs without AVX2 support
        // - musl: for musl libc (Alpine Linux, etc.)
        // - musl-baseline: musl + no AVX2
        //
        // Available variants by platform:
        // - linux-x64: x64, x64-baseline, x64-musl, x64-musl-baseline
        // - linux-arm64: aarch64, aarch64-musl
        // - macos-x64: x64, x64-baseline
        // - macos-arm64: aarch64
        // - windows-x64: x64, x64-baseline
        // - windows-arm64: aarch64 (native, since Bun v1.3.10)

        // If the platform already has a qualifier, it's already a specific variant
        // Don't expand it to avoid duplicates
        if platform.qualifier.is_some() {
            return vec![platform.clone()];
        }

        let mut variants = vec![platform.clone()];

        match (platform.os.as_str(), platform.arch.as_str()) {
            ("linux", "x64") => {
                // Linux x64 has all variants
                variants.push(Platform {
                    os: platform.os.clone(),
                    arch: platform.arch.clone(),
                    qualifier: Some("baseline".to_string()),
                });
                variants.push(Platform {
                    os: platform.os.clone(),
                    arch: platform.arch.clone(),
                    qualifier: Some("musl".to_string()),
                });
                variants.push(Platform {
                    os: platform.os.clone(),
                    arch: platform.arch.clone(),
                    qualifier: Some("musl-baseline".to_string()),
                });
            }
            ("linux", "arm64") => {
                // Linux arm64 has musl variant
                variants.push(Platform {
                    os: platform.os.clone(),
                    arch: platform.arch.clone(),
                    qualifier: Some("musl".to_string()),
                });
            }
            ("macos", "x64") | ("windows", "x64") => {
                // macOS x64 and Windows x64 have baseline variant
                variants.push(Platform {
                    os: platform.os.clone(),
                    arch: platform.arch.clone(),
                    qualifier: Some("baseline".to_string()),
                });
            }
            // macos-arm64 has no variants (just aarch64)
            _ => {}
        }

        variants
    }
}

impl BunPlugin {
    /// Map our platform OS names to Bun's naming convention
    fn map_os_to_bun(os: &str) -> &str {
        match os {
            "macos" => "darwin",
            "linux" => "linux",
            "windows" => "windows",
            other => other,
        }
    }

    /// Map our platform arch names to Bun's naming convention
    /// Note: This handles simple cases. Complex musl/baseline variants are handled in arch()
    fn map_arch_to_bun(arch: &str) -> &str {
        match arch {
            "x64" => "x64",
            "arm64" | "aarch64" => "aarch64",
            other => other,
        }
    }

    /// Build the Bun release asset filename (`bun-{os}-{arch}.zip`) for a target platform.
    fn bun_asset_filename(target: &PlatformTarget, version: &str) -> String {
        let os_name = Self::map_os_to_bun(target.os_name());
        let arch_name = Self::get_bun_arch_for_target(target, version);
        format!("bun-{os_name}-{arch_name}.zip")
    }

    /// Get the full Bun arch string for a target platform.
    ///
    /// This handles Bun's platform qualifiers (musl, baseline, etc.) and the
    /// Windows ARM64 release cutover: native `bun-windows-aarch64.zip` archives
    /// only exist from v1.3.10 onward, so older Windows ARM64 versions fall back
    /// to `x64-baseline` under emulation.
    fn get_bun_arch_for_target(target: &PlatformTarget, version: &str) -> String {
        match (target.os_name(), target.arch_name()) {
            ("windows", "arm64") if !Self::has_native_windows_arm64_archive(version) => {
                "x64-baseline".to_string()
            }
            (_, arch) => {
                Self::bun_arch_with_qualifier(Self::map_arch_to_bun(arch), target.qualifier())
            }
        }
    }

    /// Compose a base arch with Bun's variant qualifier suffix (musl, baseline, etc.).
    fn bun_arch_with_qualifier(base_arch: &str, qualifier: Option<&str>) -> String {
        match qualifier {
            Some("musl") => format!("{base_arch}-musl"),
            Some("musl-baseline") => format!("{base_arch}-musl-baseline"),
            Some("baseline") => format!("{base_arch}-baseline"),
            Some(other) => format!("{base_arch}-{other}"),
            None => base_arch.to_string(),
        }
    }

    /// Whether Bun publishes a native Windows ARM64 archive for the given version.
    fn has_native_windows_arm64_archive(version: &str) -> bool {
        let version = version
            .strip_prefix("bun-v")
            .or_else(|| version.strip_prefix('v'))
            .unwrap_or(version);

        // The historical x64-baseline fallback only applies to concrete semver
        // releases before the cutoff. Anything that isn't a clean `x.y.z` release
        // (a channel/tag like `canary`, or an unparseable ref) prefers the native
        // Windows ARM64 archive instead of silently forcing x64 emulation.
        let Some(version) = Versioning::new(version).filter(Versioning::is_ideal) else {
            return true;
        };

        version
            >= Versioning::new(WINDOWS_ARM64_NATIVE_ARCHIVE_VERSION)
                .expect("Windows ARM64 native archive cutoff must be a valid version")
    }

    /// Check if the current system has AVX2 support (runtime detection)
    #[cfg(target_arch = "x86_64")]
    fn has_avx2() -> bool {
        std::arch::is_x86_feature_detected!("avx2")
    }

    #[cfg(not(target_arch = "x86_64"))]
    fn has_avx2() -> bool {
        false
    }

    /// Check if we're running on a musl-based system
    /// Respects the global libc setting when configured, otherwise falls back
    /// to runtime detection (e.g. via `detect_libc`) for best effort accuracy.
    fn is_musl() -> bool {
        match Settings::get().libc() {
            Some("musl") => true,
            Some("gnu") => false,
            _ => detect_libc() == Some("musl"),
        }
    }

    /// Get the platform variant suffix for the current system
    /// Returns Some("baseline"), Some("musl"), Some("musl-baseline"), or None
    /// Uses runtime detection for AVX2 capability and Settings::get().arch() for MISE_ARCH support
    fn get_platform_variant() -> Option<&'static str> {
        let settings = Settings::get();
        match settings.arch() {
            "x64" => {
                if Self::is_musl() {
                    if Self::has_avx2() {
                        Some("musl")
                    } else {
                        Some("musl-baseline")
                    }
                } else if Self::has_avx2() {
                    None // Standard x64 with AVX2, no variant suffix
                } else {
                    Some("baseline")
                }
            }
            "arm64" => {
                if Self::is_musl() {
                    Some("musl")
                } else {
                    None // Standard aarch64, no variant suffix
                }
            }
            _ => None,
        }
    }

    /// Get the full Bun arch string with variants (musl, baseline, etc.) for the
    /// current system and the given Bun version.
    ///
    /// Uses Settings::get().arch() to respect MISE_ARCH overrides and runtime AVX2
    /// detection (both encapsulated in `current_platform_target`), then delegates to
    /// the shared `get_bun_arch_for_target` so the Windows ARM64 cutover logic lives
    /// in a single place.
    fn get_bun_arch_with_variants(version: &str) -> String {
        Self::get_bun_arch_for_target(&Self::current_platform_target(), version)
    }

    /// Build a `PlatformTarget` for the current system, mapping runtime AVX2/musl
    /// detection (via `get_platform_variant`) onto the platform qualifier.
    fn current_platform_target() -> PlatformTarget {
        let settings = Settings::get();
        PlatformTarget::new(Platform {
            os: settings.os().to_string(),
            arch: settings.arch().to_string(),
            qualifier: Self::get_platform_variant().map(str::to_string),
        })
    }
}

fn os() -> String {
    let settings = Settings::get();
    BunPlugin::map_os_to_bun(settings.os()).to_string()
}

fn arch(version: &str) -> String {
    BunPlugin::get_bun_arch_with_variants(version)
}

fn bun_bin_name() -> &'static str {
    if cfg!(windows) { "bun.exe" } else { "bun" }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(platform: &str) -> PlatformTarget {
        PlatformTarget::new(Platform::parse(platform).unwrap())
    }

    #[test]
    fn windows_arm64_asset_filename_matches_bun_release_cutover() {
        // Native `bun-windows-aarch64.zip` archives first shipped in Bun 1.3.10.
        // Older releases must keep using x64-baseline under emulation.
        for (version, expected) in [
            ("1.3.9", "bun-windows-x64-baseline.zip"),
            ("1.3.10", "bun-windows-aarch64.zip"),
            ("v1.3.10", "bun-windows-aarch64.zip"),
            ("bun-v1.3.10", "bun-windows-aarch64.zip"),
            ("1.3.14", "bun-windows-aarch64.zip"),
            // Non-semver refs (channels/tags) prefer the native archive.
            ("canary", "bun-windows-aarch64.zip"),
        ] {
            assert_eq!(
                BunPlugin::bun_asset_filename(&target("windows-arm64"), version),
                expected,
                "unexpected Windows ARM64 asset for Bun {version}"
            );
        }
    }

    #[test]
    fn bun_asset_filename_preserves_platform_variants() {
        // Regression: every other platform/variant must keep its existing asset name.
        for (platform, expected) in [
            ("linux-x64", "bun-linux-x64.zip"),
            ("linux-x64-baseline", "bun-linux-x64-baseline.zip"),
            ("linux-x64-musl", "bun-linux-x64-musl.zip"),
            ("linux-x64-musl-baseline", "bun-linux-x64-musl-baseline.zip"),
            ("linux-arm64", "bun-linux-aarch64.zip"),
            ("linux-arm64-musl", "bun-linux-aarch64-musl.zip"),
            ("macos-x64", "bun-darwin-x64.zip"),
            ("macos-x64-baseline", "bun-darwin-x64-baseline.zip"),
            ("macos-arm64", "bun-darwin-aarch64.zip"),
            ("windows-x64", "bun-windows-x64.zip"),
            ("windows-x64-baseline", "bun-windows-x64-baseline.zip"),
        ] {
            assert_eq!(
                BunPlugin::bun_asset_filename(&target(platform), "1.3.14"),
                expected,
                "unexpected asset for {platform}"
            );
        }
    }
}
