use color_eyre::eyre::Result;

use crate::build_time::BUILD_TIME;
use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;

#[derive(Debug, clap::Args)]
#[clap(about = "Show rtx version", alias = "-v", alias = "v")]
pub struct Version {}

impl Command for Version {
    fn run(self, _config: Config, out: &mut Output) -> Result<()> {
        rtxprintln!(
            out,
            "rtx {} (built on {})",
            env!("CARGO_PKG_VERSION"),
            BUILD_TIME.format("%Y-%m-%d")
        );
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
