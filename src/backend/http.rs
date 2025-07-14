use crate::backend::Backend;
use crate::backend::backend_type::BackendType;
use crate::backend::static_helpers::{
    get_filename_from_url, install_artifact, lookup_platform_key, template_string, verify_artifact,
};
use crate::cli::args::BackendArg;
use crate::config::Config;
use crate::config::Settings;
use crate::http::HTTP;
use crate::install_context::InstallContext;
use crate::toolset::ToolVersion;
use async_trait::async_trait;
use eyre::Result;
use std::fmt::Debug;
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
        let url_template = lookup_platform_key(&opts, "url")
            .or_else(|| opts.get("url").cloned())
            .ok_or_else(|| eyre::eyre!("Http backend requires 'url' option"))?;

        // Template the URL with actual values
        let url = template_string(&url_template, &tv);

        // Download
        let filename = get_filename_from_url(&url);
        let file_path = tv.download_path().join(&filename);

        // Store the asset URL in the tool version
        let platform_key = self.get_platform_key();
        let platform_info = tv.lock_platforms.entry(platform_key).or_default();
        platform_info.url = Some(url.clone());

        ctx.pr.set_message(format!("download {filename}"));
        HTTP.download_file(&url, &file_path, Some(&ctx.pr)).await?;

        // Verify (shared)
        verify_artifact(&tv, &file_path, &opts, Some(&ctx.pr))?;

        // Install (shared)
        install_artifact(&tv, &file_path, &opts, Some(&ctx.pr))?;

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
        if let Some(bin_path_template) = opts.get("bin_path") {
            let bin_path = template_string(bin_path_template, tv);
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
}
