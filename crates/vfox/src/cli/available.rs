use vfox::{Vfox, VfoxResult};

#[derive(clap::Args)]
pub struct Available {}

impl Available {
    pub async fn run(&self) -> VfoxResult<()> {
        for (name, url) in Vfox::list_available_sdks() {
            println!("{name} {url}");
        }
        Ok(())
    }
}
