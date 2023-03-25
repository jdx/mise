use std::fs;
use std::string::ToString;
use std::time::Duration;

use color_eyre::eyre::Result;
use console::style;
use once_cell::sync::Lazy;
use versions::Versioning;

use crate::build_time::{built_info, BUILD_TIME};
use crate::cli::command::Command;
use crate::config::Config;
use crate::env::CI;
use crate::file::modified_duration;
use crate::output::Output;
use crate::{dirs, duration};

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
        "{} {}-{} ({} {})",
        if cfg!(debug_assertions) {
            format!("{}-DEBUG", env!("CARGO_PKG_VERSION"))
        } else {
            env!("CARGO_PKG_VERSION").to_string()
        },
        *OS,
        *ARCH,
        built_info::GIT_COMMIT_HASH_SHORT.unwrap_or("unknown"),
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
    if args.len() == 2 && (args[0] == "rtx" || args[0].ends_with("/rtx")) {
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
    if *CI {
        return;
    }
    if let Some(latest) = check_for_new_version(duration::DAILY) {
        warn!("rtx version {} available", latest);
        if cfg!(feature = "self_update") {
            let cmd = style("rtx self-update").bright().yellow().for_stderr();
            warn!("To update, run {}", cmd);
        }
    }
}

pub fn check_for_new_version(cache_duration: Duration) -> Option<String> {
    if let Some(latest) = get_latest_version(cache_duration).and_then(|v| Versioning::new(&v)) {
        let current = Versioning::new(env!("CARGO_PKG_VERSION")).unwrap();
        if current < latest {
            return Some(latest.to_string());
        }
    }
    None
}

fn get_latest_version(duration: Duration) -> Option<String> {
    let version_file_path = dirs::CACHE.join("latest-version");
    if let Ok(metadata) = modified_duration(&version_file_path) {
        if metadata < duration {
            if let Ok(version) = fs::read_to_string(&version_file_path) {
                return Some(version);
            }
        }
    }
    let _ = fs::create_dir_all(&*dirs::CACHE);
    let version = get_latest_version_call();
    let _ = fs::write(version_file_path, version.clone().unwrap_or_default());
    version
}

#[cfg(test)]
fn get_latest_version_call() -> Option<String> {
    Some("0.0.0".to_string())
}

#[cfg(not(test))]
fn get_latest_version_call() -> Option<String> {
    let timeout = Duration::from_secs(3);
    const URL: &str = "http://rtx.pub/VERSION";
    debug!("checking for version from {}", URL);
    let client = reqwest::blocking::ClientBuilder::new()
        .user_agent(format!("rtx/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .ok()?;
    match client.get(URL).timeout(timeout).send() {
        Ok(res) => {
            if res.status().is_success() {
                return res.text().ok().map(|text| text.trim().to_string());
            }
            debug!("failed to check for version: {:#?}", res);
        }
        Err(err) => {
            debug!("failed to check for version: {:#?}", err);
        }
    };
    None
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
