use std::string::ToString;
use std::time::Duration;

use console::style;
use miette::Result;
use once_cell::sync::Lazy;
use versions::Versioning;

use crate::build_time::{built_info, BUILD_TIME};
use crate::cli::self_update::SelfUpdate;

use crate::env;
use crate::file::modified_duration;

use crate::{dirs, duration, file};

#[derive(Debug, clap::Args)]
#[clap(about = "Show mise version", alias = "v")]
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
    let mut version = RAW_VERSION.clone();
    if cfg!(debug_assertions) {
        version.push_str("-DEBUG");
    };
    let build_time = BUILD_TIME.format("%Y-%m-%d");
    let extra = match &built_info::GIT_COMMIT_HASH_SHORT {
        Some(sha) => format!("({} {})", sha, build_time),
        _ => format!("({})", build_time),
    };
    format!("{} {}-{} {}", version, *OS, *ARCH, extra)
});

pub static RAW_VERSION: Lazy<String> = Lazy::new(|| env!("CARGO_PKG_VERSION").to_string());

impl Version {
    pub fn run(self) -> Result<()> {
        show_version();
        Ok(())
    }
}

pub fn print_version_if_requested(args: &[String]) {
    if args.len() == 2 && *env::MISE_BIN_NAME == "mise" {
        let cmd = &args[1].to_lowercase();
        if cmd == "version" || cmd == "-v" || cmd == "--version" {
            show_version();
            std::process::exit(0);
        }
    }
    debug!("Version: {}", *VERSION);
}

fn show_version() {
    miseprintln!("{}", *VERSION);
    show_latest();
}

fn show_latest() {
    if *env::CI {
        return;
    }
    if let Some(latest) = check_for_new_version(duration::DAILY) {
        warn!("mise version {} available", latest);
        if SelfUpdate::is_available() {
            let cmd = style("mise self-update").bright().yellow().for_stderr();
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
            if let Ok(version) = file::read_to_string(&version_file_path) {
                return Some(version);
            }
        }
    }
    let _ = file::create_dir_all(*dirs::CACHE);
    let version = get_latest_version_call();
    let _ = file::write(version_file_path, version.clone().unwrap_or_default());
    version
}

#[cfg(test)]
fn get_latest_version_call() -> Option<String> {
    Some("0.0.0".to_string())
}

#[cfg(not(test))]
fn get_latest_version_call() -> Option<String> {
    const URL: &str = "http://mise.jdx.dev/VERSION";
    debug!("checking mise version from {}", URL);
    match crate::http::HTTP_VERSION_CHECK.get_text(URL) {
        Ok(text) => {
            debug!("got version {text}");
            Some(text.trim().to_string())
        }
        Err(err) => {
            debug!("failed to check for version: {:#?}", err);
            None
        }
    }
}
