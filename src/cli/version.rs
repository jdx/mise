use std::string::ToString;
use std::time::Duration;

use color_eyre::eyre::Result;
use console::style;
use once_cell::sync::Lazy;
use versions::Versioning;

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
        show_version(out);
        Ok(())
    }
}

pub fn print_version_if_requested(args: &[String], out: &mut Output) {
    if args.len() == 2 {
        let cmd = &args[1].to_lowercase();
        if cmd == "version" || cmd == "-v" || cmd == "--version" {
            show_version(out);
            std::process::exit(0);
        }
    }
}

fn show_version(out: &mut Output) {
    rtxprintln!(out, "{}", *VERSION);
    show_latest();
}

fn show_latest() {
    if let Some(latest) = check_for_new_version() {
        warn!("rtx version {} available", latest);
        if cfg!(feature = "self_update") {
            let cmd = style("rtx self-update").bright().yellow().for_stderr();
            warn!("To update, run {}", cmd);
        }
    }
}

pub fn check_for_new_version() -> Option<String> {
    if let Some(latest) = get_latest_version() {
        let current = Versioning::new(env!("CARGO_PKG_VERSION")).unwrap();
        if current < latest {
            return Some(latest.to_string());
        }
    }
    None
}

fn get_latest_version() -> Option<Versioning> {
    reqwest::blocking::ClientBuilder::new()
        .timeout(Duration::from_secs(1))
        .build()
        .ok()?
        .get("https://rtx.jdxcode.com/VERSION")
        .send()
        .ok()
        .and_then(|res| {
            if res.status().is_success() {
                return res
                    .text()
                    .ok()
                    .and_then(|text| Versioning::new(text.as_str().trim()));
            }
            debug!("failed to check for version: {:#?}", res);
            None
        })
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
