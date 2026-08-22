use vfox::{Vfox, VfoxResult};

#[derive(usage_rs::Args)]
pub(super) struct EnvKeys {
    sdk: String,
    version: String,
}

impl EnvKeys {
    pub(super) async fn run(&self) -> VfoxResult<()> {
        let vfox = Vfox::new();
        let env_keys = vfox
            .env_keys(
                &self.sdk,
                &self.version,
                serde_json::Value::Object(Default::default()),
            )
            .await?;
        for env_key in env_keys {
            println!("{}={}", env_key.key, env_key.value);
        }
        Ok(())
    }
}
