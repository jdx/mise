use std::path::PathBuf;

use crate::backend::Backend;
use crate::cli::args::BackendArg;
use crate::file::display_path;
use crate::http::HTTP_FETCH;
use crate::install_context::InstallContext;
use crate::lock_file::LockFile;
use crate::toolset::ToolRequest;
use crate::{cmd, file, plugins};
use eyre::Result;
use xx::regex;

#[derive(Debug)]
pub struct ErlangPlugin {
    ba: BackendArg,
}

const KERL_VERSION: &str = "4.1.1";

impl ErlangPlugin {
    pub fn new() -> Self {
        Self {
            ba: plugins::core::new_backend_arg("erlang"),
        }
    }

    fn kerl_path(&self) -> PathBuf {
        self.ba.cache_path.join(format!("kerl-{}", KERL_VERSION))
    }

    fn kerl_base_dir(&self) -> PathBuf {
        self.ba.cache_path.join("kerl")
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
            // TODO: find a way to not have to do this #1209
            file::remove_all(self.kerl_base_dir())?;
            return Ok(());
        }
        self.install_kerl()?;
        cmd!(self.kerl_path(), "update", "releases")
            .env("KERL_BASE_DIR", self.kerl_base_dir())
            .run()?;
        Ok(())
    }

    fn install_kerl(&self) -> Result<()> {
        debug!("Installing kerl to {}", display_path(self.kerl_path()));
        HTTP_FETCH.download_file(
            format!("https://raw.githubusercontent.com/kerl/kerl/{KERL_VERSION}/kerl"),
            &self.kerl_path(),
            None,
        )?;
        file::make_executable(self.kerl_path())?;
        Ok(())
    }
}

impl Backend for ErlangPlugin {
    fn ba(&self) -> &BackendArg {
        &self.ba
    }
    fn _list_remote_versions(&self) -> Result<Vec<String>> {
        self.update_kerl()?;
        let versions = crate::plugins::core::run_fetch_task_with_timeout(move || {
            let output = cmd!(self.kerl_path(), "list", "releases", "all")
                .env("KERL_BASE_DIR", self.ba.cache_path.join("kerl"))
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

    fn install_version_impl(&self, ctx: &InstallContext) -> Result<()> {
        self.update_kerl()?;

        file::remove_all(ctx.tv.install_path())?;
        match &ctx.tv.request {
            ToolRequest::Ref { .. } => {
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
                .env("KERL_BASE_DIR", self.ba.cache_path.join("kerl"))
                .run()?;
            }
        }

        Ok(())
    }
}
