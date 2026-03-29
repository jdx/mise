use vfox::{Vfox, VfoxResult};

#[derive(clap::Args)]
pub struct EnvKeys {
    pub sdk: String,
    pub version: String,
}

impl EnvKeys {
    pub async fn run(&self) -> VfoxResult<()> {
        let vfox = Vfox::new();
        let install_path = vfox.install_dir.join(&self.sdk).join(&self.version);
        let env_keys = vfox
            .env_keys(
                &self.sdk,
                &self.version,
                install_path,
                serde_json::Value::Object(Default::default()),
            )
            .await?;
        for env_key in env_keys {
            println!("{}={}", env_key.key, env_key.value);
        }
        Ok(())
    }
}
