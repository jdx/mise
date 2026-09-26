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
/// (excludes java/python due to complex version schemes)
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
/// Tools that should have downloads tracked
/// (all core plugins and registry tools, including java/python)
static PLUGINS_TRACK_DOWNLOADS: LazyLock<HashSet<&str>> = LazyLock::new(|| {
    CORE_PLUGINS
        .keys()
        .map(|name| name.as_str())
        .chain(REGISTRY.keys())
        .collect()
});
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
    let ctx = VersionsHostLogContext::version_list(tool);
    let settings = Settings::get();
    if settings.prefer_offline() || !settings.use_versions_host || !lists_versions_for(tool) {
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
        Ok(body) => {
            let response: VersionsResponse = toml::from_str(&body)?;
            response
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
                .collect()
        }
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
                // fallback=true: the sole caller
                // (`Backend::list_remote_versions_with_refresh`) logs this error
                // at debug and then lists versions from the backend's upstream
                // source anyway, so a failure here is not the end of the road.
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
/// Tracks a tool installation asynchronously (fire-and-forget)
/// Tracks all core plugins and registry tools (including java/python)
pub(crate) fn track_install(tool: &str, full: &str, version: &str) {
    let settings = Settings::get();
    if settings.offline() {
        return;
    }

    // Check if tracking is enabled (also requires use_versions_host to be enabled)
    if !settings.use_versions_host || !settings.use_versions_host_track {
        return;
    }

    // Only track known tools (core plugins and registry tools)
    if !PLUGINS_TRACK_DOWNLOADS.contains(tool) {
        return;
    }

    let tool = tool.to_string();
    let full = full.to_string();
    let version = version.to_string();

    // Fire-and-forget: spawn a task that won't block installation
    tokio::spawn(async move {
        if let Err(e) = track_install_async(&tool, &full, &version).await {
            trace!("Failed to track install for {tool}@{version}: {e}");
        }
    });
}
async fn track_install_async(tool: &str, full: &str, version: &str) -> eyre::Result<()> {
    use crate::platform::{ARCH, OS};

    let url = track_install_url(tool);

    let body = serde_json::json!({
        "full": full,
        "version": version,
        "os": *OS,
        "arch": *ARCH
    });

    match HTTP_FETCH
        .post_json_with_headers(url, &body, &VERSIONS_HOST_HEADERS)
        .await
    {
        Ok(true) => log_versions_host_trace(
            VersionsHostLogContext::install_track(tool, full, version),
            "success",
            "",
        ),
        Ok(false) => log_versions_host_trace(
            VersionsHostLogContext::install_track(tool, full, version),
            "failed",
            "status=unknown",
        ),
        Err(err) => log_versions_host_trace(
            VersionsHostLogContext::install_track(tool, full, version),
            "failed",
            &format!(
                "status={} error={}",
                http::error_code(&err).unwrap_or(0),
                log_value(&err.to_string())
            ),
        ),
    }

    Ok(())
}
fn track_install_url(tool: &str) -> String {
    format!(
        "https://mise-versions.jdx.dev/api/tools/{}",
        urlencoding::encode(tool)
    )
}
fn version_list_url(tool: &str) -> String {
    format!("https://mise-versions.jdx.dev/data/{}.toml", tool)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_track_install_url_encodes_tool_path_segment() {
        assert_eq!(
            track_install_url("ubi:https://example.com/foo/bar"),
            "https://mise-versions.jdx.dev/api/tools/ubi%3Ahttps%3A%2F%2Fexample.com%2Ffoo%2Fbar"
        );
    }
    #[test]
    fn test_track_install_url_for_registered_tool_name() {
        assert_eq!(
            track_install_url("node"),
            "https://mise-versions.jdx.dev/api/tools/node"
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
