use crate::backend::backend_type::BackendType;
use crate::backend::platform::lookup_platform_key;
use crate::cli::args::BackendArg;
use crate::config::Config;
use crate::config::Settings;
use crate::http::HTTP;
use crate::install_context::InstallContext;
use crate::toolset::ToolVersion;
use crate::toolset::ToolVersionOptions;
use crate::{backend::Backend, file, github, gitlab, hash};
use async_trait::async_trait;
use eyre::{Result, bail};
use regex::Regex;
use std::fmt::Debug;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug)]
pub struct UnifiedGitBackend {
    ba: Arc<BackendArg>,
}

#[async_trait]
impl Backend for UnifiedGitBackend {
    fn get_type(&self) -> BackendType {
        if self.is_gitlab() {
            BackendType::Gitlab
        } else {
            BackendType::Github
        }
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<String>> {
        let repo = &self.ba.tool_name;
        if self.is_gitlab() {
            let releases = gitlab::list_releases(repo).await?;
            Ok(releases
                .into_iter()
                .map(|r| r.tag_name.trim_start_matches('v').to_string())
                .collect())
        } else {
            let releases = github::list_releases(repo).await?;
            Ok(releases
                .into_iter()
                .map(|r| r.tag_name.trim_start_matches('v').to_string())
                .collect())
        }
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> Result<ToolVersion> {
        let feature_name = if self.is_gitlab() {
            "gitlab backend"
        } else {
            "github backend"
        };
        Settings::get().ensure_experimental(feature_name)?;
        let repo = self.repo();
        let opts = tv.request.options();
        let api_url = opts
            .get("api_url")
            .map(|s| s.as_str())
            .unwrap_or(if self.is_gitlab() {
                "https://gitlab.com/api/v4"
            } else {
                "https://api.github.com"
            });

        // Find the asset URL for this specific version
        let asset_url = self.resolve_asset_url(&tv, &opts, repo, api_url).await?;

        // Download
        let filename = self.get_filename_from_url(&asset_url)?;
        let file_path = tv.download_path().join(&filename);

        ctx.pr.set_message(format!("download {filename}"));
        HTTP.download_file(&asset_url, &file_path, Some(&ctx.pr))
            .await?;

        // Only add checksum if it doesn't already exist (for lockfile verification)
        if let std::collections::btree_map::Entry::Vacant(e) = tv.checksums.entry(filename) {
            let hash = hash::file_hash_sha256(&file_path, Some(&ctx.pr))?;
            e.insert(format!("sha256:{hash}"));
        }

        // Verify
        self.verify_artifact(&tv, &file_path, &opts)?;

        // Install
        self.install_artifact(&tv, &file_path, &opts)?;

        // Verify checksum if specified
        self.verify_checksum(ctx, &mut tv, &file_path)?;

        Ok(tv)
    }

    async fn list_bin_paths(
        &self,
        _config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> Result<Vec<std::path::PathBuf>> {
        let opts = tv.request.options();
        if let Some(bin_path) = opts.get("bin_path") {
            Ok(vec![tv.install_path().join(bin_path)])
        } else {
            // Look for bin directory in the install path
            let bin_path = tv.install_path().join("bin");
            if bin_path.exists() {
                Ok(vec![bin_path])
            } else {
                // Look for bin directory in subdirectories (for extracted archives)
                let mut paths = Vec::new();
                if let Ok(entries) = std::fs::read_dir(tv.install_path()) {
                    for entry in entries.flatten() {
                        let sub_bin_path = entry.path().join("bin");
                        if sub_bin_path.exists() {
                            paths.push(sub_bin_path);
                        }
                    }
                }
                if !paths.is_empty() {
                    Ok(paths)
                } else {
                    Ok(vec![tv.install_path()])
                }
            }
        }
    }
}

impl UnifiedGitBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self { ba: Arc::new(ba) }
    }

    fn is_gitlab(&self) -> bool {
        self.ba.backend_type() == BackendType::Gitlab
    }

    fn repo(&self) -> &str {
        &self.ba.tool_name // e.g., "BurntSushi/ripgrep" or "gitlab-org/gitlab-runner"
    }

    async fn resolve_asset_url(
        &self,
        tv: &ToolVersion,
        opts: &ToolVersionOptions,
        repo: &str,
        api_url: &str,
    ) -> Result<String> {
        let version = if tv.version.starts_with('v') {
            tv.version.clone()
        } else {
            format!("v{}", tv.version)
        };

        // Check for direct platform-specific URLs first using the helper
        if let Some(direct_url) = lookup_platform_key(&opts.opts, "url") {
            return Ok(direct_url.clone());
        }

        if self.is_gitlab() {
            self.resolve_gitlab_asset_url(tv, opts, repo, api_url, &version)
                .await
        } else {
            self.resolve_github_asset_url(tv, opts, repo, api_url, &version)
                .await
        }
    }

    async fn resolve_github_asset_url(
        &self,
        _tv: &ToolVersion,
        opts: &ToolVersionOptions,
        repo: &str,
        api_url: &str,
        version: &str,
    ) -> Result<String> {
        let release = github::get_release_for_url(api_url, repo, version).await?;

        // Get platform-specific pattern first, then fall back to general pattern
        let pattern = lookup_platform_key(&opts.opts, "asset_pattern")
            .or_else(|| opts.get("asset_pattern"))
            .map(|s| s.as_str())
            .unwrap_or("{name}-{version}-{target}.{ext}");

        // Find matching asset - pattern is already templated by mise.toml parsing
        let asset = release
            .assets
            .into_iter()
            .find(|a| self.matches_pattern(&a.name, pattern))
            .ok_or_else(|| eyre::eyre!("No matching asset found for pattern: {}", pattern))?;

        Ok(asset.browser_download_url)
    }

    async fn resolve_gitlab_asset_url(
        &self,
        _tv: &ToolVersion,
        opts: &ToolVersionOptions,
        repo: &str,
        api_url: &str,
        version: &str,
    ) -> Result<String> {
        let release = gitlab::get_release_for_url(api_url, repo, version).await?;

        // Get platform-specific pattern first, then fall back to general pattern
        let pattern = lookup_platform_key(&opts.opts, "asset_pattern")
            .or_else(|| opts.get("asset_pattern"))
            .map(|s| s.as_str())
            .unwrap_or("{name}-{version}-{os}-{arch}.{ext}");

        // Find matching asset - pattern is already templated by mise.toml parsing
        let asset = release
            .assets
            .links
            .into_iter()
            .find(|a| self.matches_pattern(&a.name, pattern))
            .ok_or_else(|| eyre::eyre!("No matching asset found for pattern: {}", pattern))?;

        Ok(asset.direct_asset_url)
    }

    fn matches_pattern(&self, asset_name: &str, pattern: &str) -> bool {
        // Simple pattern matching - convert glob-like pattern to regex
        let regex_pattern = pattern
            .replace(".", "\\.")
            .replace("*", ".*")
            .replace("?", ".");

        if let Ok(re) = Regex::new(&format!("^{regex_pattern}$")) {
            re.is_match(asset_name)
        } else {
            // Fallback to simple contains check
            asset_name.contains(pattern)
        }
    }

    fn get_filename_from_url(&self, url: &str) -> Result<String> {
        Ok(url.split('/').next_back().unwrap_or("download").to_string())
    }

    fn verify_artifact(
        &self,
        _tv: &ToolVersion,
        file_path: &Path,
        opts: &ToolVersionOptions,
    ) -> Result<()> {
        // Check platform-specific checksum first
        let checksum = lookup_platform_key(&opts.opts, "checksum").or_else(|| opts.get("checksum"));

        if let Some(checksum) = checksum {
            self.verify_checksum_str(file_path, checksum)?;
        }

        // Check platform-specific size
        let size = lookup_platform_key(&opts.opts, "size").or_else(|| opts.get("size"));

        if let Some(size_str) = size {
            let expected_size: u64 = size_str.parse()?;
            let actual_size = file_path.metadata()?.len();
            if actual_size != expected_size {
                bail!(
                    "Size mismatch: expected {}, got {}",
                    expected_size,
                    actual_size
                );
            }
        }

        Ok(())
    }

    fn verify_checksum_str(&self, file_path: &Path, checksum: &str) -> Result<()> {
        if let Some((algo, hash_str)) = checksum.split_once(':') {
            hash::ensure_checksum(file_path, hash_str, None, algo)?;
        } else {
            bail!("Invalid checksum format: {}", checksum);
        }
        Ok(())
    }

    fn install_artifact(
        &self,
        tv: &ToolVersion,
        file_path: &Path,
        opts: &ToolVersionOptions,
    ) -> Result<()> {
        let install_path = tv.install_path();
        let strip_components = opts
            .get("strip_components")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        file::remove_all(&install_path)?;
        file::create_dir_all(&install_path)?;

        // Use TarFormat for format detection
        let ext = file_path.extension().and_then(|s| s.to_str()).unwrap_or("");
        let format = file::TarFormat::from_ext(ext);
        let tar_opts = file::TarOptions {
            format,
            strip_components,
            pr: None,
        };
        if format == file::TarFormat::Zip {
            file::unzip(file_path, &install_path)?;
        } else if format == file::TarFormat::Raw {
            // Copy the file directly to the bin_path directory or install_path
            if let Some(bin_path) = opts.get("bin_path") {
                let bin_dir = install_path.join(bin_path);
                file::create_dir_all(&bin_dir)?;
                let dest = bin_dir.join(file_path.file_name().unwrap());
                file::copy(file_path, &dest)?;
                file::make_executable(&dest)?;
            } else {
                let dest = install_path.join(file_path.file_name().unwrap());
                file::copy(file_path, &dest)?;
                file::make_executable(&dest)?;
            }
        } else {
            file::untar(file_path, &install_path, &tar_opts)?;
        }
        Ok(())
    }
}
