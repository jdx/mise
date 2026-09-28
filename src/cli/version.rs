use std::path::Path;
use std::time::Duration;

use console::style;
use eyre::Result;
use versions::Versioning;

use crate::build_time::BUILD_TIME;
use crate::cli::self_update::{SelfUpdate, upgrade_instructions_or_hint};
use crate::config::Settings;
use crate::file::modified_duration;
use crate::platform::{ARCH, OS};
use crate::ui::style;
use crate::version::{V, VERSION};
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
        miseprintln!("{}", serde_json::to_string_pretty(&json)?);
        Ok(())
    }
}

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
    if (ci_info::is_ci() && !cfg!(test))
        || Settings::try_get().is_ok_and(|settings| settings.disable_update_warning)
    {
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
            "mise settings auto_update=true"
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

/// Cache publication metadata, not the selected version: releases may become
/// eligible while the network cache is still fresh.
fn cached_release_index(path: &Path, duration: Duration) -> Cached {
    match modified_duration(path) {
        Ok(age) if age < duration => match file::read_to_string(path) {
            // Read succeeded, so this reflects what the last check learned —
            // possibly nothing, which is the negative cache.
            Ok(body) => Cached::Fresh((!body.trim().is_empty()).then(|| body.trim().to_string())),
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
    let age = self_update_release_age(None);
    let cutoff = match crate::duration::parse_into_timestamp(&age) {
        Ok(cutoff) => cutoff,
        Err(err) => {
            debug!("invalid self-update release age: {err:#}");
            return None;
        }
    };
    // Separate from the old selected-version cache. The index is independent of
    // policy and is filtered again on every invocation, even within its TTL.
    let path = source.cache_path().with_extension("releases-v1");
    let index = match cached_release_index(&path, duration) {
        Cached::Fresh(index) => index,
        Cached::Stale => {
            let index = match fetch_release_index(&source).await {
                Ok(index) => Some(index),
                Err(err) => {
                    debug!("failed to check for version: {err:#}");
                    None
                }
            };
            let _ = file::create_dir_all(*dirs::CACHE);
            // Preserve negative caching for network failures.
            let _ = file::write(path, index.clone().unwrap_or_default());
            index
        }
    }?;
    match select_eligible_release(&index, cutoff) {
        Ok(version) => Some(version),
        Err(err) => {
            debug!("failed to select eligible version: {err:#}");
            None
        }
    }
}

pub(crate) fn self_update_release_age(cli: Option<&str>) -> String {
    let settings = Settings::try_get().ok();
    effective_release_age(
        cli,
        settings
            .as_ref()
            .and_then(|s| s.self_update.minimum_release_age.as_deref()),
        settings
            .as_ref()
            .and_then(|s| s.minimum_release_age.as_deref()),
    )
    .to_string()
}

fn effective_release_age<'a>(
    cli: Option<&'a str>,
    override_age: Option<&'a str>,
    global: Option<&'a str>,
) -> &'a str {
    cli.or(override_age).or(global).unwrap_or("24h")
}

// Only official mise release tags use this calendar-version format. This is not
// a comparator for arbitrary tool versions or backend resolution.
pub(crate) fn mise_release_key(version: &str) -> Result<(u32, u32, u32)> {
    let parts = version
        .trim_start_matches('v')
        .split('.')
        .collect::<Vec<_>>();
    eyre::ensure!(
        parts.len() == 3
            && parts
                .iter()
                .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())),
        "invalid mise release version: {version}"
    );
    Ok((parts[0].parse()?, parts[1].parse()?, parts[2].parse()?))
}

fn select_eligible_release(index: &str, cutoff: jiff::Timestamp) -> Result<String> {
    let mut candidates = Vec::new();
    for line in index.lines().filter(|line| !line.trim().is_empty()) {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        eyre::ensure!(fields.len() == 2, "invalid mise release index row");
        let key = mise_release_key(fields[0])?;
        let published = jiff::Timestamp::from_second(fields[1].parse()?)?;
        if published <= cutoff {
            candidates.push((key, fields[0].trim_start_matches('v')));
        }
    }
    candidates.sort_unstable_by_key(|(key, _)| *key);
    candidates.last().map(|(_, version)| (*version).to_string())
        .ok_or_else(|| eyre::eyre!("no mise release satisfies minimum release age; lower self_update.minimum_release_age or specify a version"))
}

pub(crate) async fn eligible_self_update_version(
    source: &SelfUpdateSource,
    cli_age: Option<&str>,
) -> Result<String> {
    source.validate()?;
    let age = self_update_release_age(cli_age);
    let cutoff = crate::duration::parse_into_timestamp(&age)?;
    select_eligible_release(&fetch_release_index(source).await?, cutoff)
}

async fn fetch_release_index(source: &SelfUpdateSource) -> Result<String> {
    if source.is_default() {
        crate::http::HTTP
            .get_text("https://mise.jdx.dev/releases.tsv")
            .await
    } else {
        // Fetch every page: an age cutoff can reach beyond the first 100 releases.
        // Do not reuse the tool-release cache or its bounded pagination.
        let mut index = String::new();
        for page in 1.. {
            let url = format!(
                "{}/repos/{}/releases?per_page=100&page={page}",
                source.api_url, source.repository
            );
            let headers = crate::github::get_headers(&url)?;
            let (releases, _): (Vec<crate::github::GithubRelease>, _) = crate::http::HTTP
                .json_headers_with_headers(&url, &headers)
                .await?;
            let done = releases.len() < 100;
            for release in releases.into_iter().filter(|r| {
                !r.draft
                    && !r.prerelease
                    && r.tag_name.starts_with('v')
                    && mise_release_key(&r.tag_name).is_ok()
            }) {
                let published = release.published_at.ok_or_else(|| {
                    eyre::eyre!("missing publication date for {}", release.tag_name)
                })?;
                let published: jiff::Timestamp = published.parse()?;
                index.push_str(&format!(
                    "{}\t{}\n",
                    release.tag_name,
                    published.as_second()
                ));
            }
            if done {
                break;
            }
        }
        Ok(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_update_age_precedence() {
        assert_eq!(effective_release_age(None, None, None), "24h");
        assert_eq!(effective_release_age(None, None, Some("7d")), "7d");
        assert_eq!(effective_release_age(None, Some("0s"), Some("7d")), "0s");
        assert_eq!(
            effective_release_age(Some("2d"), Some("0s"), Some("7d")),
            "2d"
        );
    }

    #[test]
    fn release_age_selection_is_inclusive_and_uses_mise_release_order() {
        let index = "v2026.10.0 200\nv2026.9.30 100\nv2026.9.29 300\n";
        assert_eq!(
            select_eligible_release(index, jiff::Timestamp::from_second(200).unwrap()).unwrap(),
            "2026.10.0"
        );
        assert_eq!(
            select_eligible_release(index, jiff::Timestamp::from_second(199).unwrap()).unwrap(),
            "2026.9.30"
        );
        assert!(select_eligible_release(index, jiff::Timestamp::from_second(99).unwrap()).is_err());
    }

    #[test]
    fn release_index_fails_closed() {
        let cutoff = jiff::Timestamp::from_second(1000).unwrap();
        for index in [
            "",
            "v2026.1.0",
            "v2026.1.0 unknown",
            "latest 1",
            "v2026.1.0 1 extra",
            "v2026.1.0 1\nv2026.2.0 unknown",
        ] {
            assert!(select_eligible_release(index, cutoff).is_err(), "{index}");
        }
    }

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
        assert_eq!(cached_release_index(&p, HOUR), Cached::Stale);
    }

    #[test]
    fn fresh_file_returns_its_index() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(dir.path(), "v2030.1.2 100\n");
        assert_eq!(
            cached_release_index(&p, HOUR),
            Cached::Fresh(Some("v2030.1.2 100".to_string()))
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
        assert_eq!(cached_release_index(&p, HOUR), Cached::Fresh(None));
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
        assert_eq!(cached_release_index(&p, HOUR), Cached::Stale);
    }

    #[test]
    fn cached_index_is_refiltered_as_releases_age() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(dir.path(), "v2026.1.1 100\nv2026.1.2 200\n");
        let Cached::Fresh(Some(index)) = cached_release_index(&p, HOUR) else {
            panic!("expected fresh release index");
        };
        assert_eq!(
            select_eligible_release(&index, jiff::Timestamp::from_second(199).unwrap()).unwrap(),
            "2026.1.1"
        );
        assert_eq!(
            select_eligible_release(&index, jiff::Timestamp::from_second(200).unwrap()).unwrap(),
            "2026.1.2"
        );
    }

    /// A zero TTL expires any file without having to fake an mtime.
    #[test]
    fn expired_file_is_stale() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(dir.path(), "v2030.1.2 100\n");
        assert_eq!(cached_release_index(&p, Duration::ZERO), Cached::Stale);
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

    /// Regression: freshness must not depend on the cached version being newer
    /// than ours. It used to, which meant anyone running a build newer than the
    /// latest release re-ran the full lookup on every invocation.
    #[test]
    fn cache_is_honoured_even_when_it_is_older_than_us() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(dir.path(), "v0.0.1 100\n");
        assert_eq!(
            cached_release_index(&p, HOUR),
            Cached::Fresh(Some("v0.0.1 100".to_string()))
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
