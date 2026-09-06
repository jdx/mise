use std::path::Path;
use std::time::Duration;

use console::style;
use eyre::Result;
use std::sync::LazyLock as Lazy;
use versions::Versioning;

use crate::build_time::BUILD_TIME;
use crate::cli::self_update::{SelfUpdate, upgrade_instructions_or_hint};
use crate::config::Settings;
use crate::file::modified_duration;
use crate::ui::style;
use crate::{dirs, duration, env, file};

const DEFAULT_SELF_UPDATE_API_URL: &str = "https://api.github.com";
const DEFAULT_SELF_UPDATE_REPOSITORY: &str = "jdx/mise";

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct SelfUpdateSource {
    pub(crate) api_url: String,
    pub(crate) repository: String,
}

impl Default for SelfUpdateSource {
    fn default() -> Self {
        Self {
            api_url: DEFAULT_SELF_UPDATE_API_URL.to_string(),
            repository: DEFAULT_SELF_UPDATE_REPOSITORY.to_string(),
        }
    }
}

impl SelfUpdateSource {
    pub(crate) fn from_settings(settings: &Settings) -> Self {
        Self {
            api_url: settings
                .self_update
                .api_url
                .trim_end_matches('/')
                .to_string(),
            repository: settings.self_update.repository.clone(),
        }
    }

    pub(crate) fn current() -> Self {
        Settings::try_get()
            .map(|settings| Self::from_settings(&settings))
            .unwrap_or_default()
    }

    fn is_default(&self) -> bool {
        self.api_url == DEFAULT_SELF_UPDATE_API_URL
            && self.repository == DEFAULT_SELF_UPDATE_REPOSITORY
    }

    fn cache_path(&self) -> std::path::PathBuf {
        if self.is_default() {
            dirs::CACHE.join("latest-version")
        } else {
            dirs::CACHE.join(format!("latest-version-{}", crate::hash::hash_to_str(self)))
        }
    }

    #[cfg(feature = "self_update")]
    pub(crate) fn repository_parts(&self) -> Result<(&str, &str)> {
        let (owner, repo) = self.repository.split_once('/').ok_or_else(|| {
            eyre::eyre!(
                "self_update.repository must be in owner/repository format, got {:?}",
                self.repository
            )
        })?;
        eyre::ensure!(
            !owner.is_empty() && !repo.is_empty() && !repo.contains('/'),
            "self_update.repository must be in owner/repository format, got {:?}",
            self.repository
        );
        Ok((owner, repo))
    }

    pub(crate) fn validate(&self) -> Result<()> {
        let api_url = url::Url::parse(&self.api_url)?;
        eyre::ensure!(
            api_url.scheme() == "https",
            "self_update.api_url must use HTTPS, got {:?}",
            self.api_url
        );
        Ok(())
    }
}

/// Display the version of mise
///
/// Displays the version, os, architecture, and the date of the build.
///
/// If the version is out of date, it will display a warning.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    visible_alias = "v",
    example(
        r###"mise version
mise --version
mise -v
mise -V"###
    )
)]
pub(crate) struct Version {
    /// Print the version information in JSON format
    #[usage(short = 'J', long)]
    json: bool,
}

impl Version {
    pub(crate) async fn run(self) -> Result<()> {
        if self.json {
            self.json().await?
        } else {
            show_version()?;
            show_latest().await;
            show_version_hint();
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

pub(crate) static OS: Lazy<String> = Lazy::new(|| env::consts::OS.into());
pub(crate) static ARCH: Lazy<String> = Lazy::new(|| {
    match env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        _ => env::consts::ARCH,
    }
    .to_string()
});

/// Normalize OS name aliases to the canonical form used by `std::env::consts::OS`.
pub(crate) fn normalize_os(os: &str) -> &str {
    match os {
        "darwin" | "macos" => "macos",
        "windows" | "win" => "windows",
        other => other,
    }
}

/// Normalize architecture name aliases to the canonical form used by [`ARCH`].
pub(crate) fn normalize_arch(arch: &str) -> &str {
    match arch {
        "x86_64" | "amd64" | "x64" => "x64",
        "aarch64" | "arm64" => "arm64",
        other => other,
    }
}

/// Whether an `os` or `os/arch` selector matches the current platform.
pub(crate) fn os_selector_matches(entry: &str) -> bool {
    if let Some((os, arch)) = entry.split_once('/') {
        normalize_os(os) == OS.as_str() && normalize_arch(arch) == ARCH.as_str()
    } else {
        normalize_os(entry) == OS.as_str()
    }
}

pub(crate) static VERSION_PLAIN: Lazy<String> = Lazy::new(|| {
    let mut v = V.to_string();
    if cfg!(debug_assertions) {
        v.push_str("-DEBUG");
    };
    v
});

pub(crate) static VERSION: Lazy<String> = Lazy::new(|| {
    let build_time = BUILD_TIME.format("%Y-%m-%d");
    let v = &*VERSION_PLAIN;
    format!("{v} {os}-{arch} ({build_time})", os = *OS, arch = *ARCH)
});

pub(crate) static V: Lazy<Versioning> =
    Lazy::new(|| Versioning::new(env!("CARGO_PKG_VERSION")).unwrap());

pub(crate) fn print_version_if_requested(args: &[String]) -> std::io::Result<bool> {
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

pub(crate) async fn show_latest() {
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

#[derive(Debug, PartialEq)]
enum VersionHint {
    AutoUpdate,
    Homebrew,
    OptimizedBinary,
    OptimizedBinaryWindows,
}

fn select_version_hint(
    self_update_available: bool,
    auto_update: bool,
    homebrew: bool,
    windows: bool,
) -> Option<VersionHint> {
    if self_update_available {
        (!auto_update).then_some(VersionHint::AutoUpdate)
    } else if homebrew {
        Some(VersionHint::Homebrew)
    } else if windows {
        Some(VersionHint::OptimizedBinaryWindows)
    } else {
        Some(VersionHint::OptimizedBinary)
    }
}

pub(crate) fn show_auto_update_hint() {
    let Ok(settings) = Settings::try_get() else {
        return;
    };
    if select_version_hint(
        SelfUpdate::is_available(),
        settings.auto_update,
        false,
        cfg!(windows),
    ) == Some(VersionHint::AutoUpdate)
    {
        hint!(
            "auto_update",
            "keep mise updated automatically with",
            "mise settings set auto_update true"
        );
    }
}

pub(crate) fn show_version_hint() {
    let Ok(settings) = Settings::try_get() else {
        return;
    };
    match select_version_hint(
        SelfUpdate::is_available(),
        settings.auto_update,
        is_homebrew_install(),
        cfg!(windows),
    ) {
        Some(VersionHint::AutoUpdate) => show_auto_update_hint(),
        Some(VersionHint::Homebrew) => hint!(
            "optimized_mise_homebrew",
            "Homebrew's mise formula can be substantially slower and larger than the optimized mise.run binary; replace it with",
            "brew uninstall mise && curl https://mise.run | sh"
        ),
        Some(VersionHint::OptimizedBinary) => hint!(
            "optimized_mise_binary",
            "third-party package builds may be slower and larger than mise's optimized binary; install the official build with",
            "curl https://mise.run | sh"
        ),
        Some(VersionHint::OptimizedBinaryWindows) => hint!(
            "optimized_mise_binary",
            "third-party package builds may be slower and larger than mise's optimized binary; download the official build from",
            "https://github.com/jdx/mise/releases/latest"
        ),
        None => {}
    }
}

fn is_homebrew_install() -> bool {
    std::fs::canonicalize(&*env::MISE_BIN)
        .ok()
        .is_some_and(|path| path.components().any(|part| part.as_os_str() == "Cellar"))
}

pub(crate) async fn check_for_new_version(cache_duration: Duration) -> Option<String> {
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
    let source = SelfUpdateSource::current();
    if let Err(err) = source.validate() {
        debug!("invalid self-update source: {err:#}");
        return None;
    }
    let version_file_path = source.cache_path();
    if let Cached::Fresh(version) = cached_latest_version(&version_file_path, duration) {
        return version;
    }
    let _ = file::create_dir_all(*dirs::CACHE);
    let version = get_latest_version_call(&source).await;
    // Written even on failure, so its mtime acts as a negative cache and a
    // machine that cannot reach the network stops retrying once per invocation.
    let _ = file::write(version_file_path, version.clone().unwrap_or_default());
    version
}

#[cfg(test)]
async fn get_latest_version_call(_source: &SelfUpdateSource) -> Option<String> {
    Some("0.0.0".to_string())
}

#[cfg(not(test))]
async fn get_latest_version_call(source: &SelfUpdateSource) -> Option<String> {
    if source.is_default() {
        fetch_latest_version(&crate::http::HTTP).await
    } else {
        fetch_latest_github_version(source).await
    }
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

#[cfg(not(test))]
async fn fetch_latest_github_version(source: &SelfUpdateSource) -> Option<String> {
    debug!(
        "checking mise version from {}/{}",
        source.api_url, source.repository
    );
    match crate::github::get_release_for_url_with_versions_host(
        &source.api_url,
        &source.repository,
        "latest",
        false,
    )
    .await
    {
        Ok(release) => Some(
            release
                .tag_name
                .strip_prefix('v')
                .unwrap_or(&release.tag_name)
                .to_string(),
        ),
        Err(err) => {
            debug!("failed to check for version: {err:#?}");
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
    fn test_normalize_os() {
        assert_eq!(normalize_os("macos"), "macos");
        assert_eq!(normalize_os("darwin"), "macos");
        assert_eq!(normalize_os("linux"), "linux");
        assert_eq!(normalize_os("windows"), "windows");
        assert_eq!(normalize_os("win"), "windows");
        assert_eq!(normalize_os("freebsd"), "freebsd");
    }

    #[test]
    fn test_normalize_arch() {
        assert_eq!(normalize_arch("arm64"), "arm64");
        assert_eq!(normalize_arch("aarch64"), "arm64");
        assert_eq!(normalize_arch("x64"), "x64");
        assert_eq!(normalize_arch("x86_64"), "x64");
        assert_eq!(normalize_arch("amd64"), "x64");
        assert_eq!(normalize_arch("riscv64"), "riscv64");
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

    #[test]
    fn custom_self_update_sources_use_separate_version_caches() {
        let default = SelfUpdateSource::default();
        let custom = SelfUpdateSource {
            api_url: "https://github.example.com/api/v3".to_string(),
            repository: "acme/mise".to_string(),
        };

        assert_eq!(default.cache_path(), dirs::CACHE.join("latest-version"));
        assert_ne!(custom.cache_path(), default.cache_path());
        assert!(
            custom
                .cache_path()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("latest-version-")
        );
    }

    #[cfg(feature = "self_update")]
    #[test]
    fn self_update_repository_requires_exactly_owner_and_repository() {
        let source = |repository: &str| SelfUpdateSource {
            repository: repository.to_string(),
            ..SelfUpdateSource::default()
        };

        assert_eq!(
            source("acme/mise").repository_parts().unwrap(),
            ("acme", "mise")
        );
        assert!(source("mise").repository_parts().is_err());
        assert!(source("acme/mise/releases").repository_parts().is_err());
        assert!(source("/mise").repository_parts().is_err());
    }

    #[test]
    fn self_update_api_url_requires_https() {
        let source = |api_url: &str| SelfUpdateSource {
            api_url: api_url.to_string(),
            ..SelfUpdateSource::default()
        };

        assert!(
            source("https://github.example.com/api/v3")
                .validate()
                .is_ok()
        );
        assert!(
            source("http://github.example.com/api/v3")
                .validate()
                .is_err()
        );
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

    #[test]
    fn version_hint_promotes_auto_update_for_official_binaries() {
        assert_eq!(
            select_version_hint(true, false, false, false),
            Some(VersionHint::AutoUpdate)
        );
        assert_eq!(select_version_hint(true, true, false, false), None);
    }

    #[test]
    fn version_hint_promotes_optimized_binaries_for_package_installs() {
        assert_eq!(
            select_version_hint(false, false, true, false),
            Some(VersionHint::Homebrew)
        );
        assert_eq!(
            select_version_hint(false, false, false, false),
            Some(VersionHint::OptimizedBinary)
        );
        assert_eq!(
            select_version_hint(false, false, false, true),
            Some(VersionHint::OptimizedBinaryWindows)
        );
    }
}
