use crate::backend::backend_type::BackendType;
use crate::backend::platform_target::PlatformTarget;
use crate::cli::args::BackendArg;
use crate::cli::version::{ARCH, OS};
use crate::config::Config;
use crate::config::Settings;
use crate::file::{self, TarOptions};
use crate::http::HTTP_FETCH;
use crate::install_context::InstallContext;
use crate::lockfile::PlatformInfo;
use crate::toolset::ToolVersion;
use crate::{backend::Backend, hash, http::HTTP};
use async_trait::async_trait;
use eyre::{Result, bail};
use itertools::Itertools;
use serde::Deserialize;
use std::fmt::Debug;
use std::sync::Arc;
use versions::Versioning;

#[derive(Debug)]
pub struct CondaBackend {
    ba: Arc<BackendArg>,
}

impl CondaBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self { ba: Arc::new(ba) }
    }

    /// Get the conda channel from settings or tool options
    fn channel(&self) -> String {
        self.ba
            .opts()
            .get("channel")
            .cloned()
            .unwrap_or_else(|| Settings::get().conda.channel.clone())
    }

    /// Map mise OS/ARCH to conda subdir
    fn conda_subdir() -> &'static str {
        match (OS.as_str(), ARCH.as_str()) {
            ("linux", "x64") => "linux-64",
            ("linux", "arm64") => "linux-aarch64",
            ("macos", "x64") => "osx-64",
            ("macos", "arm64") => "osx-arm64",
            ("windows", "x64") => "win-64",
            _ => "noarch",
        }
    }

    /// Map PlatformTarget to conda subdir for lockfile resolution
    fn conda_subdir_for_platform(target: &PlatformTarget) -> &'static str {
        match (target.os_name(), target.arch_name()) {
            ("linux", "x64") => "linux-64",
            ("linux", "arm64") => "linux-aarch64",
            ("macos", "x64") => "osx-64",
            ("macos", "arm64") => "osx-arm64",
            ("windows", "x64") => "win-64",
            _ => "noarch",
        }
    }

    /// Build a proper download URL from the API response
    fn build_download_url(download_url: &str) -> String {
        if download_url.starts_with("//") {
            format!("https:{}", download_url)
        } else if download_url.starts_with('/') {
            format!("https://conda.anaconda.org{}", download_url)
        } else {
            download_url.to_string()
        }
    }

    /// Fetch package files from the anaconda.org API
    async fn fetch_package_files(&self) -> Result<Vec<CondaPackageFile>> {
        let channel = self.channel();
        let url = format!(
            "https://api.anaconda.org/package/{}/{}/files",
            channel,
            self.tool_name()
        );
        let files: Vec<CondaPackageFile> = HTTP_FETCH.json(&url).await?;
        Ok(files)
    }

    /// Find the best package file for a given version and platform
    fn find_package_file<'a>(
        &self,
        files: &'a [CondaPackageFile],
        version: &str,
        subdir: &str,
    ) -> Option<&'a CondaPackageFile> {
        // Filter by version and platform
        let matching: Vec<_> = files
            .iter()
            .filter(|f| f.version == version && f.attrs.subdir == subdir)
            .collect();

        if matching.is_empty() {
            return None;
        }

        // Prefer .conda format over .tar.bz2 (newer, faster)
        matching
            .iter()
            .find(|f| f.basename.ends_with(".conda"))
            .or_else(|| matching.iter().find(|f| f.basename.ends_with(".tar.bz2")))
            .copied()
    }

    /// Extract a conda package (.conda or .tar.bz2) to the install path
    fn extract_conda_package(
        &self,
        ctx: &InstallContext,
        tarball_path: &std::path::Path,
        install_path: &std::path::Path,
    ) -> Result<()> {
        let filename = tarball_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        if filename.ends_with(".conda") {
            // .conda format: ZIP containing pkg-*.tar.zst
            self.extract_conda_format(ctx, tarball_path, install_path)?;
        } else if filename.ends_with(".tar.bz2") {
            // Legacy format: plain tar.bz2
            ctx.pr.set_message(format!("extract {filename}"));
            let tar_opts = TarOptions {
                format: file::TarFormat::TarBz2,
                pr: Some(ctx.pr.as_ref()),
                ..Default::default()
            };
            file::untar(tarball_path, install_path, &tar_opts)?;
        } else {
            bail!("unsupported conda package format: {}", filename);
        }

        Ok(())
    }

    /// Extract .conda format (ZIP with inner tar.zst)
    fn extract_conda_format(
        &self,
        ctx: &InstallContext,
        conda_path: &std::path::Path,
        install_path: &std::path::Path,
    ) -> Result<()> {
        let filename = conda_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("package.conda");
        ctx.pr.set_message(format!("extract {filename}"));

        // Create a temp directory for extraction
        let temp_dir = conda_path.parent().unwrap().join("conda_extract_temp");
        file::create_dir_all(&temp_dir)?;

        // Unzip the .conda file
        file::unzip(conda_path, &temp_dir, &Default::default())?;

        // Find and extract pkg-*.tar.zst
        let pkg_tar = std::fs::read_dir(&temp_dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .find(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("pkg-") && n.ends_with(".tar.zst"))
            });

        if let Some(pkg_tar_path) = pkg_tar {
            let tar_opts = TarOptions {
                format: file::TarFormat::TarZst,
                pr: Some(ctx.pr.as_ref()),
                ..Default::default()
            };
            file::untar(&pkg_tar_path, install_path, &tar_opts)?;
        } else {
            bail!("could not find pkg-*.tar.zst in .conda archive");
        }

        // Clean up temp directory
        file::remove_all(&temp_dir)?;

        Ok(())
    }

    /// Verify SHA256 checksum if available
    fn verify_checksum(
        &self,
        tarball_path: &std::path::Path,
        expected_sha256: Option<&str>,
        pr: Option<&dyn crate::ui::progress_report::SingleReport>,
    ) -> Result<()> {
        if let Some(expected) = expected_sha256 {
            hash::ensure_checksum(tarball_path, expected, pr, "sha256")?;
        }
        Ok(())
    }
}

#[async_trait]
impl Backend for CondaBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Conda
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<String>> {
        let files = self.fetch_package_files().await?;
        let subdir = Self::conda_subdir();

        // Filter by current platform and extract unique versions
        let versions: Vec<String> = files
            .iter()
            .filter(|f| f.attrs.subdir == subdir || f.attrs.subdir == "noarch")
            .map(|f| f.version.clone())
            .unique()
            .sorted_by_cached_key(|v| Versioning::new(v))
            .collect();

        Ok(versions)
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> Result<ToolVersion> {
        Settings::get().ensure_experimental("conda backend")?;
        let files = self.fetch_package_files().await?;
        let subdir = Self::conda_subdir();

        // Find the package file for this version
        let pkg_file = self
            .find_package_file(&files, &tv.version, subdir)
            .or_else(|| self.find_package_file(&files, &tv.version, "noarch"));

        let pkg_file = match pkg_file {
            Some(f) => f,
            None => bail!(
                "conda package {}@{} not found for platform {}",
                self.tool_name(),
                tv.version,
                subdir
            ),
        };

        // Build download URL
        let download_url = Self::build_download_url(&pkg_file.download_url);

        // Download the package
        // basename may contain a path prefix (e.g., "osx-arm64/ruff-0.8.0-py311h_0.conda")
        let filename = std::path::Path::new(&pkg_file.basename)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&pkg_file.basename);
        let tarball_path = tv.download_path().join(filename);
        if !tarball_path.exists() {
            ctx.pr
                .set_message(format!("download {}", pkg_file.basename));
            HTTP.download_file(&download_url, &tarball_path, Some(ctx.pr.as_ref()))
                .await?;
        }

        // Verify checksum
        self.verify_checksum(
            &tarball_path,
            pkg_file.sha256.as_deref(),
            Some(ctx.pr.as_ref()),
        )?;

        // Store lockfile info
        let platform_key = self.get_platform_key();
        let platform_info = tv.lock_platforms.entry(platform_key).or_default();
        platform_info.url = Some(download_url);
        if let Some(sha256) = &pkg_file.sha256 {
            platform_info.checksum = Some(format!("sha256:{}", sha256));
        }

        // Extract to install path
        let install_path = tv.install_path();
        file::remove_all(&install_path)?;
        file::create_dir_all(&install_path)?;
        self.extract_conda_package(ctx, &tarball_path, &install_path)?;

        // Make binaries executable
        let bin_path = install_path.join("bin");
        if bin_path.exists() {
            for entry in std::fs::read_dir(&bin_path)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_file() {
                    file::make_executable(&path)?;
                }
            }
        }

        Ok(tv)
    }

    async fn resolve_lock_info(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        let files = self.fetch_package_files().await?;
        let subdir = Self::conda_subdir_for_platform(target);

        // Find the package file for this version and platform
        let pkg_file = self
            .find_package_file(&files, &tv.version, subdir)
            .or_else(|| self.find_package_file(&files, &tv.version, "noarch"));

        match pkg_file {
            Some(pkg_file) => {
                let download_url = Self::build_download_url(&pkg_file.download_url);
                Ok(PlatformInfo {
                    url: Some(download_url),
                    checksum: pkg_file.sha256.as_ref().map(|s| format!("sha256:{}", s)),
                    name: None,
                    size: None,
                    url_api: None,
                })
            }
            None => {
                // No package available for this platform
                Ok(PlatformInfo {
                    url: None,
                    checksum: None,
                    name: None,
                    size: None,
                    url_api: None,
                })
            }
        }
    }
}

/// Represents a conda package file from the anaconda.org API
#[derive(Debug, Deserialize)]
struct CondaPackageFile {
    version: String,
    basename: String,
    download_url: String,
    sha256: Option<String>,
    #[serde(default)]
    attrs: CondaPackageAttrs,
}

/// Package attributes including platform info
#[derive(Debug, Default, Deserialize)]
struct CondaPackageAttrs {
    #[serde(default)]
    subdir: String,
}
