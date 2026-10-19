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

use std::path::{Path, PathBuf};
use std::sync::LazyLock as Lazy;

use aube_registry::NetworkMode;
use aube_registry::client::RegistryClient;
use aube_registry::config::NpmConfig;
use eyre::Result;

use crate::backend::VersionInfo;
use crate::backend::npm::is_semver_prerelease;
use crate::config::{Settings, SettingsExt};

/// npm registry configuration from the environment and the user's
/// `~/.npmrc`, read once per process.
static CONFIG: Lazy<NpmConfig> = Lazy::new(|| NpmConfig::load(&meta_dir()));

/// Process-wide npm registry client. Registry URLs, scoped registries, and
/// auth are read once from the environment and the user's `~/.npmrc`; the
/// neutral project dir keeps a cwd `.npmrc` out of mise-owned queries.
///
/// The network mode honors mise's offline flags so metadata queries don't hit
/// the network when the user asked for offline — `offline()` fails on a cold
/// cache, `prefer_offline()` serves the cache and only falls back to the
/// network on a miss. (Offline state is set from the CLI/env at startup, so
/// resolving it once at client construction is sufficient.)
static CLIENT: Lazy<std::result::Result<RegistryClient, String>> = Lazy::new(|| {
    // Before the first registry request, so the memoized User-Agent is
    // mise's rather than standalone aube's.
    crate::backend::aube_host::init();
    let config = CONFIG.clone();
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
    RegistryClient::try_from_config(config)
        .map(|client| client.with_network_mode(mode))
        .map_err(|err| {
            let mut msg = format!("Could not initialize the npm registry HTTP client: {err}");
            let mut source = std::error::Error::source(&err);
            while let Some(cause) = source {
                msg.push_str(&format!(": {cause}"));
                source = cause.source();
            }
            msg
        })
});

fn client() -> Result<&'static RegistryClient> {
    CLIENT.as_ref().map_err(|err| eyre::eyre!("{err}"))
}

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
    Ok(client()?
        .fetch_packument_with_time_cached(name, &meta_dir())
        .await?)
}

fn all_versions_deprecated(packument: &aube_registry::Packument) -> bool {
    packument
        .versions
        .values()
        .all(|metadata| metadata.deprecated.is_some())
}

/// List a package's versions as [`VersionInfo`], semver-ascending with publish
/// timestamps. npm registry versions are strict semver (the registry enforces
/// it) but the packument's `versions` map is keyed lexically, so the keys are
/// sorted to match the order `npm view versions` produced — prefix resolution
/// (e.g. `npm:@angular/cli@19`) depends on it. Anything unparseable keeps a
/// stable position at the end.
pub(crate) async fn list_versions(name: &str) -> Result<Vec<VersionInfo>> {
    let packument = fetch_packument(name).await?;
    let all_versions_deprecated = all_versions_deprecated(&packument);
    let versions = packument.versions.iter().filter_map(|(version, metadata)| {
        (all_versions_deprecated || metadata.deprecated.is_none()).then_some(version)
    });
    Ok(sort_versions(versions)
        .into_iter()
        .map(|version| VersionInfo {
            version: version.clone(),
            created_at: packument.time.get(version).cloned(),
            prerelease: Some(is_semver_prerelease(version)),
            ..Default::default()
        })
        .collect())
}

fn sort_versions<'a>(versions: impl Iterator<Item = &'a String>) -> Vec<&'a String> {
    // npm accepts numeric components up to JavaScript's MAX_SAFE_INTEGER.
    // `versions::SemVer` stores them as u32, so use `semver::Version`'s u64
    // components and compare precedence without build metadata.
    let mut versions = versions
        .map(|version| (version, semver::Version::parse(version).ok()))
        .collect::<Vec<_>>();
    versions.sort_by(|(_, a), (_, b)| match (a, b) {
        (Some(a), Some(b)) => a.cmp_precedence(b),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
    versions.into_iter().map(|(version, _)| version).collect()
}

/// Resolve the `latest` dist-tag for a package, if the registry publishes one.
pub(crate) async fn latest_dist_tag(name: &str) -> Result<Option<String>> {
    let packument = fetch_packument(name).await?;
    let all_versions_deprecated = all_versions_deprecated(&packument);
    Ok(packument
        .dist_tags
        .get("latest")
        .filter(|version| {
            all_versions_deprecated
                || packument
                    .versions
                    .get(*version)
                    .is_none_or(|metadata| metadata.deprecated.is_none())
        })
        .cloned())
}

/// Search npm packages matching `query` through the `/-/v1/search` endpoint
/// of the registry `.npmrc` routes the query to, with the same auth as
/// metadata and tarball requests.
pub(crate) async fn search_tools(query: &str, limit: usize) -> Result<Vec<vfox::BackendTool>> {
    let timeout = Settings::get().fetch_remote_versions_timeout();
    Ok(client()?
        .search_packages(query, limit, timeout)
        .await?
        .into_iter()
        .map(|p| vfox::BackendTool {
            name: p.name,
            description: p.description,
        })
        .collect())
}

/// The registry an npm search for `query` goes to, so cached search results
/// are not reused after `.npmrc` points somewhere else.
pub(crate) fn search_registry(query: &str) -> String {
    let routing_name = if query.starts_with('@') && !query.contains('/') {
        format!("{query}/")
    } else {
        query.to_string()
    };
    CONFIG.registry_for(&routing_name).to_string()
}

/// The registry `.npmrc` and `NPM_CONFIG_*` route metadata queries for `name` to.
pub(crate) fn package_registry(name: &str) -> String {
    CONFIG.registry_for(name).to_string()
}

/// Whether `registry` is the public npm registry, whose lists the versions
/// host publishes.
pub(crate) fn is_public_registry(registry: &str) -> bool {
    registry.trim_end_matches('/') == "https://registry.npmjs.org"
}

/// Download the exact npm registry tarball for a package version, honoring
/// user npm registry and authentication configuration.
pub(crate) async fn download_tarball(name: &str, version: &str, path: &Path) -> Result<()> {
    let metadata = client()?
        .fetch_single_version_metadata(name, version)
        .await?;
    let dist = metadata
        .dist
        .ok_or_else(|| eyre::eyre!("npm package {name}@{version} has no dist metadata"))?;
    let bytes = client()?.fetch_tarball_bytes(&dist.tarball).await?;
    crate::file::write(path, bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::sort_versions;

    #[test]
    fn sort_versions_supports_npm_safe_integer_components() {
        let versions = [
            "0.52.0",
            "0.0.8999999999999999",
            "0.1.0",
            "0.0.8888888888",
            "0.0.899999999",
            "0.0.9007199254740991",
        ]
        .map(String::from);

        let sorted = sort_versions(versions.iter())
            .into_iter()
            .map(String::as_str)
            .collect::<Vec<_>>();

        assert_eq!(
            sorted,
            [
                "0.0.899999999",
                "0.0.8888888888",
                "0.0.8999999999999999",
                "0.0.9007199254740991",
                "0.1.0",
                "0.52.0",
            ]
        );
    }
}
