use std::time::Duration;

use console::style;
use eyre::Result;
use std::sync::LazyLock as Lazy;
use versions::Versioning;

use crate::build_time::BUILD_TIME;
use crate::cli::self_update::SelfUpdate;
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

pub static VERSION_PLAIN: Lazy<String> = Lazy::new(|| {
    let mut v = V.to_string();
    if cfg!(debug_assertions) {
        v.push_str("-DEBUG");
    };
    v
});

pub static VERSION: Lazy<String> = Lazy::new(|| {
    let build_time = BUILD_TIME.format("%Y-%m-%d");
    let v = &*VERSION_PLAIN;
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
    if args.len() == 2 && !*crate::env::IS_RUNNING_AS_SHIM {
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
    // Bail before anything can touch the HTTP client. Constructing it eagerly
    // parses the system CA trust store, which is ~34M instructions — roughly 70%
    // of the cost of `mise --version`. The request itself would fail on the
    // offline check in http.rs anyway, so that work is pure waste.
    if Settings::get().offline() {
        return;
    }
    if let Some(latest) = check_for_new_version(duration::DAILY).await {
        warn!("mise version {} available", latest);
        if SelfUpdate::is_available() {
            let cmd = style("mise self-update").bright().yellow().for_stderr();
            warn!("To update, run {}", cmd);
        } else if let Some(instructions) = crate::cli::self_update::upgrade_instructions_text() {
            warn!("{}", instructions);
        }
    }
}

pub async fn check_for_new_version(cache_duration: Duration) -> Option<String> {
    if let Some(latest) = get_latest_version(cache_duration)
        .await
        .and_then(Versioning::new)
        && *V < latest
    {
        return Some(latest.to_string());
    }
    None
}

async fn get_latest_version(duration: Duration) -> Option<String> {
    let version_file_path = dirs::CACHE.join("latest-version");
    // Freshness is only about *when we last checked*, never about what we found.
    //
    // Previously this guard also required the cached version to be >= ours, which
    // conflated "the cache is current" with "an update exists". Every outcome
    // meaning "no update available" therefore failed the guard and re-ran the
    // whole check on the next invocation — and since a failed check writes an
    // empty file (below), an offline machine re-parsed the entire CA trust store
    // on *every* `mise --version`, forever. The `*V < latest` comparison that
    // actually decides whether to nag lives in `check_for_new_version`, so
    // dropping it here loses nothing.
    if let Ok(age) = modified_duration(&version_file_path)
        && age < duration
    {
        return file::read_to_string(&version_file_path)
            .ok()
            .map(|s| s.trim().to_string())
            .and_then(Versioning::new)
            .map(|v| v.to_string());
    }
    let _ = file::create_dir_all(*dirs::CACHE);
    let version = get_latest_version_call().await;
    // Written even on failure, so its mtime acts as a negative cache and a
    // machine that cannot reach the network stops retrying once per invocation.
    let _ = file::write(version_file_path, version.clone().unwrap_or_default());
    version
}

#[cfg(test)]
async fn get_latest_version_call() -> Option<String> {
    Some("0.0.0".to_string())
}

#[cfg(not(test))]
async fn get_latest_version_call() -> Option<String> {
    let url = "https://mise.jdx.dev/VERSION";
    debug!("checking mise version from {}", url);
    match crate::http::HTTP.get_text(url).await {
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
