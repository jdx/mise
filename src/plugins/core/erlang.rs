use std::path::PathBuf;

use eyre::Result;

use crate::cli::args::ForgeArg;
use crate::file::display_path;
use crate::forge::Forge;
use crate::http::HTTP_FETCH;
use crate::install_context::InstallContext;
use crate::lock_file::LockFile;
use crate::plugins::core::CorePlugin;
use crate::toolset::ToolRequest;
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
        self.core
            .fa
            .cache_path
            .join(format!("kerl-{}", KERL_VERSION))
    }

    fn kerl_base_dir(&self) -> PathBuf {
        self.core.fa.cache_path.join("kerl")
    }

    fn lock_build_tool(&self) -> Result<fslock::LockFile> {
        LockFile::new(&self.kerl_path())
            .with_callback(|l| {
                trace!("install_or_update_kerl {}", l.display());
            })
            .lock()
    }

    async fn update_kerl(&self) -> Result<()> {
        let _lock = self.lock_build_tool();
        if self.kerl_path().exists() {
            // TODO: find a way to not have to do this #1209
            file::remove_all(self.kerl_base_dir())?;
            return Ok(());
        }
        self.install_kerl().await?;
        cmd!(self.kerl_path(), "update", "releases")
            .env("KERL_BASE_DIR", self.kerl_base_dir())
            .run()?;
        Ok(())
    }

    async fn install_kerl(&self) -> Result<()> {
        debug!("Installing kerl to {}", display_path(self.kerl_path()));
        HTTP_FETCH
            .download_file(
                format!("https://raw.githubusercontent.com/kerl/kerl/{KERL_VERSION}/kerl"),
                &self.kerl_path(),
                None,
            )
            .await?;
        file::make_executable(&self.kerl_path())?;
        Ok(())
    }

    async fn fetch_remote_versions(&self) -> Result<Vec<String>> {
        match self.core.fetch_remote_versions_from_mise().await {
            Ok(Some(versions)) => return Ok(versions),
            Ok(None) => {}
            Err(e) => warn!("failed to fetch remote versions: {}", e),
        }
        self.update_kerl().await?;
        let versions = CorePlugin::run_fetch_task_with_timeout(move || {
            let output = cmd!(self.kerl_path(), "list", "releases", "all")
                .env("KERL_BASE_DIR", self.core.fa.cache_path.join("kerl"))
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

#[async_trait]
impl Forge for ErlangPlugin {
    fn fa(&self) -> &ForgeArg {
        &self.core.fa
    }
    async fn _list_remote_versions(&self) -> Result<Vec<String>> {
        self.core
            .remote_version_cache
            .get_or_try_init(|| async { self.fetch_remote_versions().await })
            .await
            .cloned()
    }

    async fn install_version_impl<'a>(&'a self, ctx: &'a InstallContext<'a>) -> eyre::Result<()> {
        self.update_kerl().await?;

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
                .env("KERL_BASE_DIR", self.core.fa.cache_path.join("kerl"))
                .run()?;
            }
        }

        Ok(())
    }
}
