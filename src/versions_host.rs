use crate::backend::VersionInfo;
use crate::config::{Settings, SettingsExt};
use crate::http;
use crate::http::HTTP_FETCH;
use crate::plugins::core::CORE_PLUGINS;
use crate::registry::REGISTRY;
pub(crate) use mise_util::versions_host::*;
use std::{
    collections::{HashMap, HashSet},
    sync::{
        LazyLock,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::Mutex;
/// Tools that use the versions host for listing versions
/// (excludes java/python due to complex version schemes). The versions host
/// does publish python's list, but it is python-build's definitions, which is
/// only python's listing when it may compile; see
/// [`list_python_build_definitions`].
static PLUGINS_USE_VERSION_HOST: LazyLock<HashSet<&str>> = LazyLock::new(|| {
    CORE_PLUGINS
        .keys()
        .map(|name| name.as_str())
        .chain(REGISTRY.keys())
        .filter(|name| !matches!(*name, "java" | "python"))
        .collect()
});
/// Whether the versions host publishes a version list for `tool`.
pub(crate) fn lists_versions_for(tool: &str) -> bool {
    PLUGINS_USE_VERSION_HOST.contains(tool)
}
/// Response format from the versions host TOML endpoint
#[derive(serde::Deserialize)]
struct VersionsResponse {
    versions: indexmap::IndexMap<String, VersionEntry>,
}
const VERSION_LIST_RETRIES: i64 = 1;
#[derive(serde::Deserialize)]
struct VersionEntry {
    created_at: toml::value::Datetime,
    #[serde(default)]
    release_url: Option<String>,
    /// Whether this name is a moving release channel. The versions host keeps
    /// this platform-independent signal, while platform-specific checksums are
    /// fetched directly from the backend when needed.
    #[serde(default)]
    rolling: bool,
    /// Pre-release flag, when the producing source can distinguish it. Absent
    /// in old host data — and for entries from sources that don't track
    /// prereleases — which maps to `None` ("unknown") without any schema
    /// upgrade. Old mise clients that don't know about this field ignore it
    /// (toml-rs accepts unknown fields by default), so populating it in
    /// mise-versions is forward-compatible.
    #[serde(default)]
    prerelease: Option<bool>,
}
/// List versions from the versions host (mise-versions.jdx.dev).
/// Returns Vec<VersionInfo> with created_at timestamps from the TOML endpoint.
pub(crate) async fn list_versions(tool: &str) -> eyre::Result<Option<Vec<VersionInfo>>> {
    if !lists_versions_for(tool) {
        log_versions_host_trace(
            VersionsHostLogContext::version_list(tool),
            "disabled",
            "fallback=true",
        );
        return Ok(None);
    }
    fetch_version_list(tool).await
}
/// The definitions `python-build --definitions` prints, with the date each was
/// added, so listing python does not need a pyenv clone. Not the list python
/// shows when it only installs precompiled builds, which is why `python` stays
/// out of [`lists_versions_for`].
pub(crate) async fn list_python_build_definitions() -> eyre::Result<Option<Vec<VersionInfo>>> {
    fetch_version_list("python").await
}
async fn fetch_version_list(tool: &str) -> eyre::Result<Option<Vec<VersionInfo>>> {
    let ctx = VersionsHostLogContext::version_list(tool);
    let settings = Settings::get();
    if settings.prefer_offline() || !settings.use_versions_host {
        log_versions_host_trace(ctx, "disabled", "fallback=true");
        return Ok(None);
    }

    static CACHE: LazyLock<Mutex<HashMap<String, Vec<VersionInfo>>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    static RATE_LIMITED: AtomicBool = AtomicBool::new(false);

    if let Some(versions) = CACHE.lock().await.get(tool) {
        log_versions_host_trace(ctx, "cache_hit", &format!("versions={}", versions.len()));
        return Ok(Some(versions.clone()));
    }
    if RATE_LIMITED.load(Ordering::Relaxed) {
        log_versions_host_warn(ctx, "skipped_rate_limited", "fallback=true");
        return Ok(None);
    }

    // Use the static TOML asset which includes created_at timestamps.
    let url = version_list_url(tool);
    let versions: Vec<VersionInfo> = match HTTP_FETCH
        .get_text_request(&url)
        .headers(&VERSIONS_HOST_HEADERS)
        .retries(VERSION_LIST_RETRIES)
        .send()
        .await
    {
        Ok(body) => parse_version_list(&body)?,
        Err(err) => match http::error_code(&err).unwrap_or(0) {
            404 => {
                log_versions_host_trace(ctx, "not_found", "status=404 fallback=true");
                return Ok(None);
            }
            429 => {
                RATE_LIMITED.store(true, Ordering::Relaxed);
                log_versions_host_warn(ctx, "rate_limited", "status=429 fallback=true");
                return Ok(None);
            }
            status => {
                // fallback=true: every caller logs this error at debug and then
                // lists versions from the upstream source anyway (python lists
                // python-build from a pyenv clone), so a failure here is not the
                // end of the road.
                log_versions_host_warn(
                    ctx,
                    "failed",
                    &format!(
                        "status={status} fallback=true error={}",
                        log_value(&err.to_string())
                    ),
                );
                return Err(err);
            }
        },
    };

    if versions.is_empty() {
        log_versions_host_trace(ctx, "empty", "fallback=true");
        return Ok(None);
    }

    log_versions_host_trace(ctx, "success", &format!("versions={}", versions.len()));

    CACHE
        .lock()
        .await
        .insert(tool.to_string(), versions.clone());
    Ok(Some(versions))
}
/// A `data/<tool>.toml` body as the version list it describes, in file order.
pub(crate) fn parse_version_list(body: &str) -> eyre::Result<Vec<VersionInfo>> {
    let response: VersionsResponse = toml::from_str(body)?;
    Ok(response
        .versions
        .into_iter()
        .map(|(version, entry)| VersionInfo {
            version,
            created_at: Some(entry.created_at.to_string()),
            release_url: entry.release_url,
            rolling: entry.rolling,
            prerelease: entry.prerelease,
            ..Default::default()
        })
        .collect())
}
/// Fetch and unpack a gzipped python-build-standalone index
/// (`python-precompiled-<arch>-<os>.gz`).
pub(crate) async fn python_precompiled(file: &str) -> eyre::Result<String> {
    let gz = HTTP_FETCH
        .get_bytes_with_headers(python_precompiled_url(file), VERSIONS_HOST_HEADERS.clone())
        .await?;
    gunzip_text(gz.as_ref(), http::MAX_BODY_BYTES)
}

/// Unpacks gzipped text held in memory, failing once it passes `limit` bytes:
/// a few kilobytes of gzip can unpack to gigabytes.
fn gunzip_text(gz: &[u8], limit: usize) -> eyre::Result<String> {
    use std::io::Read;

    let mut raw = String::new();
    flate2::read::GzDecoder::new(gz)
        .take(limit as u64 + 1)
        .read_to_string(&mut raw)?;
    eyre::ensure!(
        raw.len() <= limit,
        "unpacked index is larger than {limit} bytes"
    );
    Ok(raw)
}
fn python_precompiled_url(file: &str) -> String {
    format!("https://mise-versions.jdx.dev/data/{file}")
}
fn version_list_url(tool: &str) -> String {
    format!("https://mise-versions.jdx.dev/data/{}.toml", tool)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_python_is_left_to_its_own_listing() {
        // The shared listing would show python-build's definitions even when python only installs
        // precompiled builds; python asks for them itself when it may compile.
        assert!(!lists_versions_for("python"));
        assert!(lists_versions_for("node"));
    }
    #[tokio::test]
    async fn test_python_build_definitions_respect_use_versions_host() {
        use crate::config::settings::SettingsPartial;
        use confique::Layer;

        let _guard = crate::test::SettingsGuard::lock();
        let mut partial = SettingsPartial::empty();
        partial.use_versions_host = Some(false);
        Settings::reset(Some(partial));
        assert!(list_python_build_definitions().await.unwrap().is_none());
    }
    #[tokio::test]
    async fn test_python_build_definitions_respect_prefer_offline() {
        use crate::config::settings::SettingsPartial;
        use confique::Layer;

        let _guard = crate::test::SettingsGuard::lock();
        let mut partial = SettingsPartial::empty();
        partial.prefer_offline = Some(true);
        Settings::reset(Some(partial));
        assert!(list_python_build_definitions().await.unwrap().is_none());
    }

    #[test]
    fn test_gunzip_text_stops_at_its_limit() {
        use std::io::Write;

        let gzip = |text: &[u8]| {
            let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
            gz.write_all(text).unwrap();
            gz.finish().unwrap()
        };
        assert_eq!(gunzip_text(&gzip(b"3.12.0"), 6).unwrap(), "3.12.0");
        assert!(gunzip_text(&gzip(b"3.12.0\n"), 6).is_err());
    }
    #[test]
    fn test_python_precompiled_url_uses_static_data_path() {
        assert_eq!(
            python_precompiled_url("python-precompiled-aarch64-apple-darwin.gz"),
            "https://mise-versions.jdx.dev/data/python-precompiled-aarch64-apple-darwin.gz"
        );
    }
    #[test]
    fn test_version_list_url_uses_static_asset_path() {
        assert_eq!(
            version_list_url("node"),
            "https://mise-versions.jdx.dev/data/node.toml"
        );
    }
    #[test]
    fn test_version_entry_deserializes_rolling_metadata() {
        let response: VersionsResponse = toml::from_str(
            r#"
[versions]
"1.0.0" = { created_at = 2026-01-01T00:00:00Z }
"nightly" = { created_at = 2026-01-02T00:00:00Z, rolling = true }
"#,
        )
        .unwrap();

        assert!(!response.versions["1.0.0"].rolling);
        assert!(response.versions["nightly"].rolling);
    }
}
