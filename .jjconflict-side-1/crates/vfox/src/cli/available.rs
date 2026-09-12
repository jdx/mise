use vfox::{Vfox, VfoxResult};

#[derive(usage_rs::Args)]
pub(super) struct Available {}

impl Available {
    pub(super) async fn run(&self) -> VfoxResult<()> {
        for (name, url) in Vfox::list_available_sdks() {
            println!("{name} {url}");
        }
        Ok(())
    }
}
