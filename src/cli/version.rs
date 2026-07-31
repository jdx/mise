use std::path::Path;
use std::time::Duration;

use console::style;
use eyre::Result;
use std::sync::LazyLock as Lazy;
use versions::Versioning;

use crate::build_time::BUILD_TIME;
use crate::cli::self_update::{SelfUpdate, upgrade_instructions_or_hint};
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
    if let Some(latest) = check_for_new_version(duration::DAILY).await {
        warn!("mise version {} available", latest);
        if SelfUpdate::is_available() {
            let cmd = style("mise self-update").bright().yellow().for_stderr();
            warn!("To update, run {}", cmd);
        } else {
            warn!("{}", upgrade_instructions_or_hint());
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

/// State of the `latest-version` cache file.
///
/// The distinction between the two variants is the point: `Fresh(None)` means
/// "we checked recently and learned nothing", which is a negative cache and must
/// suppress another lookup. Collapsing it into `Stale` is what made a machine
/// that cannot reach the network re-check on every single invocation.
#[derive(Debug, PartialEq)]
enum Cached {
    /// Read within `duration`; the payload is whatever we last learned.
    Fresh(Option<String>),
    /// Missing, unreadable, or older than `duration`.
    Stale,
}

/// Classify the cache file.
///
/// Deliberately does not compare against [`V`]. Doing so conflated "the cache is
/// current" with "an update exists", so every outcome meaning "no update
/// available" — a failed request, or running a build newer than the latest
/// release — looked stale forever. `check_for_new_version` applies `*V < latest`
/// itself.
fn cached_latest_version(path: &Path, duration: Duration) -> Cached {
    match modified_duration(path) {
        Ok(age) if age < duration => match file::read_to_string(path) {
            // Read succeeded, so this reflects what the last check learned —
            // possibly nothing, which is the negative cache.
            Ok(body) => Cached::Fresh(Versioning::new(body.trim()).map(|v| v.to_string())),
            // Could not read it at all. Distinct from an empty body: treating a
            // permissions problem or transient I/O error as a negative cache
            // would suppress update checks for the whole TTL on the strength of
            // a file we never actually saw.
            Err(_) => Cached::Stale,
        },
        _ => Cached::Stale,
    }
}

async fn get_latest_version(duration: Duration) -> Option<String> {
    let version_file_path = dirs::CACHE.join("latest-version");
    if let Cached::Fresh(version) = cached_latest_version(&version_file_path, duration) {
        return version;
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
    fetch_latest_version(&crate::http::HTTP).await
}

async fn fetch_latest_version(client: &crate::http::Client) -> Option<String> {
    let url = "https://mise.jdx.dev/VERSION";
    debug!("checking mise version from {}", url);
    match client.get_text(url).await {
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

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: Duration = Duration::from_secs(3600);

    fn write(dir: &Path, body: &str) -> std::path::PathBuf {
        let p = dir.join("latest-version");
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn missing_file_is_stale() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("latest-version");
        assert_eq!(cached_latest_version(&p, HOUR), Cached::Stale);
    }

    #[test]
    fn fresh_file_returns_its_version() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(dir.path(), "2030.1.2\n");
        assert_eq!(
            cached_latest_version(&p, HOUR),
            Cached::Fresh(Some("2030.1.2".to_string()))
        );
    }

    /// The negative cache. A check that learned nothing still records *when* it
    /// ran, so the next invocation must not retry — that retry loop is what made
    /// a machine with an unreachable network re-parse the CA trust store on
    /// every single run.
    #[test]
    fn fresh_but_empty_file_is_a_negative_cache_not_stale() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(dir.path(), "");
        assert_eq!(cached_latest_version(&p, HOUR), Cached::Fresh(None));
    }

    /// A file we cannot read is not evidence of anything, so it must not act as a
    /// negative cache — otherwise a permissions problem or transient I/O error
    /// silently suppresses update checks for the whole TTL.
    #[test]
    fn fresh_but_unreadable_file_is_stale_not_a_negative_cache() {
        let dir = tempfile::tempdir().unwrap();
        // A directory where a file is expected: fresh mtime, but every read fails.
        let p = dir.path().join("latest-version");
        std::fs::create_dir(&p).unwrap();
        assert_eq!(cached_latest_version(&p, HOUR), Cached::Stale);
    }

    /// `Versioning` is permissive and parses almost any string, so garbage in the
    /// cache round-trips as a version rather than reading as "learned nothing".
    /// Pinned here because it means the empty file above is the *only* negative
    /// cache we get — a stricter parser would give us a second one for free, and
    /// anyone tempted to widen this should know which behaviour they are relying on.
    #[test]
    fn versioning_is_permissive_so_garbage_is_not_a_negative_cache() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(dir.path(), "not-a-version\n");
        assert_eq!(
            cached_latest_version(&p, HOUR),
            Cached::Fresh(Some("not-a-version".to_string()))
        );
    }

    /// A zero TTL expires any file without having to fake an mtime.
    #[test]
    fn expired_file_is_stale() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(dir.path(), "2030.1.2\n");
        assert_eq!(cached_latest_version(&p, Duration::ZERO), Cached::Stale);
    }

    #[tokio::test]
    async fn latest_version_ignores_http_client_initialization_errors() {
        let client = crate::http::Client::with_init_error("builder error: OpenSSL error");

        assert_eq!(fetch_latest_version(&client).await, None);
    }

    /// Regression: freshness must not depend on the cached version being newer
    /// than ours. It used to, which meant anyone running a build newer than the
    /// latest release re-ran the full lookup on every invocation.
    #[test]
    fn cache_is_honoured_even_when_it_is_older_than_us() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(dir.path(), "0.0.1\n");
        assert_eq!(
            cached_latest_version(&p, HOUR),
            Cached::Fresh(Some("0.0.1".to_string()))
        );
    }
}
