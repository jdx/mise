use color_eyre::eyre::Result;
use lazy_static::lazy_static;

use crate::build_time::BUILD_TIME;
use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;

#[derive(Debug, clap::Args)]
#[clap(about = "Show rtx version", alias = "-v", alias = "v")]
pub struct Version {}

lazy_static! {
    pub static ref VERSION: String = format!(
        "{} (built on {})",
        env!("CARGO_PKG_VERSION"),
        BUILD_TIME.format("%Y-%m-%d")
    );
}

impl Command for Version {
    fn run(self, _config: Config, out: &mut Output) -> Result<()> {
        let v = VERSION.to_string();
        rtxprintln!(out, "{v}");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::cli::Cli;

    use super::*;

    #[test]
    fn test_version() {
        let config = Config::load().unwrap();
        let mut out = Output::tracked();

        Cli::new()
            .run(config, &vec!["rtx".into(), "version".into()], &mut out)
            .unwrap();

        let expected = format!("rtx {}", env!("CARGO_PKG_VERSION"));
        assert!(out.stdout.content.contains(&expected));
    }
}
