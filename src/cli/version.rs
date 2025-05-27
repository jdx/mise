use std::time::Duration;

use console::style;
use eyre::Result;
use std::sync::LazyLock as Lazy;
use versions::Versioning;

use crate::build_time::BUILD_TIME;
use crate::cli::self_update::SelfUpdate;
#[cfg(not(test))]
use crate::config::Settings;
use crate::file::modified_duration;
use crate::ui::style;
use crate::{dirs, duration, env, file};

/// Display the version of mise
///
/// Displays the version, os, architecture, and the date of the build.
///
/// If the version is out of date, it will display a warning.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, visible_alias = "v", after_long_help = AFTER_LONG_HELP)]
pub struct Version {
    /// Print the version information in JSON format
    #[clap(short = 'J', long)]
    json: bool,
}

impl Version {
    pub async fn run(self) -> Result<()> {
        if self.json {
            self.json().await?
        } else {
            show_version()?;
            show_latest().await;
        }
        Ok(())
    }

    async fn json(&self) -> Result<()> {
        let json = serde_json::json!({
            "version": *VERSION,
            "latest": get_latest_version(duration::DAILY).await,
            "os": *OS,
            "arch": *ARCH,
            "build_time": BUILD_TIME.to_string(),
        });
        println!("{}", serde_json::to_string_pretty(&json)?);
        Ok(())
    }
}

pub static OS: Lazy<String> = Lazy::new(|| env::consts::OS.into());
pub static ARCH: Lazy<String> = Lazy::new(|| {
    match env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        _ => env::consts::ARCH,
    }
    .to_string()
});

pub static VERSION: Lazy<String> = Lazy::new(|| {
    let mut v = V.to_string();
    if cfg!(debug_assertions) {
        v.push_str("-DEBUG");
    };
    let build_time = BUILD_TIME.format("%Y-%m-%d");
    format!("{v} {os}-{arch} ({build_time})", os = *OS, arch = *ARCH)
});

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise version</bold>
    $ <bold>mise --version</bold>
    $ <bold>mise -v</bold>
    $ <bold>mise -V</bold>
"#
);

pub static V: Lazy<Versioning> = Lazy::new(|| Versioning::new(env!("CARGO_PKG_VERSION")).unwrap());

pub fn print_version_if_requested(args: &[String]) -> std::io::Result<bool> {
    #[cfg(unix)]
    let mise_bin = "mise";
    #[cfg(windows)]
    let mise_bin = "mise.exe";
    if args.len() == 2
        && (*env::MISE_BIN_NAME == mise_bin || env::MISE_BIN_NAME.starts_with("mise-"))
    {
        let cmd = &args[1].to_lowercase();
        if cmd == "version" || cmd == "-v" || cmd == "--version" || cmd == "v" {
            show_version()?;
            return Ok(true);
        }
    }
    debug!("Version: {}", *VERSION);
    Ok(false)
}

fn show_version() -> std::io::Result<()> {
    if console::user_attended() {
        let banner = style::nred(
            r#"
              _                                        __              
   ____ ___  (_)_______        ___  ____        ____  / /___ _________
  / __ `__ \/ / ___/ _ \______/ _ \/ __ \______/ __ \/ / __ `/ ___/ _ \
 / / / / / / (__  )  __/_____/  __/ / / /_____/ /_/ / / /_/ / /__/  __/
/_/ /_/ /_/_/____/\___/      \___/_/ /_/     / .___/_/\__,_/\___/\___/
                                            /_/"#
                .trim_start_matches("\n"),
        );
        let jdx = style::nbright("by @jdx");
        miseprintln!("{banner}                 {jdx}");
    }
    miseprintln!("{}", *VERSION);
    Ok(())
}

pub async fn show_latest() {
    if ci_info::is_ci() && !cfg!(test) {
        return;
    }
    if let Some(latest) = check_for_new_version(duration::DAILY).await {
        warn!("mise version {} available", latest);
        if SelfUpdate::is_available() {
            let cmd = style("mise self-update").bright().yellow().for_stderr();
            warn!("To update, run {}", cmd);
        }
    }
}

pub async fn check_for_new_version(cache_duration: Duration) -> Option<String> {
    if let Some(latest) = get_latest_version(cache_duration)
        .await
        .and_then(Versioning::new)
    {
        if *V < latest {
            return Some(latest.to_string());
        }
    }
    None
}

async fn get_latest_version(duration: Duration) -> Option<String> {
    let version_file_path = dirs::CACHE.join("latest-version");
    if let Ok(metadata) = modified_duration(&version_file_path) {
        if metadata < duration {
            if let Ok(version) = file::read_to_string(&version_file_path) {
                return Some(version.trim().to_string());
            }
        }
    }
    let _ = file::create_dir_all(*dirs::CACHE);
    let version = get_latest_version_call().await;
    let _ = file::write(version_file_path, version.clone().unwrap_or_default());
    version
}

#[cfg(test)]
async fn get_latest_version_call() -> Option<String> {
    Some("0.0.0".to_string())
}

#[cfg(not(test))]
async fn get_latest_version_call() -> Option<String> {
    let settings = Settings::get();
    let url = match settings.paranoid {
        true => "https://mise.jdx.dev/VERSION",
        // using http is not a security concern and enabling tls makes mise significantly slower
        false => "http://mise.jdx.dev/VERSION",
    };
    debug!("checking mise version from {}", url);
    match crate::http::HTTP_VERSION_CHECK.get_text(url).await {
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
