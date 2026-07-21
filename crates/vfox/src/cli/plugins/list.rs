use vfox::Vfox;
use vfox::VfoxResult;

#[derive(clap::Args)]
#[command(alias = "ls")]
pub struct List {}

impl List {
    pub async fn run(&self) -> VfoxResult<()> {
        let vfox = Vfox::new();
        let sdks = vfox.list_sdks()?;
        for sdk in sdks {
            println!("{sdk}");
        }
        Ok(())
    }
}
