//! npm registry metadata queries backed by the `aube-registry` crate
//! (<https://github.com/jdx/aube>).
//!
//! Replaces shelling out to `npm view`, so listing versions for `npm:` tools
//! does not require node/npm to be installed. `aube-registry` owns all the
//! npm config semantics (`.npmrc` parsing, `${VAR}` expansion, scoped
//! registries, nerf-dart auth lookup, `NPM_CONFIG_*` env translation).
//!
//! Scope note, matching what `npm view` did when invoked with a neutral
//! `--prefix` (the previous behavior): the client's project dir is a
//! mise-owned cache dir, so a project `.npmrc` in the cwd cannot redirect or
//! authenticate mise-owned metadata queries. User-level `~/.npmrc` (or
//! `NPM_CONFIG_USERCONFIG`) and `NPM_CONFIG_*` env vars still apply.

use std::path::PathBuf;
use std::sync::LazyLock as Lazy;

use aube_registry::NetworkMode;
use aube_registry::client::RegistryClient;
use aube_registry::config::NpmConfig;
use eyre::Result;

use crate::backend::VersionInfo;
use crate::backend::npm::is_semver_prerelease;
use crate::config::Settings;

/// Process-wide npm registry client. Registry URLs, scoped registries, and
/// auth are read once from the environment and the user's `~/.npmrc`; the
/// neutral project dir keeps a cwd `.npmrc` out of mise-owned queries.
///
/// The network mode honors mise's offline flags so metadata queries don't hit
/// the network when the user asked for offline — `offline()` fails on a cold
/// cache, `prefer_offline()` serves the cache and only falls back to the
/// network on a miss. (Offline state is set from the CLI/env at startup, so
/// resolving it once at client construction is sufficient.)
static CLIENT: Lazy<RegistryClient> = Lazy::new(|| {
    let config = NpmConfig::load(&meta_dir());
    let settings = Settings::get();
    let mode = if settings.offline() {
        NetworkMode::Offline
    } else if settings.prefer_offline() {
        NetworkMode::PreferOffline
    } else {
        NetworkMode::Online
    };
    // Create the on-disk packument cache dir once here (client init runs at
    // most once per process) rather than on every async fetch — keeps the
    // blocking mkdir + its sync lock off the Tokio worker in the hot path.
    let _ = crate::file::create_dir_all(meta_dir());
    RegistryClient::from_config(config).with_network_mode(mode)
});

/// Neutral mise-owned directory used both as the client's "project dir" (so
/// no real project `.npmrc` is read) and as `aube-registry`'s on-disk
/// packument cache location.
fn meta_dir() -> PathBuf {
    crate::dirs::CACHE.join("npm-meta")
}

/// Fetch the full packument for `name`, honoring the configured registry and
/// on-disk cache. The full (non-corgi) route is used so the `time` map is
/// present for release-date / `minimum_release_age` filtering.
async fn fetch_packument(name: &str) -> Result<aube_registry::Packument> {
    Ok(CLIENT
        .fetch_packument_with_time_cached(name, &meta_dir())
        .await?)
}

/// List a package's versions as [`VersionInfo`], semver-ascending with publish
/// timestamps. npm registry versions are strict semver (the registry enforces
/// it) but the packument's `versions` map is keyed lexically, so the keys are
/// sorted to match the order `npm view versions` produced — prefix resolution
/// (e.g. `npm:@angular/cli@19`) depends on it. Anything unparseable keeps a
/// stable position at the end.
pub async fn list_versions(name: &str) -> Result<Vec<VersionInfo>> {
    let packument = fetch_packument(name).await?;
    let mut versions: Vec<&String> = packument.versions.keys().collect();
    versions.sort_by_cached_key(|v| {
        let parsed = versions::SemVer::new(v);
        (parsed.is_none(), parsed)
    });
    Ok(versions
        .into_iter()
        .map(|version| VersionInfo {
            version: version.clone(),
            created_at: packument.time.get(version).cloned(),
            prerelease: is_semver_prerelease(version),
            ..Default::default()
        })
        .collect())
}

/// Resolve the `latest` dist-tag for a package, if the registry publishes one.
pub async fn latest_dist_tag(name: &str) -> Result<Option<String>> {
    let packument = fetch_packument(name).await?;
    Ok(packument.dist_tags.get("latest").cloned())
}
