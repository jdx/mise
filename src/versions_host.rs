use crate::cli::args::BackendArg;
use crate::config::SETTINGS;
use crate::http::HTTP_FETCH;
use crate::plugins::core::CORE_PLUGINS;
use crate::registry::REGISTRY;
use crate::{http, registry};
use once_cell::sync::Lazy;
use std::collections::HashSet;
use url::Url;

static PLUGINS_USE_VERSION_HOST: Lazy<HashSet<&str>> = Lazy::new(|| {
    CORE_PLUGINS
        .iter()
        .map(|(name, _)| name.as_str())
        .chain(REGISTRY.keys().copied())
        .filter(|name| !matches!(*name, "java" | "python"))
        .collect()
});

pub fn list_versions(ba: &BackendArg) -> eyre::Result<Option<Vec<String>>> {
    if !SETTINGS.use_versions_host
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
    let response = match SETTINGS.paranoid {
        true => HTTP_FETCH.get_text(format!("https://mise-versions.jdx.dev/{}", &ba.short)),
        false => HTTP_FETCH.get_text(format!("http://mise-versions.jdx.dev/{}", &ba.short)),
    };
    let versions =
        // using http is not a security concern and enabling tls makes mise significantly slower
        match response {
            Err(err) if http::error_code(&err) == Some(404) => return Ok(None),
            res => res?,
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
