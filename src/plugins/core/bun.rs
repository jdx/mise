use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use async_trait::async_trait;
use eyre::Result;
use itertools::Itertools;
use versions::Versioning;

use crate::backend::static_helpers::fetch_checksum_from_shasums;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::http::HTTP;
use crate::install_context::InstallContext;
use crate::lockfile::PlatformInfo;
use crate::toolset::ToolVersion;
use crate::ui::progress_report::SingleReport;
use crate::{
    backend::{
        Backend, GitHubReleaseInfo, ReleaseType, VersionInfo, platform_target::PlatformTarget,
    },
    config::{Config, Settings},
    platform::Platform,
};
use crate::{file, github, plugins};

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
            .execute()
    }

    async fn download(&self, tv: &ToolVersion, pr: &dyn SingleReport) -> Result<PathBuf> {
        let url = format!(
            "https://github.com/oven-sh/bun/releases/download/bun-v{}/bun-{}-{}.zip",
            tv.version,
            os(),
            arch()
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
        file::rename(
            tv.download_path()
                .join(format!("bun-{}-{}", os(), arch()))
                .join(bun_bin_name()),
            self.bun_bin(tv),
        )?;
        if cfg!(unix) {
            file::make_executable(self.bun_bin(tv))?;
            file::make_symlink(Path::new("./bun"), &tv.install_path().join("bin/bunx"))?;
        }
        Ok(())
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

    async fn idiomatic_filenames(&self) -> Result<Vec<String>> {
        Ok(vec![".bun-version".into(), "package.json".into()])
    }

    async fn parse_idiomatic_file(&self, path: &Path) -> Result<String> {
        if path.file_name().is_some_and(|f| f == "package.json") {
            let pkg = crate::package_json::PackageJson::parse(path)?;
            return pkg
                .runtime_version("bun")
                .or_else(|| pkg.package_manager_version("bun"))
                .ok_or_else(|| eyre::eyre!("no bun version found in package.json"));
        }
        let contents = file::read_to_string(path)?;
        Ok(contents.trim().to_string())
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
        let os_name = Self::map_os_to_bun(target.os_name());
        let arch_name = Self::get_bun_arch_for_target(target);
        let asset_pattern = format!("bun-{os_name}-{arch_name}.zip");

        Ok(Some(GitHubReleaseInfo {
            repo: "oven-sh/bun".to_string(),
            asset_pattern: Some(asset_pattern),
            api_url: Some(format!(
                "https://github.com/oven-sh/bun/releases/download/bun-v{version}"
            )),
            release_type: ReleaseType::GitHub,
        }))
    }

    async fn resolve_lock_info(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        let version = &tv.version;

        // Build platform-specific filename
        let os_name = Self::map_os_to_bun(target.os_name());
        let arch_name = Self::get_bun_arch_for_target(target);
        let filename = format!("bun-{os_name}-{arch_name}.zip");

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

    /// Get the full Bun arch string for a target platform
    /// This handles musl, baseline, and other variants based on platform qualifiers
    fn get_bun_arch_for_target(target: &PlatformTarget) -> String {
        let base_arch = Self::map_arch_to_bun(target.arch_name());

        // Handle qualifiers like musl, baseline, etc.
        if let Some(qualifier) = target.qualifier() {
            match qualifier {
                "musl" => format!("{}-musl", base_arch),
                "musl-baseline" => format!("{}-musl-baseline", base_arch),
                "baseline" => format!("{}-baseline", base_arch),
                other => format!("{}-{}", base_arch, other),
            }
        } else {
            base_arch.to_string()
        }
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
    /// This is determined by the binary's compile-time target, since mixing
    /// glibc and musl binaries on the same system doesn't work anyway
    fn is_musl() -> bool {
        cfg!(target_env = "musl")
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

    /// Get the full Bun arch string with variants (musl, baseline, etc.)
    /// Uses Settings::get().arch() to respect MISE_ARCH overrides and runtime AVX2 detection
    fn get_bun_arch_with_variants() -> String {
        let settings = Settings::get();
        let arch = settings.arch();
        let os = settings.os();
        match arch {
            "x64" => {
                if Self::is_musl() {
                    if Self::has_avx2() {
                        "x64-musl".to_string()
                    } else {
                        "x64-musl-baseline".to_string()
                    }
                } else if Self::has_avx2() {
                    "x64".to_string()
                } else {
                    "x64-baseline".to_string()
                }
            }
            "arm64" => {
                if Self::is_musl() {
                    "aarch64-musl".to_string()
                } else if os == "windows" {
                    // Bun has no native windows-arm64 build; fall back to x64 under emulation
                    "x64-baseline".to_string()
                } else {
                    "aarch64".to_string()
                }
            }
            other => other.to_string(),
        }
    }
}

fn os() -> String {
    let settings = Settings::get();
    BunPlugin::map_os_to_bun(settings.os()).to_string()
}

fn arch() -> String {
    BunPlugin::get_bun_arch_with_variants()
}

fn bun_bin_name() -> &'static str {
    if cfg!(windows) { "bun.exe" } else { "bun" }
}
