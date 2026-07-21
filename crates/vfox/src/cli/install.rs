use std::path::PathBuf;
use vfox::{Vfox, VfoxResult};

#[derive(clap::Args)]
pub struct Install {
    pub sdk: String,
    pub version: String,
    #[clap(short, long)]
    pub output_dir: Option<PathBuf>,
}

impl Install {
    pub async fn run(&self) -> VfoxResult<()> {
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
