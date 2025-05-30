use crate::cli::args::BackendArg;
use crate::config::Settings;
use crate::http::HTTP_FETCH;
use crate::plugins::core::CORE_PLUGINS;
use crate::registry::REGISTRY;
use crate::{http, registry};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        LazyLock,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::Mutex;
use url::Url;

static PLUGINS_USE_VERSION_HOST: LazyLock<HashSet<&str>> = LazyLock::new(|| {
    CORE_PLUGINS
        .keys()
        .map(|name| name.as_str())
        .chain(REGISTRY.keys().copied())
        .filter(|name| !matches!(*name, "java" | "python"))
        .collect()
});

pub async fn list_versions(ba: &BackendArg) -> eyre::Result<Option<Vec<String>>> {
    if !Settings::get().use_versions_host
        || ba.short.contains(':')
        || !PLUGINS_USE_VERSION_HOST.contains(ba.short.as_str())
    {
        return Ok(None);
    }
    // ensure that we're using a default shorthand plugin
    if let Some(plugin) = ba.backend()?.plugin() {
        if let Ok(Some(remote_url)) = plugin.get_remote_url() {
            let normalized_remote = normalize_remote(&remote_url).unwrap_or("INVALID_URL".into());
            let shorthand_remote = REGISTRY
                .get(plugin.name())
                .and_then(|rt| rt.backends().first().map(|b| registry::full_to_url(b)))
                .unwrap_or_default();
            if normalized_remote != normalize_remote(&shorthand_remote).unwrap_or_default() {
                trace!(
                    "Skipping versions host check for {} because it has a non-default remote",
                    ba.short
                );
                return Ok(None);
            }
        }
    }
    static CACHE: LazyLock<Mutex<HashMap<String, Vec<String>>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    static RATE_LIMITED: AtomicBool = AtomicBool::new(false);
    if let Some(versions) = CACHE.lock().await.get(ba.short.as_str()) {
        return Ok(Some(versions.clone()));
    }
    if RATE_LIMITED.load(Ordering::Relaxed) {
        warn!("{ba}: skipping versions host check due to rate limit");
        return Ok(None);
    }
    let url = match Settings::get().paranoid {
        true => format!("https://mise-versions.jdx.dev/{}", &ba.short),
        false => format!("http://mise-versions.jdx.dev/{}", &ba.short),
    };
    let versions =
        // using http is not a security concern and enabling tls makes mise significantly slower
        match HTTP_FETCH.get_text(url).await {
            Ok(res) => res,
            Err(err) => {
                match http::error_code(&err).unwrap_or(0) {
                    404 => return Ok(None),
                    429 => {
                        RATE_LIMITED.store(true, Ordering::Relaxed);
                        warn!("{ba}: mise-version rate limited");
                        return Ok(None);
                    }
                    _ => return Err(err),
                }
            }
        };
    let versions = versions
        .lines()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect::<Vec<String>>();
    trace!(
        "got {} {} versions from versions host",
        versions.len(),
        &ba.short
    );
    CACHE
        .lock()
        .await
        .insert(ba.short.clone(), versions.clone());
    match versions.is_empty() {
        true => Ok(None),
        false => Ok(Some(versions)),
    }
}

fn normalize_remote(remote: &str) -> eyre::Result<String> {
    let url = Url::parse(remote)?;
    let host = url.host_str().unwrap();
    let path = url.path().trim_end_matches(".git");
    Ok(format!("{host}{path}"))
}
