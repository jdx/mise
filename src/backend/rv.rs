use crate::backend::Backend;
use crate::backend::backend_type::BackendType;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::Settings;
use crate::file;
use crate::install_context::InstallContext;
use crate::toolset::ToolVersion;
use crate::{Result, config::Config};
use async_trait::async_trait;
use eyre::bail;
use itertools::Itertools;
use serde::Deserialize;
use std::collections::HashSet;
use std::{fmt::Debug, path::PathBuf, sync::Arc};

#[derive(Debug)]
pub struct RvBackend {
    ba: Arc<BackendArg>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct RvRubyVersion {
    key: String,
    version: String,
    path: String,
    arch: String,
    os: String,
    gem_root: Option<String>,
    installed: bool,
    active: bool,
}

#[async_trait]
impl Backend for RvBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Rv
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    fn get_dependencies(&self) -> eyre::Result<Vec<&str>> {
        // rv is not managed by mise (for MVP)
        Ok(vec![])
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> eyre::Result<Vec<String>> {
        self.ensure_rv_available()?;
        self.check_platform_support()?;

        let settings = Settings::get();
        let current_os = settings.os();
        let current_arch = settings.arch();

        trace!("Listing rv ruby versions for OS={} ARCH={}", current_os, current_arch);

        // Run: rv ruby list --format json
        let output = cmd!("rv", "ruby", "list", "--format", "json").read()?;

        let rv_os = Self::rv_os_name(current_os);
        let rv_arch = Self::rv_arch_name(current_arch);

        // Parse JSON
        let versions: Vec<RvRubyVersion> = serde_json::from_str(&output)?;

        // Filter by OS and arch, normalize versions, and deduplicate
        let mut seen = HashSet::new();
        let filtered_versions: Vec<String> = versions
            .into_iter()
            .filter(|v| v.os == rv_os && v.arch == rv_arch)
            .map(|v| Self::normalize_version(&v.version))
            .filter(|v| seen.insert(v.clone()))
            .sorted_by_cached_key(|v| versions::Versioning::new(v))
            .collect();

        trace!("Found {} rv ruby versions after filtering", filtered_versions.len());
        Ok(filtered_versions)
    }

    async fn install_version_(&self, ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion> {
        self.ensure_rv_available()?;
        self.check_platform_support()?;

        let version = &tv.version;
        let rv_path = Self::rv_install_path(version);
        let mise_path = tv.install_path();

        info!(
            "Installing Ruby {} via rv to {}",
            version,
            rv_path.display()
        );

        // Run: rv ruby install {version} --install-dir {rv_path}
        CmdLineRunner::new("rv")
            .arg("ruby")
            .arg("install")
            .arg(version)
            .arg("--install-dir")
            .arg(&rv_path)
            .with_pr(ctx.pr.as_ref())
            .execute()?;

        // Remove existing symlink/directory if present
        if mise_path.exists() {
            if mise_path.is_symlink() {
                trace!("Removing existing symlink at {}", mise_path.display());
                std::fs::remove_file(&mise_path)?;
            } else {
                trace!("Removing existing directory at {}", mise_path.display());
                std::fs::remove_dir_all(&mise_path)?;
            }
        }

        // Create parent directory if needed
        if let Some(parent) = mise_path.parent() {
            file::create_dir_all(parent)?;
        }

        // Create symlink: mise_path -> rv_path (Unix only)
        #[cfg(unix)]
        {
            trace!(
                "Creating symlink: {} -> {}",
                mise_path.display(),
                rv_path.display()
            );
            std::os::unix::fs::symlink(&rv_path, &mise_path)?;
        }

        #[cfg(not(unix))]
        {
            bail!("rv backend only supports Unix-like systems");
        }

        // Install default gems if configured
        self.install_default_gems(&ctx.config, &tv).await?;

        Ok(tv)
    }

    async fn latest_version(
        &self,
        _config: &Arc<Config>,
        query: Option<String>,
    ) -> eyre::Result<Option<String>> {
        self.ensure_rv_available()?;

        let prefix = query.as_deref().unwrap_or("latest");

        // Use rv ruby find for prefix matching
        trace!("Finding latest rv ruby version matching: {}", prefix);
        let output = cmd!("rv", "ruby", "find", prefix).read();

        match output {
            Ok(version) => {
                let version = version.trim();
                if version.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(Self::normalize_version(version)))
                }
            }
            Err(_) => Ok(None),
        }
    }

    fn list_installed_versions(&self) -> Vec<String> {
        let rubies_dir = Self::rv_rubies_dir();
        if !rubies_dir.exists() {
            return vec![];
        }

        let mut versions = vec![];
        if let Ok(entries) = std::fs::read_dir(&rubies_dir) {
            for entry in entries.flatten() {
                if let Ok(file_name) = entry.file_name().into_string() {
                    // Extract version from "ruby-3.4.5" -> "3.4.5"
                    if let Some(version) = file_name.strip_prefix("ruby-") {
                        versions.push(version.to_string());
                    }
                }
            }
        }

        versions.sort_by_cached_key(|v| versions::Versioning::new(v));
        versions
    }
}

impl RvBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self { ba: Arc::new(ba) }
    }

    fn rv_rubies_dir() -> PathBuf {
        crate::dirs::HOME
            .join(".rubies")
    }

    fn rv_install_path(version: &str) -> PathBuf {
        Self::rv_rubies_dir().join(format!("ruby-{}", version))
    }

    fn ensure_rv_available(&self) -> Result<()> {
        if which::which("rv").is_err() {
            bail!(
                "rv is not installed.\n\
                Install via:\n\
                  brew install rv\n\
                \n\
                Or download from: https://github.com/spinel-coop/rv/releases"
            );
        }
        Ok(())
    }

    fn check_platform_support(&self) -> Result<()> {
        let os = std::env::consts::OS;
        match os {
            "macos" | "linux" => Ok(()),
            _ => bail!(
                "rv backend is not supported on {}.\n\
                Try using the core Ruby backend instead:\n\
                  mise use ruby@latest",
                os
            ),
        }
    }

    fn normalize_version(rv_version: &str) -> String {
        // Convert "ruby-3.4.5" or "3.4.5" to "3.4.5"
        rv_version
            .strip_prefix("ruby-")
            .unwrap_or(rv_version)
            .to_string()
    }

    fn rv_os_name(os: &str) -> &str {
        // rv uses "macos" and "linux" directly
        os
    }

    fn rv_arch_name(arch: &str) -> &str {
        // Convert mise arch names to rv arch names
        match arch {
            "arm64" => "aarch64",
            "x64" => "x86_64",
            _ => arch,
        }
    }

    async fn install_default_gems(&self, config: &Arc<Config>, tv: &ToolVersion) -> Result<()> {
        let default_gems_file = crate::dirs::HOME.join(".default-ruby-packages");

        if !default_gems_file.exists() {
            trace!("No .default-ruby-packages file found, skipping default gems");
            return Ok(());
        }

        let contents = file::read_to_string(&default_gems_file)?;
        let gem_bin = tv.install_path().join("bin/gem");

        if !gem_bin.exists() {
            warn!(
                "gem binary not found at {}, skipping default gems",
                gem_bin.display()
            );
            return Ok(());
        }

        info!("Installing default gems from ~/.default-ruby-packages");

        for line in contents.lines() {
            let line = line.trim();
            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Parse package name (and optional version/flags)
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }

            let package = parts[0];
            debug!("Installing default gem: {}", package);

            let result = CmdLineRunner::new(&gem_bin)
                .arg("install")
                .arg(package)
                .envs(self.dependency_env(config).await?)
                .execute();

            match result {
                Ok(_) => trace!("Successfully installed gem: {}", package),
                Err(e) => {
                    warn!("Failed to install gem {}: {:#}", package, e);
                    // Don't fail the entire installation if a gem fails
                }
            }
        }

        Ok(())
    }
}
