use crate::backend::backend_type::BackendType;
use crate::backend::platform::lookup_platform_key;
use crate::cli::args::BackendArg;
use crate::config::Config;
use crate::config::Settings;
use crate::http::HTTP;
use crate::install_context::InstallContext;
use crate::toolset::ToolVersion;
use crate::toolset::ToolVersionOptions;
use crate::{backend::Backend, file, hash};
use async_trait::async_trait;
use eyre::{Result, bail};
use std::fmt::Debug;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug)]
pub struct HttpBackend {
    ba: Arc<BackendArg>,
}

#[async_trait]
impl Backend for HttpBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Http
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<String>> {
        // Http backend doesn't support remote version listing
        Ok(vec![])
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> Result<ToolVersion> {
        Settings::get().ensure_experimental("http backend")?;
        let opts = tv.request.options();

        // Use the new helper to get platform-specific URL first, then fall back to general URL
        let url = lookup_platform_key(&opts, "url")
            .or_else(|| opts.get("url").cloned())
            .ok_or_else(|| eyre::eyre!("Http backend requires 'url' option"))?;

        // Download
        let filename = self.get_filename_from_url(&url)?;
        let file_path = tv.download_path().join(&filename);

        ctx.pr.set_message(format!("download {filename}"));
        HTTP.download_file(&url, &file_path, Some(&ctx.pr)).await?;

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
            // Always treat bin_path as a directory
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

impl HttpBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self { ba: Arc::new(ba) }
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
        // Check checksum if specified
        if let Some(checksum) = opts.get("checksum") {
            self.verify_checksum_str(file_path, checksum)?;
        }

        // Check size if specified
        if let Some(size_str) = opts.get("size") {
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
