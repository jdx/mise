use std::path::PathBuf;

use color_eyre::eyre::Result;

use crate::config::Settings;

use crate::file::display_path;
use crate::install_context::InstallContext;
use crate::lock_file::LockFile;
use crate::plugins::core::CorePlugin;
use crate::plugins::{Plugin, HTTP};
use crate::toolset::ToolVersionRequest;
use crate::{cmd, file};

#[derive(Debug)]
pub struct ErlangPlugin {
    core: CorePlugin,
}

const KERL_VERSION: &str = "4.0.0";

impl ErlangPlugin {
    pub fn new() -> Self {
        Self {
            core: CorePlugin::new("erlang"),
        }
    }

    fn kerl_path(&self) -> PathBuf {
        self.core.cache_path.join(format!("kerl-{}", KERL_VERSION))
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
            return Ok(());
        }
        self.install_kerl()?;
        cmd!(self.kerl_path(), "update", "releases")
            .env("KERL_BASE_DIR", self.core.cache_path.join("kerl"))
            .run()?;
        Ok(())
    }

    fn install_kerl(&self) -> Result<()> {
        debug!("Installing kerl to {}", display_path(&self.kerl_path()));
        HTTP.download_file(
            format!("https://raw.githubusercontent.com/kerl/kerl/{KERL_VERSION}/kerl"),
            &self.kerl_path(),
        )?;
        file::make_executable(&self.kerl_path())?;
        Ok(())
    }

    fn fetch_remote_versions(&self) -> Result<Vec<String>> {
        match self.core.fetch_remote_versions_from_rtx() {
            Ok(Some(versions)) => return Ok(versions),
            Ok(None) => {}
            Err(e) => warn!("failed to fetch remote versions: {}", e),
        }
        self.update_kerl()?;
        let versions = CorePlugin::run_fetch_task_with_timeout(move || {
            let output = cmd!(self.kerl_path(), "list", "releases", "all")
                .env("KERL_BASE_DIR", self.core.cache_path.join("kerl"))
                .read()?;
            let versions = output
                .split('\n')
                .filter(|s| regex!(r"^[0-9].+$").is_match(s))
                .map(|s| s.to_string())
                .collect();
            Ok(versions)
        })?;
        Ok(versions)
    }
}

impl Plugin for ErlangPlugin {
    fn name(&self) -> &str {
        self.core.name
    }

    fn list_remote_versions(&self, _settings: &Settings) -> Result<Vec<String>> {
        self.core
            .remote_version_cache
            .get_or_try_init(|| self.fetch_remote_versions())
            .cloned()
    }

    fn install_version_impl(&self, ctx: &InstallContext) -> Result<()> {
        self.update_kerl()?;

        file::remove_all(ctx.tv.install_path())?;
        match &ctx.tv.request {
            ToolVersionRequest::Ref(..) => {
                unimplemented!("erlang does not yet support refs");
            }
            _ => {
                cmd!(
                    self.kerl_path(),
                    "build-install",
                    &ctx.tv.version,
                    &ctx.tv.version,
                    ctx.tv.install_path()
                )
                .env("KERL_BASE_DIR", self.core.cache_path.join("kerl"))
                .run()?;
            }
        }

        Ok(())
    }
}
