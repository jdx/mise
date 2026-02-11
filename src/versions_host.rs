use crate::backend::VersionInfo;
use crate::config::Settings;
use crate::http;
use crate::http::HTTP_FETCH;
use crate::plugins::core::CORE_PLUGINS;
use crate::registry::REGISTRY;
use reqwest::header::{HeaderMap, HeaderValue};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        LazyLock,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::Mutex;

/// Headers for requests to mise-versions, including CI detection
static VERSIONS_HOST_HEADERS: LazyLock<HeaderMap> = LazyLock::new(|| {
    let mut headers = HeaderMap::new();
    if ci_info::is_ci() {
        headers.insert("x-mise-ci", HeaderValue::from_static("true"));
    }
    headers
});

/// Tools that use the versions host for listing versions
/// (excludes java/python due to complex version schemes)
static PLUGINS_USE_VERSION_HOST: LazyLock<HashSet<&str>> = LazyLock::new(|| {
    CORE_PLUGINS
        .keys()
        .map(|name| name.as_str())
        .chain(REGISTRY.keys().copied())
        .filter(|name| !matches!(*name, "java" | "python"))
        .collect()
});

/// Tools that should have downloads tracked
/// (all core plugins and registry tools, including java/python)
static PLUGINS_TRACK_DOWNLOADS: LazyLock<HashSet<&str>> = LazyLock::new(|| {
    CORE_PLUGINS
        .keys()
        .map(|name| name.as_str())
        .chain(REGISTRY.keys().copied())
        .collect()
});

/// Response format from the versions host TOML endpoint
#[derive(serde::Deserialize)]
struct VersionsResponse {
    versions: indexmap::IndexMap<String, VersionEntry>,
}

#[derive(serde::Deserialize)]
struct VersionEntry {
    created_at: toml::value::Datetime,
    #[serde(default)]
    release_url: Option<String>,
}

/// List versions from the versions host (mise-versions.jdx.dev).
/// Returns Vec<VersionInfo> with created_at timestamps from the TOML endpoint.
pub async fn list_versions(tool: &str) -> eyre::Result<Option<Vec<VersionInfo>>> {
    let settings = Settings::get();
    if settings.prefer_offline()
        || !settings.use_versions_host
        || !PLUGINS_USE_VERSION_HOST.contains(tool)
    {
        return Ok(None);
    }

    static CACHE: LazyLock<Mutex<HashMap<String, Vec<VersionInfo>>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    static RATE_LIMITED: AtomicBool = AtomicBool::new(false);

    if let Some(versions) = CACHE.lock().await.get(tool) {
        return Ok(Some(versions.clone()));
    }
    if RATE_LIMITED.load(Ordering::Relaxed) {
        warn!("{tool}: skipping versions host check due to rate limit");
        return Ok(None);
    }

    // Use TOML format which includes created_at timestamps
    let url = format!("https://mise-versions.jdx.dev/tools/{}.toml", tool);
    let versions: Vec<VersionInfo> = match HTTP_FETCH
        .get_text_with_headers(&url, &VERSIONS_HOST_HEADERS)
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
                    ..Default::default()
                })
                .collect()
        }
        Err(err) => match http::error_code(&err).unwrap_or(0) {
            404 => return Ok(None),
            429 => {
                RATE_LIMITED.store(true, Ordering::Relaxed);
                warn!("{tool}: mise-versions rate limited");
                return Ok(None);
            }
            _ => return Err(err),
        },
    };

    trace!(
        "got {} {} versions from versions host",
        versions.len(),
        tool
    );

    if versions.is_empty() {
        return Ok(None);
    }

    CACHE
        .lock()
        .await
        .insert(tool.to_string(), versions.clone());
    Ok(Some(versions))
}

/// Tracks a tool installation asynchronously (fire-and-forget)
/// Tracks all core plugins and registry tools (including java/python)
pub fn track_install(tool: &str, full: &str, version: &str) {
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
    use crate::cli::version::{ARCH, OS};

    let url = "https://mise-versions.jdx.dev/api/track";

    let body = serde_json::json!({
        "tool": tool,
        "full": full,
        "version": version,
        "os": *OS,
        "arch": *ARCH
    });

    match HTTP_FETCH
        .post_json_with_headers(url, &body, &VERSIONS_HOST_HEADERS)
        .await
    {
        Ok(true) => trace!("Tracked install: {full}@{version}"),
        Ok(false) => trace!("Track request failed"),
        Err(e) => trace!("Track request error: {e}"),
    }

    Ok(())
}
