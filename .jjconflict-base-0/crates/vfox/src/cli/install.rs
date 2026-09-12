use std::path::PathBuf;
use vfox::{Vfox, VfoxResult};

#[derive(usage_rs::Args)]
pub(super) struct Install {
    sdk: String,
    version: String,
    #[usage(short, long)]
    output_dir: Option<PathBuf>,
}

impl Install {
    pub(super) async fn run(&self) -> VfoxResult<()> {
        let vfox = Vfox::new();
        let out = self
            .output_dir
            .clone()
            .unwrap_or_else(|| vfox.install_dir.join(&self.sdk).join(&self.version));
        info!(
            "Installing {} version {} to {out:?}",
            self.sdk, self.version
        );
        vfox.install(&self.sdk, &self.version, &out).await?;
        Ok(())
    }
}
