use vfox::Vfox;
use vfox::VfoxResult;

#[derive(usage_rs::Args)]
pub(crate) struct List {}

impl List {
    pub(crate) async fn run(&self) -> VfoxResult<()> {
        let vfox = Vfox::new();
        let sdks = vfox.list_sdks()?;
        for sdk in sdks {
            println!("{sdk}");
        }
        Ok(())
    }
}
