use vfox::{Vfox, VfoxResult};

#[derive(clap::Args)]
pub struct EnvKeys {
    pub sdk: String,
    pub version: String,
}

impl EnvKeys {
    pub async fn run(&self) -> VfoxResult<()> {
        let vfox = Vfox::new();
        let env_keys = vfox.env_keys(&self.sdk, &self.version).await?;
        for env_key in env_keys {
            println!("{}={}", env_key.key, env_key.value);
        }
        Ok(())
    }
}
