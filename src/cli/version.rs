use std::string::ToString;

use color_eyre::eyre::Result;
use once_cell::sync::Lazy;

use crate::build_time::BUILD_TIME;
use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;

#[derive(Debug, clap::Args)]
#[clap(about = "Show rtx version", alias = "v")]
pub struct Version {}

pub static OS: Lazy<String> = Lazy::new(|| std::env::consts::OS.into());
pub static ARCH: Lazy<String> = Lazy::new(|| {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        _ => std::env::consts::ARCH,
    }
    .to_string()
});

pub static VERSION: Lazy<String> = Lazy::new(|| {
    format!(
        "{} {}-{} (built {})",
        if cfg!(debug_assertions) {
            format!("{}-DEBUG", env!("CARGO_PKG_VERSION"))
        } else {
            env!("CARGO_PKG_VERSION").to_string()
        },
        *OS,
        *ARCH,
        BUILD_TIME.format("%Y-%m-%d"),
    )
});

impl Command for Version {
    fn run(self, _config: Config, out: &mut Output) -> Result<()> {
        let v = VERSION.to_string();
        rtxprintln!(out, "{v}");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_str_eq;

    use crate::assert_cli;

    use super::*;

    #[test]
    fn test_version() {
        let stdout = assert_cli!("version");
        assert_str_eq!(stdout, VERSION.to_string() + "\n");
    }
}
